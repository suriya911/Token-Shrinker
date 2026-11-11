import assert from "node:assert/strict";
import test from "node:test";
import { deflateRawSync } from "node:zlib";
import { listZipEntries, readZipEntry } from "../lib/zip.mjs";

/**
 * Builds a ZIP archive in memory so these tests need no checked-in binary fixture.
 * `method` is 0 (stored) or 8 (deflate), matching the two cases the reader supports.
 */
function buildZip(members) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const { name, content, method } of members) {
    const nameBytes = Buffer.from(name, "utf8");
    const raw = Buffer.from(content, "utf8");
    const body = method === 8 ? deflateRawSync(raw) : raw;

    const local = Buffer.alloc(30 + nameBytes.length);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(method, 8);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(raw.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    nameBytes.copy(local, 30);
    locals.push(local, body);

    const entry = Buffer.alloc(46 + nameBytes.length);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(method, 10);
    entry.writeUInt32LE(body.length, 20);
    entry.writeUInt32LE(raw.length, 24);
    entry.writeUInt16LE(nameBytes.length, 28);
    entry.writeUInt32LE(offset, 42);
    nameBytes.copy(entry, 46);
    central.push(entry);

    offset += local.length + body.length;
  }
  const localBytes = Buffer.concat(locals);
  const centralBytes = Buffer.concat(central);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(members.length, 8);
  eocd.writeUInt16LE(members.length, 10);
  eocd.writeUInt32LE(centralBytes.length, 12);
  eocd.writeUInt32LE(localBytes.length, 16);
  return Buffer.concat([localBytes, centralBytes, eocd]);
}

const manifest = JSON.stringify({ name: "token-shrinker", contributes: { commands: [1, 2, 3] } });

test("lists every archive member without shelling out to tar or unzip", () => {
  const archive = buildZip([
    { name: "extension.vsixmanifest", content: "<PackageManifest />", method: 8 },
    { name: "extension/package.json", content: manifest, method: 8 },
    { name: "extension/dist/index.js", content: "export default 1;", method: 0 },
  ]);
  assert.deepEqual(listZipEntries(archive), [
    "extension.vsixmanifest", "extension/package.json", "extension/dist/index.js",
  ]);
});

test("reads a deflated member back to its exact original bytes", () => {
  const archive = buildZip([{ name: "extension/package.json", content: manifest, method: 8 }]);
  assert.equal(readZipEntry(archive, "extension/package.json").toString("utf8"), manifest);
  assert.equal(JSON.parse(readZipEntry(archive, "extension/package.json")).contributes.commands.length, 3);
});

test("reads a stored member that was never compressed", () => {
  const archive = buildZip([{ name: "extension/dist/index.js", content: "export default 1;", method: 0 }]);
  assert.equal(readZipEntry(archive, "extension/dist/index.js").toString("utf8"), "export default 1;");
});

test("rejects a file that is not a ZIP archive", () => {
  assert.throws(() => listZipEntries(Buffer.from("this is plain text, not an archive")),
    /not a ZIP archive/);
});

test("reports a missing member by name instead of returning empty content", () => {
  const archive = buildZip([{ name: "extension/package.json", content: manifest, method: 8 }]);
  assert.throws(() => readZipEntry(archive, "extension/dist/index.js"),
    /extension\/dist\/index\.js is missing/);
});

test("finds the central directory even when the archive carries a trailing comment", () => {
  const archive = buildZip([{ name: "a.txt", content: "hello", method: 0 }]);
  const commented = Buffer.concat([archive, Buffer.from("trailing comment bytes")]);
  commented.writeUInt16LE(22, commented.length - 22 - 22 + 20);
  assert.deepEqual(listZipEntries(commented), ["a.txt"]);
});
