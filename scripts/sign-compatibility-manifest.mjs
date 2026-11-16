import { createPrivateKey, sign } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const [inputArg = "compatibility-manifest.unsigned.json", outputArg = "compatibility-manifest.json"] = process.argv.slice(2);
const inputPath = resolve(inputArg);
const outputPath = resolve(outputArg);
const keyText = process.env.TOKEN_SHRINKER_ED25519_PRIVATE_KEY;
const keyId = process.env.TOKEN_SHRINKER_SIGNING_KEY_ID;
if (!keyText || !keyId) {
  throw new Error("set TOKEN_SHRINKER_ED25519_PRIVATE_KEY and TOKEN_SHRINKER_SIGNING_KEY_ID; signing keys are never read from repository files");
}

function rawKeyBytes(value) {
  const text = value.trim();
  if (/^[0-9a-f]{64}$/iu.test(text)) return Buffer.from(text, "hex");
  const decoded = Buffer.from(text, "base64");
  if (decoded.length !== 32) throw new Error("Ed25519 private key must be a 32-byte seed in hex or base64");
  return decoded;
}

function privateKeyFromSeed(seed) {
  return createPrivateKey({
    key: Buffer.concat([
      Buffer.from("302e020100300506032b657004220420", "hex"),
      seed,
    ]),
    format: "der",
    type: "pkcs8",
  });
}

function canonicalUnsigned(manifest) {
  return {
    schemaVersion: manifest.schemaVersion,
    generatedAt: manifest.generatedAt,
    expiresAt: manifest.expiresAt,
    components: manifest.components.map((component) => ({
      id: component.id,
      authoritativeSource: component.authoritativeSource,
      ownership: component.ownership,
      releases: component.releases.map((release) => ({
        version: release.version,
        channel: release.channel,
        protocol: { min: release.protocol.min, max: release.protocol.max },
        artifacts: release.artifacts.map((artifact) => ({
          platform: artifact.platform,
          url: artifact.url,
          sha256: artifact.sha256,
        })),
      })),
    })),
  };
}

const manifest = JSON.parse(await readFile(inputPath, "utf8"));
if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.components)) {
  throw new Error("input must be an unsigned compatibility manifest with schemaVersion 1 and components");
}
const unsigned = canonicalUnsigned(manifest);
const payload = Buffer.from(JSON.stringify(unsigned));
const signature = sign(null, payload, privateKeyFromSeed(rawKeyBytes(keyText))).toString("base64");
await writeFile(outputPath, `${JSON.stringify({ ...unsigned, signature: { algorithm: "ed25519", keyId, value: signature } }, null, 2)}\n`);
console.log(`signed compatibility manifest: ${outputPath}`);
