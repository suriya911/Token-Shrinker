#!/usr/bin/env node
/** Thin, downloader-free launcher for platform-specific native packages. */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { realpathSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const require = createRequire(import.meta.url);
const PLATFORM_PACKAGES: Readonly<Record<string, string>> = {
  "darwin-arm64": "@token-shrinker/cli-darwin-arm64",
  "darwin-x64": "@token-shrinker/cli-darwin-x64",
  "linux-x64": "@token-shrinker/cli-linux-x64-gnu",
  "win32-x64": "@token-shrinker/cli-win32-x64",
};

export function platformPackage(platform = process.platform, arch = process.arch): string {
  const key = platform + "-" + arch;
  const packageName = PLATFORM_PACKAGES[key];
  if (!packageName) {
    throw new Error("Token-Shrinker has no native package for " + key +
      ". Supported targets: " + Object.keys(PLATFORM_PACKAGES).join(", "));
  }
  return packageName;
}

export function resolveBinary(): string {
  if (process.env.TOKEN_SHRINKER_BINARY) return process.env.TOKEN_SHRINKER_BINARY;
  const packageName = platformPackage();
  try {
    const packageJson = require.resolve(packageName + "/package.json");
    const executable = process.platform === "win32" ? "token-shrinker.exe" : "token-shrinker";
    return join(dirname(packageJson), "bin", executable);
  } catch (error) {
    throw new Error("The optional native package " + packageName +
      " is missing. Reinstall @token-shrinker/cli with optional dependencies enabled.",
      { cause: error });
  }
}

export async function verifyBinaryChecksum(binaryPath: string, expectedSha256: string):
  Promise<boolean> {
  const observed = createHash("sha256").update(await readFile(binaryPath)).digest("hex");
  if (!/^[0-9a-f]{64}$/.test(expectedSha256)) return false;
  let difference = observed.length ^ expectedSha256.length;
  for (let index = 0; index < observed.length; index += 1) {
    difference |= observed.charCodeAt(index) ^ expectedSha256.charCodeAt(index);
  }
  return difference === 0;
}

export async function launch(args = process.argv.slice(2)): Promise<number> {
  const child = spawn(resolveBinary(), args, { stdio: "inherit", windowsHide: false });
  const forward = (signal: NodeJS.Signals): void => {
    if (!child.killed) child.kill(signal);
  };
  process.once("SIGINT", forward);
  process.once("SIGTERM", forward);
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      process.removeListener("SIGINT", forward);
      process.removeListener("SIGTERM", forward);
      if (signal) resolve(1);
      else resolve(code ?? 1);
    });
  });
}

const invokedPath = process.argv[1];
if (invokedPath && realpathSync(invokedPath) === realpathSync(fileURLToPath(import.meta.url))) {
  launch().then((code) => { process.exitCode = code; }).catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
