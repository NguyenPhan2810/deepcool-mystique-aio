// Drives the running Tauri UI through WebView2's DevTools protocol.
//
// Why this exists: the webview is not a browser tab, so there is no console to read and
// no DOM to inspect by hand. With the app started under
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222
// this connects to the page and evaluates expressions in it, which is the only way to
// tell "the click did nothing" from "the click worked and the panel looks the same".
//
// Usage:
//   node ui-probe.mjs rate          clicks 30 Hz, waits, reports the control state
//   node ui-probe.mjs eval "<js>"   evaluates one expression and prints the value
//
// Read-only except for the `rate` mode, which presses one button.

const [, , mode = "rate", expression = ""] = process.argv;
const endpoint = process.env.MYSTIQUE_CDP ?? "http://127.0.0.1:9222/json";

const targets = await (await fetch(endpoint)).json();
const page = targets.find((target) => target.type === "page");
if (!page) {
  console.log(JSON.stringify({ error: "no page target; is the debug port on?" }));
  process.exit(1);
}

const socket = new WebSocket(page.webSocketDebuggerUrl);
const pending = new Map();
let nextId = 0;

socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    pending.get(message.id)(message);
    pending.delete(message.id);
  }
});

const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });

await new Promise((resolve) => socket.addEventListener("open", resolve));

async function evaluate(source) {
  const reply = await send("Runtime.evaluate", {
    expression: source,
    returnByValue: true,
    awaitPromise: true,
  });
  const result = reply.result?.result;
  if (reply.result?.exceptionDetails) {
    return { exception: reply.result.exceptionDetails.exception?.description ?? "evaluation failed" };
  }
  return result?.value;
}

const snapshot = `
(() => {
  const pressed = {};
  for (const hz of [1, 5, 10, 20, 30]) {
    const node = document.getElementById('stream-rate-' + hz);
    pressed[hz] = node ? node.getAttribute('aria-pressed') : null;
  }
  const text = (id) => {
    const node = document.getElementById(id);
    return node ? (node.textContent || '').trim() : null;
  };
  return {
    pressed,
    streamWord: text('stream-word'),
    frames: text('stream-frames'),
    failed: text('stream-failed'),
    sample: text('stream-sample'),
    transfer: text('stream-transfer'),
    achievedRowHidden: document.getElementById('stream-achieved-row')?.hidden ?? null,
    achieved: text('stream-achieved'),
    streamError: text('stream-error'),
    appError: text('app-error'),
    logCount: text('log-badge')
  };
})()`;

if (mode === "eval") {
  console.log(JSON.stringify(await evaluate(expression), null, 2));
} else if (mode === "shot") {
  const path = expression || "design/ui-console.png";
  const shot = await send("Page.captureScreenshot", { format: "png" });
  const { writeFileSync } = await import("node:fs");
  writeFileSync(path, Buffer.from(shot.result.data, "base64"));
  console.log(JSON.stringify({ wrote: path }));
} else {
  const before = await evaluate(snapshot);
  await evaluate(`document.getElementById('stream-rate-30').click()`);
  await new Promise((resolve) => setTimeout(resolve, 2500));
  const after = await evaluate(snapshot);
  console.log(JSON.stringify({ before, after }, null, 2));
}

socket.close();
