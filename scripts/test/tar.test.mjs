import assert from "node:assert/strict";
import test from "node:test";
import { gzipSync } from "node:zlib";
import { listTarEntries, readTarEntries, readTarEntry } from "../lib/tar.mjs";

const BLOCK = 512;

/** Builds a ustar archive in memory so these tests need no checked-in binary fixture. */
function buildTar(members) {
  const blocks = [];
  for (const { name, content, type = "0", prefix = "" } of members) {
    const body = Buffer.from(content, "utf8");
    const header = Buffer.alloc(BLOCK);
    header.write(name, 0, 100, "utf8");
    header.write("000644 \0", 100, 8, "utf8");
    header.write(body.length.toString(8).padStart(11, "0") + "\0", 124, 12, "utf8");
    header.write(type, 156, 1, "utf8");
    header.write("ustar\0", 257, 6, "utf8");
    header.write("00", 263, 2, "utf8");
    if (prefix) header.write(prefix, 345, 155, "utf8");
    // The checksum field is treated as spaces while the checksum itself is computed.
    header.write("        ", 148, 8, "utf8");
    let sum = 0;
    for (const byte of header) sum += byte;
    header.write(sum.toString(8).padStart(6, "0") + "\0 ", 148, 8, "utf8");

    const padded = Buffer.alloc(Math.ceil(body.length / BLOCK) * BLOCK);
    body.copy(padded);
    blocks.push(header, padded);
  }
  blocks.push(Buffer.alloc(BLOCK * 2));
  return Buffer.concat(blocks);
}

const manifest = JSON.stringify({ name: "@token-shrinker/cli", version: "0.2.0" });

test("lists members of a plain tar archive", () => {
  const archive = buildTar([
    { name: "package/package.json", content: manifest },
    { name: "package/LICENSE", content: "Apache-2.0" },
    { name: "package/bin/token-shrinker.exe", content: "MZ binary" },
  ]);
  assert.deepEqual(listTarEntries(archive), [
    "package/package.json", "package/LICENSE", "package/bin/token-shrinker.exe",
  ]);
});

test("gunzips a .tgz transparently, the shape npm pack produces", () => {
  const archive = gzipSync(buildTar([{ name: "package/package.json", content: manifest }]));
  assert.deepEqual(listTarEntries(archive), ["package/package.json"]);
  assert.equal(readTarEntry(archive, "package/package.json").toString("utf8"), manifest);
});

test("reads a member whose bytes do not fill the final 512-byte block", () => {
  const content = "x".repeat(BLOCK + 7);
  const archive = buildTar([
    { name: "package/a.txt", content },
    { name: "package/b.txt", content: "second entry must still be found" },
  ]);
  assert.equal(readTarEntry(archive, "package/a.txt").toString("utf8"), content);
  assert.equal(readTarEntry(archive, "package/b.txt").toString("utf8"),
    "second entry must still be found");
});

test("joins the ustar prefix field onto long paths", () => {
  const archive = buildTar([
    { name: "index.d.ts.map", prefix: "package/dist/deeply/nested", content: "{}" },
  ]);
  assert.deepEqual(listTarEntries(archive), ["package/dist/deeply/nested/index.d.ts.map"]);
});

test("resolves a GNU long-name entry to the real path", () => {
  const longPath = "package/dist/" + "segment/".repeat(14) + "index.js";
  const archive = buildTar([
    { name: "././@LongLink", type: "L", content: longPath + "\0" },
    { name: longPath.slice(0, 100), content: "export default 1;" },
  ]);
  assert.deepEqual(listTarEntries(archive), [longPath]);
  assert.equal(readTarEntry(archive, longPath).toString("utf8"), "export default 1;");
});

test("preserves the directory type flag so callers can tell files from directories", () => {
  const archive = buildTar([
    { name: "package/dist/", type: "5", content: "" },
    { name: "package/dist/index.js", content: "export default 1;" },
  ]);
  const types = Object.fromEntries(readTarEntries(archive).map((e) => [e.name, e.type]));
  assert.equal(types["package/dist/"], "5");
  assert.equal(types["package/dist/index.js"], "0");
});

test("rejects a file that is not a tar archive", () => {
  assert.throws(() => listTarEntries(Buffer.alloc(BLOCK, 0x41)), /not a tar archive/);
});

test("reports a missing member by name instead of returning empty content", () => {
  const archive = buildTar([{ name: "package/package.json", content: manifest }]);
  assert.throws(() => readTarEntry(archive, "package/bin/token-shrinker.exe"),
    /package\/bin\/token-shrinker\.exe is missing/);
});
