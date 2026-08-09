import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join, resolve } from "node:path";

const directory = resolve(process.argv[2] ?? "release");
const output = join(directory, "SHA256SUMS.txt");
const files = (await readdir(directory, { withFileTypes: true }))
  .filter((entry) => entry.isFile() && entry.name !== basename(output))
  .map((entry) => entry.name)
  .sort();
const lines = [];
for (const file of files) {
  const digest = createHash("sha256").update(await readFile(join(directory, file))).digest("hex");
  lines.push(`${digest} *${file}`);
}
await writeFile(output, lines.join("\n") + "\n");
console.log(`wrote ${files.length} checksums to ${output}`);
