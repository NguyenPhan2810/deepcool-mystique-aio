import fs from "node:fs";

const [archivePath, internalPath, patternText = "."] = process.argv.slice(2);
if (!archivePath || !internalPath) {
  console.error("Usage: node asar-entry-strings.mjs <archive.asar> <internal/path> [regex]");
  process.exit(2);
}

const archive = fs.readFileSync(archivePath);
const archiveHeaderSize = archive.readUInt32LE(4);
const headerJsonSize = archive.readUInt32LE(12);
const header = JSON.parse(archive.subarray(16, 16 + headerJsonSize).toString("utf8"));

let entry = { files: header.files };
for (const part of internalPath.replaceAll("\\", "/").split("/").filter(Boolean)) {
  entry = entry.files?.[part];
  if (!entry) throw new Error(`Not found in archive: ${internalPath}`);
}
if (entry.files || entry.unpacked) throw new Error(`Entry is not a packed file: ${internalPath}`);

const start = 8 + archiveHeaderSize + Number(entry.offset);
const data = archive.subarray(start, start + Number(entry.size));
const wanted = new RegExp(patternText, "i");
const found = new Set();

function collect(text, regex) {
  for (const match of text.matchAll(regex)) {
    const value = match[0].trim();
    if (wanted.test(value)) found.add(value);
  }
}

collect(data.toString("latin1"), /[\x20-\x7e]{4,}/g);
collect(data.toString("utf16le"), /[\x20-\x7e]{4,}/g);

for (const value of [...found].sort((a, b) => a.localeCompare(b))) {
  console.log(value);
}
