#!/usr/bin/env node
/*
 * Host side of the DeepCool USB capture harness.
 *
 *   node re/tools/frida-usb-capture.mjs                 # every DeepCool.exe process
 *   node re/tools/frida-usb-capture.mjs --pid 12345     # one process
 *   node re/tools/frida-usb-capture.mjs --exclude 8128  # all but one
 *
 * Writes one JSON object per line to re/captures/capture-<stamp>.jsonl.
 *
 * Why "--all" is the default: Electron apps are multi-process, and Chromium's
 * WebUSB implementation does device I/O outside the main process. Attaching to
 * the first PID alone produced 264 WinUsb_Initialize probes and zero transfers.
 */

import { createWriteStream, mkdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInterface } from 'node:readline';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, '..', '..');
const OUT_DIR = join(REPO, 're', 'captures');
const AGENT = join(HERE, 'frida-usb-capture.js');
const PROCESS_NAME = 'DeepCool.exe';

const require = createRequire(import.meta.url);

function loadFrida() {
  const candidates = [
    'frida',
    join(process.env.APPDATA ?? '', 'npm', 'node_modules', 'rea-agents', 'node_modules', 'frida'),
    join(process.env.APPDATA ?? '', 'npm', 'node_modules', 'frida'),
  ];
  const failures = [];
  for (const id of candidates) {
    try {
      return require(id);
    } catch (err) {
      failures.push(`${id}: ${err.message}`);
    }
  }
  throw new Error('frida not resolvable.\n' + failures.join('\n'));
}

function parseArgs(argv) {
  const opts = { pid: null, exclude: [], duration: 0 };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--pid') opts.pid = Number(argv[++i]);
    else if (a === '--exclude') opts.exclude.push(Number(argv[++i]));
    else if (a === '--seconds') opts.duration = Number(argv[++i]);
    else if (a === '--help' || a === '-h') {
      console.log('usage: frida-usb-capture.mjs [--pid N] [--exclude N] [--seconds N]');
      process.exit(0);
    }
  }
  return opts;
}

function stamp() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

function hex(buf, limit = 56) {
  const n = Math.min(buf.length, limit);
  const parts = [];
  for (let i = 0; i < n; i++) parts.push(buf[i].toString(16).padStart(2, '0'));
  return parts.join(' ') + (buf.length > n ? ` … (${buf.length}B)` : '');
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const frida = loadFrida();
  mkdirSync(OUT_DIR, { recursive: true });

  const device = await frida.getLocalDevice();
  const procs = await device.enumerateProcesses();

  let targets;
  if (opts.pid) {
    targets = procs.filter((p) => p.pid === opts.pid);
  } else {
    targets = procs.filter((p) => p.name.toLowerCase() === PROCESS_NAME.toLowerCase());
  }
  targets = targets.filter((p) => !opts.exclude.includes(p.pid));

  if (targets.length === 0) {
    console.error(`\nNo ${PROCESS_NAME} process found. Start DeepCool first.\n`);
    process.exit(2);
  }

  console.error(`[capture] attaching to ${targets.length} process(es): ` +
    targets.map((t) => `${t.pid}`).join(', '));

  const outPath = join(OUT_DIR, `capture-${stamp()}.jsonl`);
  const out = createWriteStream(outPath, { flags: 'a' });

  const source = await readFile(AGENT, 'utf8');
  const sessions = [];
  const counts = new Map();
  let transfers = 0;

  for (const target of targets) {
    let session;
    try {
      session = await device.attach(target.pid);
    } catch (err) {
      console.error(`[capture] ${target.pid}: attach failed: ${err.message}`);
      continue;
    }
    session.detached.connect((reason) =>
      console.error(`[capture] ${target.pid} detached: ${reason}`));

    let script;
    try {
      script = await session.createScript(source, { name: `deepcool-usb-${target.pid}` });
    } catch (err) {
      console.error(`[capture] ${target.pid}: script failed: ${err.message}`);
      continue;
    }

    script.message.connect((message) => {
      const p = message.payload;
      if (!p || typeof p !== 'object') return;

      if (p.t === 'note') {
        // Init probes are noisy at 6 processes; only surface ones that matter.
        if (!/^WinUsb_Initialize/.test(p.msg)) {
          console.error(`[${target.pid}] ${p.msg}`);
        }
        out.write(JSON.stringify({ t: 'note', pid: target.pid, at: Date.now(), msg: p.msg }) + '\n');
        return;
      }
      if (p.t === 'stats') {
        const s = p.stats;
        if (s.out || s.inn) {
          console.error(`[${target.pid}] out=${s.out} (${s.bytesOut}B) in=${s.inn} (${s.bytesIn}B)`);
        }
        return;
      }
      if (p.t !== 'usb') return;

      transfers++;
      const key = `0x${p.pipe.toString(16).padStart(2, '0')}:${p.dir}`;
      counts.set(key, (counts.get(key) ?? 0) + 1);

      const buf = p.b64 ? Buffer.from(p.b64, 'base64') : Buffer.alloc(0);

      if (p.dir === 'out') {
        console.error(`[${target.pid}] out 0x${p.pipe.toString(16)} ${hex(buf)}`);
      } else if (buf.length >= 2) {
        console.error(`[${target.pid}] in  0x${p.pipe.toString(16)} ${hex(buf)}`);
      }

      out.write(JSON.stringify({
        t: 'usb', pid: target.pid, seq: p.seq, at: p.at,
        dir: p.dir, pipe: p.pipe, len: p.len,
        truncated: p.truncated, hex: buf.toString('hex'),
      }) + '\n');
    });

    try {
      await script.load();
      sessions.push({ pid: target.pid, session, script });
      console.error(`[capture] ${target.pid}: hooked`);
    } catch (err) {
      console.error(`[capture] ${target.pid}: load failed: ${err.message}`);
    }
  }

  if (sessions.length === 0) {
    console.error('[capture] no sessions established');
    process.exit(1);
  }

  console.error(`[capture] writing ${outPath}`);
  console.error('[capture] now drive the vendor app. Enter to stop.');

  await new Promise((resolve) => {
    process.on('SIGINT', resolve);
    if (process.stdin.isTTY) {
      createInterface({ input: process.stdin, output: process.stdout }).once('line', resolve);
    }
    if (opts.duration > 0) setTimeout(resolve, opts.duration * 1000);
  });

  for (const s of sessions) {
    try { await s.script.unload(); } catch {}
    try { await s.session.detach(); } catch {}
  }
  out.end();

  console.error(`\n[capture] ${transfers} transfers -> ${outPath}`);
  for (const [key, n] of [...counts.entries()].sort()) {
    console.error(`  ${key}  ${n}`);
  }
  if (transfers === 0) {
    console.error(
      '\n  Still nothing. The app may do USB in a process we cannot see, or the\n' +
        '  transfer path is not winusb.dll. Next step would be hooking WriteFile/\n' +
        '  ReadFile on the device handle instead.',
    );
  }
}

main().catch((err) => {
  console.error('[capture] failed:', err?.stack ?? err);
  process.exit(1);
});
