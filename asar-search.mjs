import fs from "node:fs";

const archivePath = process.argv[2];
const terms = process.argv.slice(3);
if (!archivePath || terms.length === 0) {
  console.error("usage: node asar-search.mjs <archive.asar> <term> [term ...]");
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
const dataOffset = 8 + archiveHeaderSize;

const files = [];
function walk(entries, prefix = "") {
  for (const [name, entry] of Object.entries(entries)) {
    const path = prefix ? `${prefix}/${name}` : name;
    if (entry.files) walk(entry.files, path);
    else files.push({ path, ...entry });
  }
}
walk(header.files);

const searchable = /\.(?:cjs|js|json|mjs|node|ts|txt)$/i;
for (const file of files) {
  if (file.unpacked || !file.offset || !searchable.test(file.path) || file.size > 64 * 1024 * 1024) continue;
  const bytes = Buffer.alloc(file.size);
  fs.readSync(fd, bytes, 0, bytes.length, dataOffset + Number(file.offset));
  const content = bytes.toString("utf8");
  const lower = content.toLowerCase();
  for (const term of terms) {
    const needle = term.toLowerCase();
    let from = 0;
    let shown = 0;
    while (shown < 8) {
      const at = lower.indexOf(needle, from);
      if (at < 0) break;
      const start = Math.max(0, at - 350);
      const end = Math.min(content.length, at + term.length + 650);
      const context = content.slice(start, end).replace(/[\r\n\t]+/g, " ");
      console.log(`\nFILE ${file.path}\nTERM ${term}\n${context}`);
      from = at + needle.length;
      shown += 1;
    }
  }
}
fs.closeSync(fd);
