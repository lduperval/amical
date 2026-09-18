// Pack the Amical GNOME Shell extension into the zip layout that
// `gnome-extensions install` expects, without depending on `zip` or on GNOME
// tooling being present on the build machine (CI builds the Linux package on
// a plain container). Store-only zip: the extension is two small text files.
import { createHash } from "node:crypto";
import {
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
  copyFileSync,
  chmodSync,
} from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const helperDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const uuid = "amical@amical.ai";
const sourceDir = join(helperDir, "gnome-extension", uuid);
const outDir = process.argv[2] ?? join(helperDir, "bin", "gnome-extension");
const zipPath = join(outDir, `${uuid}.shell-extension.zip`);

const metadata = JSON.parse(
  readFileSync(join(sourceDir, "metadata.json"), "utf8"),
);
if (metadata.uuid !== uuid) {
  console.error(
    `metadata.json uuid '${metadata.uuid}' does not match '${uuid}'`,
  );
  process.exit(1);
}

const crcTable = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};

const listFiles = (dir) =>
  readdirSync(dir)
    .sort()
    .flatMap((name) => {
      const full = join(dir, name);
      return statSync(full).isDirectory() ? listFiles(full) : [full];
    });

// Fixed timestamp keeps the archive reproducible across builds.
const dosTime = 0;
const dosDate = (1 << 5) | 1 | ((2020 - 1980) << 9);
const locals = [];
const centrals = [];
let offset = 0;
for (const file of listFiles(sourceDir)) {
  const name = Buffer.from(relative(sourceDir, file).split("\\").join("/"));
  const data = readFileSync(file);
  const crc = crc32(data);
  const local = Buffer.alloc(30);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4);
  local.writeUInt16LE(0, 6);
  local.writeUInt16LE(0, 8);
  local.writeUInt16LE(dosTime, 10);
  local.writeUInt16LE(dosDate, 12);
  local.writeUInt32LE(crc, 14);
  local.writeUInt32LE(data.length, 18);
  local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(name.length, 26);
  local.writeUInt16LE(0, 28);
  const central = Buffer.alloc(46);
  central.writeUInt32LE(0x02014b50, 0);
  central.writeUInt16LE(20, 4);
  central.writeUInt16LE(20, 6);
  central.writeUInt16LE(0, 8);
  central.writeUInt16LE(0, 10);
  central.writeUInt16LE(dosTime, 12);
  central.writeUInt16LE(dosDate, 14);
  central.writeUInt32LE(crc, 16);
  central.writeUInt32LE(data.length, 20);
  central.writeUInt32LE(data.length, 24);
  central.writeUInt16LE(name.length, 28);
  central.writeUInt16LE(0, 30);
  central.writeUInt16LE(0, 32);
  central.writeUInt16LE(0, 34);
  central.writeUInt16LE(0, 36);
  central.writeUInt32LE(0, 38);
  central.writeUInt32LE(offset, 42);
  locals.push(local, name, data);
  centrals.push(central, name);
  offset += local.length + name.length + data.length;
}
const centralSize = centrals.reduce((sum, buf) => sum + buf.length, 0);
const end = Buffer.alloc(22);
end.writeUInt32LE(0x06054b50, 0);
end.writeUInt16LE(0, 4);
end.writeUInt16LE(0, 6);
end.writeUInt16LE(centrals.length / 2, 8);
end.writeUInt16LE(centrals.length / 2, 10);
end.writeUInt32LE(centralSize, 12);
end.writeUInt32LE(offset, 16);
end.writeUInt16LE(0, 20);

mkdirSync(outDir, { recursive: true });
const zip = Buffer.concat([...locals, ...centrals, end]);
writeFileSync(zipPath, zip);
const installer = join(outDir, "install-amical-gnome-extension.sh");
copyFileSync(
  join(helperDir, "gnome-extension", "install-amical-gnome-extension.sh"),
  installer,
);
chmodSync(installer, 0o755);
copyFileSync(
  join(helperDir, "gnome-extension", "README.md"),
  join(outDir, "README.md"),
);
console.log(
  `Packed ${uuid} v${metadata.version} -> ${zipPath} (${zip.length} bytes, sha256 ${createHash("sha256").update(zip).digest("hex").slice(0, 12)})`,
);
