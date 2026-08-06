import fs from "node:fs";
import path from "node:path";

const [archivePath, internalPath, outputPath] = process.argv.slice(2);

if (!archivePath || !internalPath || !outputPath) {
  console.error("Usage: node asar-extract.mjs <archive.asar> <internal/path> <output>");
  process.exit(2);
}

const archive = fs.readFileSync(archivePath);
const archiveHeaderSize = archive.readUInt32LE(4);
const headerJsonSize = archive.readUInt32LE(12);
const header = JSON.parse(archive.subarray(16, 16 + headerJsonSize).toString("utf8"));

let entry = { files: header.files };
for (const part of internalPath.replaceAll("\\", "/").split("/").filter(Boolean)) {
  entry = entry.files?.[part];
  if (!entry) {
    throw new Error(`Not found in archive: ${internalPath}`);
  }
}

if (entry.files) {
  throw new Error(`Archive entry is a directory: ${internalPath}`);
}
if (entry.unpacked) {
  throw new Error(`Archive entry is unpacked; read it beside the archive instead: ${internalPath}`);
}

const payloadOffset = 8 + archiveHeaderSize;
const start = payloadOffset + Number(entry.offset);
const end = start + Number(entry.size);
if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || end > archive.length) {
  throw new Error(`Invalid archive bounds for ${internalPath}`);
}

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, archive.subarray(start, end));
console.log(`${internalPath} -> ${outputPath} (${entry.size} bytes)`);
