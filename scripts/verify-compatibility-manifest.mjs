import { createPublicKey, verify } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const manifestPath = resolve(process.argv[2] ?? "compatibility-manifest.json");
const publicKeyText = process.env.TOKEN_SHRINKER_ED25519_PUBLIC_KEY;
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
if (!publicKeyText) throw new Error("set TOKEN_SHRINKER_ED25519_PUBLIC_KEY for independent verification");

function publicKeyFromRaw(value) {
  const text = value.trim();
  const raw = /^[0-9a-f]{64}$/iu.test(text) ? Buffer.from(text, "hex") : Buffer.from(text, "base64");
  if (raw.length !== 32) throw new Error("Ed25519 public key must be 32 bytes in hex or base64");
  return createPublicKey({
    key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), raw]),
    format: "der",
    type: "spki",
  });
}

const unsigned = {
  schemaVersion: manifest.schemaVersion,
  generatedAt: manifest.generatedAt,
  expiresAt: manifest.expiresAt,
  components: manifest.components,
};
const payload = Buffer.from(JSON.stringify(unsigned));
const signature = manifest.signature;
if (signature?.algorithm !== "ed25519" || typeof signature.value !== "string" || !signature.keyId) {
  throw new Error("manifest signature metadata is incomplete");
}
if (!verify(null, payload, publicKeyFromRaw(publicKeyText), Buffer.from(signature.value, "base64"))) {
  throw new Error("compatibility manifest signature verification failed");
}
console.log(`compatibility manifest signature verified: ${manifestPath} (${signature.keyId})`);
