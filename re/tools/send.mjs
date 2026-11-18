// Sends one command to a running daemon and prints the reply.
//
// Usage: node send.mjs <cmd> [json-args] [timeout-ms]
// Example: node send.mjs set_mode '{"mode":3}'
//
// Prints the daemon's reply verbatim as one JSON line, including the packet records the
// command produced. Kept deliberately dumb: it is the terminal equivalent of pressing a
// button, not a client library.

import net from "node:net";

const [, , cmd, argsJson = "{}", timeoutArg = "4000"] = process.argv;
const pipe = process.env.MYSTIQUE_PIPE ?? String.raw`\\.\pipe\mystique-control-v1`;
const timeout = Number(timeoutArg);

if (!cmd) {
  console.log("usage: node send.mjs <cmd> [json-args] [timeout-ms]");
  process.exit(2);
}

const socket = net.connect(pipe);
let buffer = "";

socket.setEncoding("utf8");
socket.on("connect", () => {
  socket.write(JSON.stringify({ id: 1, cmd, args: JSON.parse(argsJson) }) + "\n");
});
socket.on("error", (error) => {
  console.log(JSON.stringify({ error: String(error) }));
  process.exit(1);
});
socket.on("data", (chunk) => {
  buffer += chunk;
  let cut;
  while ((cut = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, cut);
    buffer = buffer.slice(cut + 1);
    if (!line.trim()) continue;
    const message = JSON.parse(line);
    if (message.id !== 1) continue;
    console.log(JSON.stringify(message));
    socket.destroy();
    process.exit(message.ok ? 0 : 1);
  }
});

setTimeout(() => {
  console.log(JSON.stringify({ error: `no reply within ${timeout} ms` }));
  process.exit(1);
}, timeout);
