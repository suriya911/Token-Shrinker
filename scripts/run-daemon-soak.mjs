import { appendFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { execFileSync } from "node:child_process";

const binary = resolve(
  process.env.TOKEN_SHRINKER_BINARY ??
    (process.platform === "win32"
      ? "target/release/token-shrinker.exe"
      : "target/release/token-shrinker"),
);
const minutes = Number.parseInt(process.env.TOKEN_SHRINKER_SOAK_MINUTES ?? "350", 10);
const intervalMs = Number.parseInt(process.env.TOKEN_SHRINKER_SOAK_INTERVAL_MS ?? "60000", 10);
const logPath = resolve(process.env.TOKEN_SHRINKER_SOAK_LOG ?? "soak/daemon-soak.jsonl");

if (!existsSync(binary)) {
  throw new Error(`soak binary does not exist: ${binary}`);
}
if (!Number.isInteger(minutes) || minutes < 1 || minutes > 350) {
  throw new Error("TOKEN_SHRINKER_SOAK_MINUTES must be an integer from 1 through 350");
}
if (!Number.isInteger(intervalMs) || intervalMs < 1000) {
  throw new Error("TOKEN_SHRINKER_SOAK_INTERVAL_MS must be at least 1000");
}

mkdirSync(dirname(logPath), { recursive: true });
const startedAt = Date.now();
const deadline = startedAt + minutes * 60_000;
let samples = 0;
let failures = 0;

function run(args) {
  try {
    const stdout = execFileSync(binary, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
    return { exitCode: 0, stdout, value: stdout ? JSON.parse(stdout) : null };
  } catch (error) {
    const stdout = error.stdout?.toString().trim() ?? "";
    return { exitCode: error.status ?? 1, stdout, stderr: error.stderr?.toString().trim() ?? "" };
  }
}

function rssKb(pid) {
  if (process.platform !== "linux" || !pid) return null;
  try {
    const status = readFileSync(`/proc/${pid}/status`, "utf8");
    const match = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
    return match ? Number(match[1]) : null;
  } catch {
    return null;
  }
}

function record(entry) {
  appendFileSync(logPath, `${JSON.stringify({ timestamp: new Date().toISOString(), ...entry })}\n`);
}

const start = run(["start"]);
record({ event: "start", result: start });
if (start.exitCode !== 0) throw new Error(`daemon start failed: ${start.stderr || start.stdout}`);

try {
  while (Date.now() < deadline) {
    const status = run(["status"]);
    const value = status.value;
    const healthy = status.exitCode === 0 && value?.running === true;
    samples += 1;
    if (!healthy) failures += 1;
    record({
      event: "health",
      elapsedSeconds: Math.round((Date.now() - startedAt) / 1000),
      healthy,
      status: value ?? { exitCode: status.exitCode, stderr: status.stderr, stdout: status.stdout },
      rssKb: rssKb(value?.pid),
    });
    if (!healthy) throw new Error(`daemon health check failed at sample ${samples}`);
    const remaining = deadline - Date.now();
    if (remaining > 0) await new Promise((resolvePromise) => setTimeout(resolvePromise, Math.min(intervalMs, remaining)));
  }
} finally {
  const stop = run(["stop"]);
  record({ event: "stop", result: stop, samples, failures, durationMinutes: minutes });
  if (stop.exitCode !== 0) failures += 1;
}

record({ event: "complete", samples, failures, durationMinutes: minutes });
if (failures > 0) process.exitCode = 1;
