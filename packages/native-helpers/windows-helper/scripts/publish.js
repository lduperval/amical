const { execFileSync } = require("node:child_process");

if (process.platform !== "win32") {
  console.log("Skipping Windows helper build on non-Windows platform");
  process.exit(0);
}

const arch = process.arch;
if (arch !== "x64" && arch !== "arm64") {
  throw new Error(`Unsupported Windows helper architecture: ${arch}`);
}

execFileSync(
  "dotnet",
  [
    "publish",
    "-c",
    "Release",
    "-r",
    `win-${arch}`,
    "--self-contained",
    "true",
    "-p:PublishSingleFile=true",
    "-o",
    "bin",
  ],
  { stdio: "inherit" },
);
