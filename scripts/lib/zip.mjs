/**
 * Minimal, dependency-free reader for the ZIP central directory.
 *
 * The verification scripts previously shelled out to `tar` on Windows and `unzip` elsewhere.
 * That assumed the `tar` on PATH was bsdtar; on any machine where Git's GNU tar comes first it
 * fails with "This does not look like a tar archive" even though the archive is valid. Reading
 * the archive in-process removes the external dependency and behaves the same on every platform.
 */
import { inflateRawSync } from "node:zlib";

const EOCD_SIGNATURE = 0x06054b50;
const CENTRAL_SIGNATURE = 0x02014b50;
const LOCAL_SIGNATURE = 0x04034b50;
const EOCD_MIN_SIZE = 22;
const MAX_COMMENT = 0xffff;

function findEndOfCentralDirectory(buffer) {
  const earliest = Math.max(0, buffer.length - EOCD_MIN_SIZE - MAX_COMMENT);
  for (let offset = buffer.length - EOCD_MIN_SIZE; offset >= earliest; offset -= 1) {
    if (buffer.readUInt32LE(offset) === EOCD_SIGNATURE) return offset;
  }
  throw new Error("not a ZIP archive: end-of-central-directory record was not found");
}

/** Reads every central-directory entry as `{ name, method, compressedSize, localOffset }`. */
export function readZipEntries(buffer) {
  const eocd = findEndOfCentralDirectory(buffer);
  const total = buffer.readUInt16LE(eocd + 10);
  let offset = buffer.readUInt32LE(eocd + 16);
  if (offset === 0xffffffff) throw new Error("ZIP64 archives are not supported by this reader");
  const entries = [];
  for (let index = 0; index < total; index += 1) {
    if (buffer.readUInt32LE(offset) !== CENTRAL_SIGNATURE) {
      throw new Error(`corrupt ZIP central directory at entry ${index}`);
    }
    const nameLength = buffer.readUInt16LE(offset + 28);
    const extraLength = buffer.readUInt16LE(offset + 30);
    const commentLength = buffer.readUInt16LE(offset + 32);
    entries.push({
      name: buffer.toString("utf8", offset + 46, offset + 46 + nameLength).replaceAll("\\", "/"),
      method: buffer.readUInt16LE(offset + 10),
      compressedSize: buffer.readUInt32LE(offset + 20),
      localOffset: buffer.readUInt32LE(offset + 42),
    });
    offset += 46 + nameLength + extraLength + commentLength;
  }
  return entries;
}

/** Lists archive member names, matching what `tar -tf` / `unzip -Z1` printed before. */
export function listZipEntries(buffer) {
  return readZipEntries(buffer).map((entry) => entry.name);
}

/** Returns one member's bytes, inflating it when the archive stored it deflated. */
export function readZipEntry(buffer, name) {
  const entry = readZipEntries(buffer).find((candidate) => candidate.name === name);
  if (!entry) throw new Error(`archive member ${name} is missing`);
  const local = entry.localOffset;
  if (buffer.readUInt32LE(local) !== LOCAL_SIGNATURE) {
    throw new Error(`corrupt local header for ${name}`);
  }
  const start = local + 30 + buffer.readUInt16LE(local + 26) + buffer.readUInt16LE(local + 28);
  const body = buffer.subarray(start, start + entry.compressedSize);
  if (entry.method === 0) return Buffer.from(body);
  if (entry.method === 8) return inflateRawSync(body);
  throw new Error(`archive member ${name} uses unsupported compression method ${entry.method}`);
}
