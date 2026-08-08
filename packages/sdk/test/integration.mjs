import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { access, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import process from "node:process";
import test from "node:test";
import { DaemonTransport, StdioTransport, TokenShrinkerClient } from "../dist/index.js";

const binary = process.env.TOKEN_SHRINKER_BINARY ??
  resolve("..", "..", "target", "debug", process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker");

test("stdio transport initializes and invokes structured tools", async () => {
  await access(binary);
  const transport = await StdioTransport.connect({ binaryPath: binary });
  try {
    const result = await transport.call("token_shrinker_capabilities", {});
    assert.equal(result.protocolVersion, "1.0");
    assert.equal(result.data.tools.length, 9);
  } finally {
    await transport.close();
  }
});

test("stdio transport reports a typed process error for a missing binary", async () => {
  await assert.rejects(
    StdioTransport.connect({ binaryPath: join(tmpdir(), "missing-token-shrinker-binary") }),
    (error) => error?.code === "stdio-process",
  );
});

test("daemon transport frames requests, authenticates, and reports typed errors", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-shrinker-sdk-"));
  const endpoint = process.platform === "win32"
    ? "\\\\.\\pipe\\token-shrinker-sdk-test-" + process.pid
    : join(directory, "fake.sock");
  const token = "a".repeat(64);
  await writeFile(join(directory, "daemon.json"), JSON.stringify({
    endpoint, pid: process.pid, startedAtMs: Date.now(),
    protocolVersion: { major: 1, minor: 0 }, authToken: token,
  }));
  const { createServer } = await import("node:net");
  const server = createServer((socket) => {
    let data = Buffer.alloc(0);
    socket.on("data", (chunk) => {
      data = Buffer.concat([data, chunk]);
      if (data.length < 4) return;
      const length = data.readUInt32BE(0);
      if (data.length < length + 4) return;
      const request = JSON.parse(data.subarray(4, 4 + length));
      const result = { protocolVersion: "1.0", requestId: request.id,
        warnings: [], data: { ok: request.authToken === token } };
      const response = Buffer.from(JSON.stringify({
        jsonrpc: "2.0", id: request.id, protocolVersion: { major: 1, minor: 0 },
        result, error: null,
      }));
      const header = Buffer.alloc(4); header.writeUInt32BE(response.length);
      socket.end(Buffer.concat([header, response]));
    });
  });
  await new Promise((resolveReady, reject) => {
    server.once("error", reject); server.listen(endpoint, resolveReady);
  });
  try {
    const transport = new DaemonTransport({ discoveryPath: join(directory, "daemon.json") });
    const result = await transport.call("capabilities", {});
    assert.equal(result.data.ok, true);
    await transport.close();
  } finally {
    await new Promise((resolveClose) => server.close(resolveClose));
    await rm(directory, { recursive: true, force: true });
  }
});

test("daemon transport integrates with the native service", async () => {
  const directory = await mkdtemp(join(tmpdir(), "token-shrinker-native-daemon-"));
  const runtime = join(directory, "runtime");
  const data = join(directory, "data");
  const child = spawn(binary, ["__daemon"], {
    env: { ...process.env, TOKEN_SHRINKER_RUNTIME_DIR: runtime, TOKEN_SHRINKER_DATA_DIR: data },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  const discoveryPath = join(runtime, "daemon.json");
  try {
    let ready = false;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      try { await access(discoveryPath); ready = true; break; } catch {}
      await new Promise((resolveWait) => setTimeout(resolveWait, 20));
    }
    assert.equal(ready, true, "native daemon did not become ready");
    const transport = new DaemonTransport({ discoveryPath });
    const capabilities = await transport.call("token_shrinker_capabilities", {});
    assert.equal(capabilities.data.tools.length, 9);
    const discovery = JSON.parse(await (await import("node:fs/promises")).readFile(discoveryPath, "utf8"));
    const stopTransport = new DaemonTransport({ discoveryPath });
    await stopTransport.call("daemon.shutdown", {});
    assert.equal(discovery.protocolVersion.major, 1);
    await new Promise((resolveExit, rejectExit) => {
      const timer = setTimeout(() => rejectExit(new Error("daemon did not exit")), 3_000);
      child.once("exit", () => { clearTimeout(timer); resolveExit(); });
    });
  } finally {
    if (child.exitCode === null) child.kill();
    await rm(directory, { recursive: true, force: true });
  }
});
