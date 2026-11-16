import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const root = resolve(process.argv[2] ?? "release-candidates");
const fail = (message) => {
  throw new Error(`Release-candidate verification failed: ${message}`);
};

async function filesUnder(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await filesUnder(path));
    else files.push(path);
  }
  return files;
}

const allFiles = await filesUnder(root);
const checksumFiles = allFiles.filter((path) => path.endsWith("SHA256SUMS.txt"));
if (checksumFiles.length < 4) fail(`expected at least 4 platform checksum files, found ${checksumFiles.length}`);

let checked = 0;
for (const checksumPath of checksumFiles) {
  const directory = join(checksumPath, "..");
  const lines = (await readFile(checksumPath, "utf8")).trim().split(/\r?\n/).filter(Boolean);
  if (lines.length === 0) fail(`empty checksum file ${checksumPath}`);
  for (const line of lines) {
    const match = line.match(/^([0-9a-f]{64}) \*([^\s]+)$/u);
    if (!match) fail(`malformed checksum line in ${checksumPath}: ${line}`);
    const filePath = join(directory, match[2]);
    const actual = createHash("sha256").update(await readFile(filePath)).digest("hex");
    if (actual !== match[1]) fail(`checksum mismatch for ${filePath}`);
    checked += 1;
  }
}

console.log(`Release-candidate artifacts verified: ${checksumFiles.length} checksum sets, ${checked} files`);
