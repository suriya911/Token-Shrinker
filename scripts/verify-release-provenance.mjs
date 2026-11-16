import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const workflowPath = resolve(process.argv[2] ?? ".github/workflows/release-candidate.yml");
const workflow = await readFile(workflowPath, "utf8");
const fail = (message) => {
  throw new Error(`Release provenance configuration failed: ${message}`);
};

for (const permission of ["id-token: write", "attestations: write", "artifact-metadata: write"]) {
  if (!workflow.includes(permission)) fail(`missing required permission ${permission}`);
}

const attestations = [...workflow.matchAll(/- name: Attest[\s\S]*?(?=\n      - name:|\n  verify:)/gu)]
  .map((match) => match[0]);
const sbomAttestation = attestations.find((block) => block.includes("Attest SBOM"));
const provenanceAttestation = attestations.find((block) => block.includes("Attest release provenance"));
if (!sbomAttestation) fail("SBOM attestation step is missing");
if (!provenanceAttestation) fail("provenance attestation step is missing");
if (!sbomAttestation.includes("sbom-path: release/token-shrinker.cdx.json")) {
  fail("SBOM attestation does not name the generated CycloneDX file");
}
if (!provenanceAttestation.includes("subject-path: release/*")) {
  fail("provenance attestation does not cover the release directory");
}
if (provenanceAttestation.includes("sbom-path:")) {
  fail("provenance attestation must use the default SLSA provenance mode");
}
for (const block of [sbomAttestation, provenanceAttestation]) {
  if (!/uses: actions\/attest@[0-9a-f]{40}/u.test(block)) fail("attestation action is not pinned to a commit SHA");
}

console.log(`Release provenance configuration verified: ${workflowPath}`);
