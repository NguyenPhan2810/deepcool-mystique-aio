// Fan/pump provenance probe.
//
// The 0x01 frame carries five sensor values and a handful of constants. Every other byte
// is zero in all 517 captured vendor frames, yet the panel shows CPU-fan and pump RPM.
// This sends a frame with every unused byte set to 0x10 and holds it, so the dials would
// read 4112 if a 16-bit pair feeds them, or 16 if a single byte does.
//
// The stream is stopped for the duration and restarted at the end. Nothing is persisted.

import net from "node:net";

const pipe = process.env.MYSTIQUE_PIPE ?? String.raw`\\.\pipe\mystique-control-v1`;
const socket = net.connect(pipe);
let buffer = "";
let nextId = 0;
const pending = new Map();

socket.setEncoding("utf8");
socket.on("data", (chunk) => {
  buffer += chunk;
  let cut;
  while ((cut = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, cut);
    buffer = buffer.slice(cut + 1);
    if (!line.trim()) continue;
    const message = JSON.parse(line);
    if (message.id && pending.has(message.id)) {
      pending.get(message.id)(message);
      pending.delete(message.id);
    }
  }
});

const call = (cmd, args) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    socket.write(JSON.stringify({ id, cmd, args }) + "\n");
  });

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const KNOWN = new Set([0, 3, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22, 24, 25]);

function frame(fill) {
  const payload = new Array(39).fill(0);
  payload[0] = 40; // CPU °C
  payload[3] = 10; // CPU %
  payload[6] = 30; // MEM %
  payload[8] = 0x09; // GPU variant, >38 °C
  payload[9] = 0x03;
  payload[11] = 40; // GPU °C
  payload[12] = 0x05;
  payload[14] = 0x06;
  payload[15] = 0x0c;
  payload[17] = 0x07;
  payload[18] = 4; // GHz
  payload[20] = 0x0a;
  payload[21] = 0x00;
  payload[22] = 0x00;
  payload[24] = 0x00;
  payload[25] = 0x00;
  for (let index = 0; index < 39; index += 1) {
    if (!KNOWN.has(index)) payload[index] = fill;
  }
  return payload;
}

const send = async (fill, label) => {
  const reply = await call("raw_packet", {
    command: 0x01,
    payload: frame(fill),
    pipe: "init",
    dry_run: false,
  });
  const ok = reply.ok ? "ok" : reply.error?.code ?? "failed";
  console.log(`${new Date().toISOString()}  ${label.padEnd(18)} ${ok}  unknown bytes = 0x${fill.toString(16).padStart(2, "0")}`);
};

await new Promise((resolve) => socket.on("connect", resolve));

const stopped = await call("stop_stream", {});
console.log(`${new Date().toISOString()}  stream stopped: ${stopped.ok}`);

await send(0x00, "baseline");
await wait(3000);

for (let pass = 1; pass <= 3; pass += 1) {
  await send(0x10, `test ${pass} (4112)`);
  await wait(8000);
  await send(0x00, "baseline");
  await wait(2000);
}

const started = await call("start_stream", {});
console.log(`${new Date().toISOString()}  stream restarted: ${started.ok}`);

socket.destroy();
process.exit(0);
