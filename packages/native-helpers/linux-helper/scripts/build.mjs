import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VALID_METHODS = new Set(["clipboard", "uinput", "xtest", "gnome_ext"]);
const args = process.argv.slice(2);
const debug = args.includes("--debug");
const explicitMethod = args.find((arg) => !arg.startsWith("--"));
const method = explicitMethod ?? process.env.AMICAL_INPUT_METHOD ?? "clipboard";

if (!VALID_METHODS.has(method)) {
  console.error(
    `Unknown input method '${method}'. Expected one of: ${[...VALID_METHODS].join(", ")}`,
  );
  process.exit(2);
}

const helperDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const profile = debug ? "debug" : "release";
const targetDir = join(helperDir, "target", "variants", method);
const cargoArgs = ["build", "--no-default-features", "--target-dir", targetDir];

if (!debug) cargoArgs.push("--release");
if (method !== "clipboard") cargoArgs.push("--features", method);

console.log(`Building linux-helper (${method}, ${profile})`);
const build = spawnSync("cargo", cargoArgs, {
  cwd: helperDir,
  stdio: "inherit",
});
if (build.error) throw build.error;
if (build.status !== 0) process.exit(build.status ?? 1);

const builtBinary = join(targetDir, profile, "linux-helper");
const stagedBinary = join(helperDir, "bin", "linux-helper");
mkdirSync(dirname(stagedBinary), { recursive: true });
copyFileSync(builtBinary, stagedBinary);
chmodSync(stagedBinary, 0o755);

const verify = spawnSync(stagedBinary, ["--print-build-input-method"], {
  encoding: "utf8",
});
if (verify.error) throw verify.error;
const actualMethod = verify.stdout.trim();
if (verify.status !== 0 || actualMethod !== method) {
  console.error(
    `Built helper verification failed: expected '${method}', got '${actualMethod || "no output"}'`,
  );
  process.exit(1);
}

console.log(`Staged verified ${method} helper at ${stagedBinary}`);

// Ship the GNOME Shell extension with every variant: the uinput build uses
// its clipboard when present, and the gnome_ext build needs it outright.
const pack = spawnSync(
  process.execPath,
  [join(helperDir, "scripts", "pack-gnome-extension.mjs")],
  { stdio: "inherit" },
);
if (pack.error) throw pack.error;
if (pack.status !== 0) process.exit(pack.status ?? 1);
