/**
 * Minimal, dependency-free reader for gzipped tar archives.
 *
 * Like the ZIP reader beside it, this exists because shelling out to `tar` is not portable.
 * GNU tar treats a Windows path such as `C:\tmp\pkg.tgz` as the remote host `C`, failing with
 * "Cannot connect to C: resolve failed" on any machine where Git's tar precedes bsdtar on PATH.
 * Reading the archive in-process removes the external dependency and the PATH sensitivity.
 */
import { gunzipSync } from "node:zlib";

const BLOCK = 512;
const NAME = { offset: 0, length: 100 };
const SIZE = { offset: 124, length: 12 };
const TYPE_FLAG = 156;
const MAGIC = { offset: 257, length: 6 };
const PREFIX = { offset: 345, length: 155 };
const LONG_NAME_TYPE = "L";

const readString = (block, { offset, length }) => {
  const raw = block.subarray(offset, offset + length);
  const end = raw.indexOf(0);
  return raw.toString("utf8", 0, end === -1 ? raw.length : end);
};

function readOctal(block, field) {
  const text = readString(block, field).trim();
  if (text === "") return 0;
  const value = Number.parseInt(text, 8);
  if (!Number.isFinite(value) || value < 0) throw new Error("corrupt tar numeric field");
  return value;
}

const isZeroBlock = (block) => block.every((byte) => byte === 0);

/** Reads every archive member as `{ name, type, content }`, transparently gunzipping if needed. */
export function readTarEntries(input) {
  const buffer = input[0] === 0x1f && input[1] === 0x8b ? gunzipSync(input) : Buffer.from(input);
  const entries = [];
  let offset = 0;
  let pendingLongName = null;
  while (offset + BLOCK <= buffer.length) {
    const header = buffer.subarray(offset, offset + BLOCK);
    if (isZeroBlock(header)) break;
    const magic = readString(header, MAGIC);
    if (magic !== "ustar" && magic !== "ustar  ") {
      throw new Error("not a tar archive: ustar magic was not found");
    }
    const size = readOctal(header, SIZE);
    const type = String.fromCharCode(header[TYPE_FLAG] || 0x30);
    const prefix = readString(header, PREFIX);
    const base = readString(header, NAME);
    const body = buffer.subarray(offset + BLOCK, offset + BLOCK + size);
    offset += BLOCK + Math.ceil(size / BLOCK) * BLOCK;

    if (type === LONG_NAME_TYPE) {
      const end = body.indexOf(0);
      pendingLongName = body.toString("utf8", 0, end === -1 ? body.length : end);
      continue;
    }
    const name = pendingLongName ?? (prefix ? `${prefix}/${base}` : base);
    pendingLongName = null;
    entries.push({ name: name.replaceAll("\\", "/"), type, content: Buffer.from(body) });
  }
  return entries;
}

/** Lists archive member names, matching what `tar -tzf` printed before. */
export function listTarEntries(input) {
  return readTarEntries(input).map((entry) => entry.name);
}

/** Returns one member's bytes. */
export function readTarEntry(input, name) {
  const entry = readTarEntries(input).find((candidate) => candidate.name === name);
  if (!entry) throw new Error(`archive member ${name} is missing`);
  return entry.content;
}
