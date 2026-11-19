// Probe whether 0x01 carries a clock for the Time display area.
//
// The panel's Time area counts from power-on (boot 08:42, panel 01:23 at 10:06), and no
// captured vendor frame carried time bytes even with the Time area selected. This writes
// a distinctive 10:30 in each unused byte pair, one pair at a time, so the clock would
// visibly jump if any pair is the clock.

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

function frame() {
  const payload = new Array(39).fill(0);
  payload[0] = 40;
  payload[3] = 10;
  payload[6] = 30;
  payload[8] = 0x09;
  payload[9] = 0x03;
  payload[11] = 40;
  payload[12] = 0x05;
  payload[14] = 0x06;
  payload[15] = 0x0c;
  payload[17] = 0x07;
  payload[18] = Math.floor(settings.brightness > 0 ? 4 : 4);
  payload[20] = 0x0a;
  payload[21] = 0x00;
  payload[22] = 0x00;
  payload[24] = 0x00;
  payload[25] = 0x00;
  return payload;
}

const pairs = [
  [1, 2],
  [4, 5],
  [7, 10],
  [13, 16],
  [19, 23],
  [26, 27],
];

for (const [hourAt, minuteAt] of pairs) {
  const payload = frame();
  payload[hourAt] = 0x10; // 10
  payload[minuteAt] = 0x30; // 30
  const reply = await call("raw_packet", {
    command: 0x01,
    payload,
    pipe: "init",
    dry_run: false,
  });
  console.log(
    `${new Date().toISOString()}  bytes ${hourAt}/${minuteAt} = 10:30  ${reply.ok ? "ok" : reply.error?.code}`,
  );
  await wait(6000);
}

const restored = await call("raw_packet", {
  command: 0x01,
  payload: frame(),
  pipe: "init",
  dry_run: false,
});
console.log(`${new Date().toISOString()}  restored  ${restored.ok ? "ok" : restored.error?.code}`);

socket.destroy();
process.exit(0);
