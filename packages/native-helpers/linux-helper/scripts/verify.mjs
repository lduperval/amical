import { accessSync, constants } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const expectedMethod = process.argv[2];
if (!expectedMethod) {
  console.error("Usage: node scripts/verify.mjs <input-method>");
  process.exit(2);
}

const helperDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const binary = join(helperDir, "bin", "linux-helper");

try {
  accessSync(binary, constants.X_OK);
} catch {
  console.error(`Linux helper is missing or not executable: ${binary}`);
  process.exit(1);
}

const result = spawnSync(binary, ["--print-build-input-method"], {
  encoding: "utf8",
});
if (result.error) throw result.error;
const actualMethod = result.stdout.trim();
if (result.status !== 0 || actualMethod !== expectedMethod) {
  console.error(
    `Wrong linux-helper variant: expected '${expectedMethod}', got '${actualMethod || "no output"}'`,
  );
  process.exit(1);
}

console.log(`Verified linux-helper input method: ${actualMethod}`);
