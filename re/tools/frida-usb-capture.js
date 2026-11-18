/*
 * Frida agent: capture DeepCool's USB traffic at the WinUSB boundary.
 *
 * The vendor app statically links libusb but resolves WinUSB dynamically via
 * LoadLibraryA + GetProcAddress, so hooking winusb.dll's exports after the module
 * loads catches every transfer regardless of which code path performs it.
 *
 * Emits one message per event:
 *   { t: 'usb', dir: 'out'|'in', pipe: <int>, len: <int>, truncated: <bool>, b64: <string> }
 *   { t: 'note', msg: <string> }
 *
 * Loaded by frida-usb-capture.mjs. Do not run stand-alone.
 */

'use strict';

const MAX_CAPTURE = 1024 * 1024; // 1 MiB per transfer; image chunks are far smaller.
const MRU_HANDLES = 16;

const stats = { out: 0, inn: 0, bytesOut: 0, bytesIn: 0, init: 0, freed: 0 };
const handles = []; // bounded MRU of WinUsb interface handles, for attribution

let seq = 0;

function b64(ptr, len) {
  if (len <= 0 || ptr.isNull()) return '';
  const n = Math.min(len, MAX_CAPTURE);
  const buf = ptr.readByteArray(n);
  if (buf === null) return '';
  const bytes = new Uint8Array(buf);
  let s = '';
  const CH = 0x8000;
  for (let i = 0; i < bytes.length; i += CH) {
    s += String.fromCharCode.apply(null, bytes.subarray(i, i + CH));
  }
  return { data: btoa(s), truncated: len > n };
}

function note(msg) {
  send({ t: 'note', msg });
}

function remember(handle) {
  const key = handle.toString();
  const at = handles.indexOf(key);
  if (at !== -1) handles.splice(at, 1);
  handles.unshift(key);
  if (handles.length > MRU_HANDLES) handles.pop();
}

function emit(dir, pipe, ptr, len) {
  const payload = b64(ptr, len);
  const rec = {
    t: 'usb',
    seq: seq++,
    at: Date.now(),
    dir,
    pipe: pipe & 0xff,
    len,
    b64: payload ? payload.data : '',
    truncated: payload ? payload.truncated : false,
  };
  if (dir === 'out') {
    stats.out++;
    stats.bytesOut += len;
  } else {
    stats.inn++;
    stats.bytesIn += len;
  }
  send(rec);
}

function reportStats() {
  send({ t: 'stats', stats });
}

function exportOf(mod, name) {
  // Frida 17 removed Module.getExportByName; keep a fallback chain.
  try {
    if (mod !== null && mod.getExportByName) return mod.getExportByName(name);
  } catch (_) {}
  try {
    if (Module.getGlobalExportByName) return Module.getGlobalExportByName(name);
  } catch (_) {}
  try {
    return Module.getExportByName(null, name);
  } catch (_) {}
  return null;
}

function hookWinUsb(mod) {
  const writePipe = exportOf(mod, 'WinUsb_WritePipe');
  const readPipe = exportOf(mod, 'WinUsb_ReadPipe');
  const initialize = exportOf(mod, 'WinUsb_Initialize');
  const free = exportOf(mod, 'WinUsb_Free');

  if (!writePipe && !readPipe && !initialize) {
    note('winusb loaded but no expected exports resolved');
    return false;
  }

  if (initialize) {
    Interceptor.attach(initialize, {
      onEnter(args) {
        this.out = args[1];
      },
      onLeave(retval) {
        stats.init++;
        if (retval.toInt32() !== 0) {
          try {
            remember(this.out.readPointer());
          } catch (_) {}
        }
        note('WinUsb_Initialize ret=' + retval.toInt32());
      },
    });
  }

  if (free) {
    Interceptor.attach(free, {
      onEnter() {
        stats.freed++;
      },
    });
  }

  if (writePipe) {
    Interceptor.attach(writePipe, {
      onEnter(args) {
        const pipe = args[1].toInt32();
        const buf = args[2];
        const len = args[3].toInt32();
        emit('out', pipe, buf, len);
      },
    });
  }

  if (readPipe) {
    Interceptor.attach(readPipe, {
      onEnter(args) {
        this.pipe = args[1].toInt32();
        this.buf = args[2];
        this.want = args[3].toInt32();
        this.got = args[4];
      },
      onLeave(retval) {
        if (retval.toInt32() === 0) return; // failed call: nothing was transferred
        let got = this.want;
        try {
          got = this.got.readU32();
        } catch (_) {}
        emit('in', this.pipe, this.buf, got);
      },
    });
  }

  note(
    'hooked winusb: write=' + !!writePipe +
      ' read=' + !!readPipe +
      ' init=' + !!initialize +
      ' free=' + !!free,
  );
  hookControlTransfers();
  return true;
}

// Handles opened on the AIO's device path. If the app bypasses the WinUSB
// exports and does raw I/O, this is how we catch it.
const deviceHandles = new Set();
const HANDLE_CAP = 64;

function hookCreateFile() {
  let createFile = null;
  try {
    createFile = Module.getGlobalExportByName('CreateFileW');
  } catch (_) {
    try {
      createFile = Module.getExportByName('kernel32.dll', 'CreateFileW');
    } catch (_) {}
  }
  if (!createFile) return;

  Interceptor.attach(createFile, {
    onEnter(args) {
      try {
        this.path = args[0].readUtf16String();
      } catch (_) {
        this.path = null;
      }
    },
    onLeave(retval) {
      if (!this.path || !/vid_3633|mystique/i.test(this.path)) return;
      const invalid = retval.isNull() || retval.toInt32() === -1;
      note('CreateFileW ' + this.path.replace(/#[0-9a-f]{8,}/gi, '#<redacted>') +
        (invalid ? ' (failed)' : ''));
      if (invalid) return;
      if (deviceHandles.size >= HANDLE_CAP) deviceHandles.clear();
      deviceHandles.add(retval.toString());
    },
  });
}

function hookRawIO() {
  const resolve = (name) => {
    try {
      return Module.getGlobalExportByName(name);
    } catch (_) {
      try {
        return Module.getExportByName('kernel32.dll', name);
      } catch (_) {}
    }
    return null;
  };

  const writeFn = resolve('WriteFile');
  if (writeFn) {
    Interceptor.attach(writeFn, {
      onEnter(args) {
        if (!deviceHandles.has(args[0].toString())) return;
        const len = args[2].toInt32();
        if (len > 0 && len < 2 * 1024 * 1024) emit('out', 0xee, args[1], len);
      },
    });
  }

  const readFn = resolve('ReadFile');
  if (readFn) {
    Interceptor.attach(readFn, {
      onEnter(args) {
        this.hit = deviceHandles.has(args[0].toString());
        this.buf = args[1];
        this.want = args[2].toInt32();
        this.got = args[3];
      },
      onLeave(retval) {
        // Win32 BOOL: non-zero is success. A pending overlapped call returns 0
        // with ERROR_IO_PENDING, so a zero here is not proof of no data.
        if (!this.hit || retval.toInt32() === 0) return;
        let n = this.want;
        try { n = this.got.readU32(); } catch (_) {}
        if (n > 0 && n < 2 * 1024 * 1024) emit('in', 0xee, this.buf, n);
      },
    });
  }
}

function hookControlTransfers() {
  if (hookControlTransfers.done) return true;
  let ctl = null;
  try {
    ctl = Module.getExportByName('winusb.dll', 'WinUsb_ControlTransfer');
  } catch (_) {
    try { ctl = Module.getGlobalExportByName('WinUsb_ControlTransfer'); } catch (_) {}
  }
  if (!ctl) return false;

  Interceptor.attach(ctl, {
    onEnter(args) {
      // WinUsb_ControlTransfer(InterfaceHandle, SetupPacket, Buffer, Length,
      //                        *LengthTransferred, Overlapped)
      // SetupPacket is an 8-byte struct passed by value.
      const setup = args[1];
      const buf = args[2];
      const len = args[3].toInt32();
      let data = '';
      if (len > 0 && len <= 4096 && !buf.isNull()) {
        try {
          const b = buf.readByteArray(len);
          if (b !== null) {
            data = Array.from(new Uint8Array(b))
              .map((x) => x.toString(16).padStart(2, '0'))
              .join(' ');
          }
        } catch (_) {}
      }
      // Direction is bit 7 of bmRequestType, the lowest byte of the setup value.
      const dir = (setup.toUInt32() & 0x80) !== 0 ? 'in' : 'out';
      note(`CONTROL ${dir} setup=${setup.toString()} len=${len} data=${data}`);
    },
  });
  hookControlTransfers.done = true;
  note('control-transfer hook installed');
  return true;
}

function hookCatchAll() {
  // Anything that reaches the device must eventually pass through one of these.
  // We do not filter by handle here — handle tracking missed the real path — we
  // filter by content: a protocol packet starts AA 2E, a frame starts "DCLd".
  const looksLikeProtocol = (ptr, len) => {
    if (len < 2 || ptr.isNull()) return false;
    try {
      const b = ptr.readByteArray(Math.min(len, 4));
      if (b === null) return false;
      const u = new Uint8Array(b);
      if (u.length >= 2 && u[0] === 0xaa && u[1] === 0x2e) return true;
      if (u.length >= 4 && u[0] === 0x44 && u[1] === 0x43 && u[2] === 0x4c && u[3] === 0x64) return true;
    } catch (_) {}
    return false;
  };

  const resolve = (mod, name) => {
    try { return Module.getExportByName(mod, name); } catch (_) {}
    try { return Module.getGlobalExportByName(name); } catch (_) {}
    return null;
  };

  const writeFile = resolve('kernel32.dll', 'WriteFile');
  if (writeFile) {
    Interceptor.attach(writeFile, {
      onEnter(args) {
        const len = args[2].toInt32();
        if (looksLikeProtocol(args[1], len)) emit('out', 0xee, args[1], len);
      },
    });
  }

  const devIoctl = resolve('kernel32.dll', 'DeviceIoControl');
  if (devIoctl) {
    Interceptor.attach(devIoctl, {
      onEnter(args) {
        const code = args[1].toInt32() >>> 0;
        const len = args[3].toInt32();
        if (looksLikeProtocol(args[2], len)) {
          note('DeviceIoControl code=0x' + code.toString(16));
          emit('out', 0xee, args[2], len);
        }
      },
    });
  }

  const ntWrite = resolve('ntdll.dll', 'NtWriteFile');
  if (ntWrite) {
    Interceptor.attach(ntWrite, {
      onEnter(args) {
        // NtWriteFile(Handle, Event, ApcRoutine, ApcContext, IoStatusBlock,
        //             Buffer, Length, ByteOffset, Key)
        const len = args[6].toInt32();
        if (looksLikeProtocol(args[5], len)) emit('out', 0xee, args[5], len);
      },
    });
  }

  const ntIoctl = resolve('ntdll.dll', 'NtDeviceIoControlFile');
  if (ntIoctl) {
    Interceptor.attach(ntIoctl, {
      onEnter(args) {
        // (Handle, Event, ApcRoutine, ApcContext, IoStatusBlock,
        //  IoControlCode, InputBuffer, InputBufferLength, OutputBuffer, OutputBufferLength)
        const code = args[5].toInt32() >>> 0;
        const len = args[7].toInt32();
        if (looksLikeProtocol(args[6], len)) {
          note('NtDeviceIoControlFile code=0x' + code.toString(16));
          emit('out', 0xee, args[6], len);
        }
      },
    });
  }

  note('catch-all hooks installed (WriteFile/DeviceIoControl/NtWriteFile/NtDeviceIoControlFile)');
}

let armed = false;

function arm() {
  if (armed) return;
  armed = true;
  hookCreateFile();
  hookCatchAll();

  const tryHook = (mod) => {
    if (mod && hookWinUsb(mod)) return true;
    return false;
  };

  try {
    if (Process.attachModuleObserver) {
      Process.attachModuleObserver({
        onAdded(mod) {
          if (mod && /^winusb\.dll$/i.test(mod.name)) tryHook(mod);
        },
      });
    }
  } catch (_) {
    note('attachModuleObserver unavailable; falling back to polling');
  }

  // Handle both "already loaded" and "loads later" without relying on one API.
  const poll = () => {
    if (armed === 'done') return;
    let candidate = null;
    try {
      candidate = Process.getModuleByName('winusb.dll');
    } catch (_) {}
    if (candidate && tryHook(candidate)) {
      armed = 'done';
      return;
    }
    setTimeout(poll, 250);
  };
  poll();

  setTimeout(reportStats, 5000);
  setInterval(reportStats, 30000);
}

rpc.exports = {
  stats() {
    return stats;
  },
  handles() {
    return handles.slice();
  },
};

arm();
