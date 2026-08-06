import fs from "node:fs";

const archivePath = process.argv[2];
const filter = (process.argv[3] || "").toLowerCase();
if (!archivePath) {
  console.error("usage: node asar-list.mjs <archive.asar> [filter]");
  process.exit(2);
}

const fd = fs.openSync(archivePath, "r");
const sizeHeader = Buffer.alloc(8);
fs.readSync(fd, sizeHeader, 0, sizeHeader.length, 0);
const archiveHeaderSize = sizeHeader.readUInt32LE(4);
const archiveHeader = Buffer.alloc(archiveHeaderSize);
fs.readSync(fd, archiveHeader, 0, archiveHeader.length, 8);
const jsonLength = archiveHeader.readUInt32LE(4);
const header = JSON.parse(archiveHeader.subarray(8, 8 + jsonLength).toString("utf8"));

function walk(entries, prefix = "") {
  for (const [name, entry] of Object.entries(entries)) {
    const path = prefix ? `${prefix}/${name}` : name;
    if (entry.files) walk(entry.files, path);
    else if (!filter || path.toLowerCase().includes(filter)) {
      console.log(`${entry.unpacked ? "U" : "P"}\t${entry.size}\t${path}`);
    }
  }
}

walk(header.files);
fs.closeSync(fd);
