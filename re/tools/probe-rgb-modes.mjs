// Probe whether 0x02 byte 3 is a mode selector rather than a bit.
//
// The vendor app exposes three lighting choices that the owner says are interlocked
// (motherboard sync, temperature warning, image/GIF edge picking) but has only one field
// for them, `options.rgbOptical`. Our captures only ever exercised values 0 and 1 because
// only the sync toggle was clicked.
//
// This holds 0x02[3] at 2, then 3, then 1, then 0, eight seconds each, so the ring can be
// watched for a distinct behaviour at each value. Nothing else in the payload changes.

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

await new Promise((resolve) => socket.on("connect", resolve));

const state = await call("device_state", {});
const settings = state.data.settings;
const base = [
  settings.idle_screen ? 0x01 : 0x00,
  settings.gyro ? 0x01 : 0x00,
  settings.rotation,
  0x00,
  settings.brightness,
];

console.log(`baseline ${base.slice(0, 4).join(" ")} brightness ${base[4]}`);

async function setByte(value, label) {
  const payload = [...base];
  payload[3] = value;
  const reply = await call("raw_packet", {
    command: 0x02,
    payload,
    pipe: "init",
    dry_run: false,
  });
  const ok = reply.ok ? "ok" : reply.error?.code ?? "failed";
  console.log(`${new Date().toISOString()}  0x02[3] = ${value}   ${label.padEnd(24)} ${ok}`);
}

for (const [value, label] of [
  [2, "watch: temp warning?"],
  [3, "watch: edge picking?"],
  [1, "watch: panel's own"],
  [0, "watch: motherboard"],
]) {
  await setByte(value, label);
  await wait(8000);
}

const original = settings.rgb_sync ? 0x00 : 0x01;
await setByte(original, "restore saved");
socket.destroy();
process.exit(0);
