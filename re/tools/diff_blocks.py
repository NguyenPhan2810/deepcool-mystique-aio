"""Diff consecutive config blocks in a USBPcap recording.

The vendor app re-sends its whole configuration block continuously (init commands
followed by a 0x0A heartbeat). That is a gift: when the owner changes a setting,
exactly one field in the block changes, and the diff tells us which command and
which value, with a timestamp.

Output is only the *changes*, so it stays readable over a long capture.
"""

from __future__ import annotations

import struct
import sys

FOOTER = b"HIDC"
PACKET_LEN = 48
PERIODIC = {0x01, 0x10}  # telemetry and status: noise for this purpose


def checksum16(data: bytes) -> int:
    return sum(data) & 0xFFFF


def iter_records(path):
    with open(path, "rb") as fh:
        header = fh.read(24)
        if len(header) < 24:
            return
        magic = header[:4]
        endian = "<"
        if magic in (b"\xa1\xb2\xc3\xd4", b"\xa1\xb2\x3c\x4d"):
            endian = ">"
        while True:
            rec = fh.read(16)
            if len(rec) < 16:
                return
            ts_sec, ts_usec, incl_len, _ = struct.unpack(endian + "IIII", rec)
            data = fh.read(incl_len)
            if len(data) < incl_len:
                return
            yield ts_sec + ts_usec / 1_000_000.0, data


def commands(blob: bytes):
    start = 0
    while True:
        i = blob.find(b"\xaa\x2e", start)
        if i < 0:
            return
        start = i + 1
        if i + PACKET_LEN > len(blob):
            continue
        pkt = blob[i : i + PACKET_LEN]
        if pkt[42:46] != FOOTER:
            continue
        if checksum16(pkt[:46]) != int.from_bytes(pkt[46:48], "little"):
            continue
        yield pkt[2], bytes(pkt[3:42])


def main(argv):
    if not argv:
        print(__doc__)
        return 1
    path = argv[0]

    t0 = None
    block: dict[int, bytes] = {}
    prev: dict[int, bytes] = {}

    for ts, data in iter_records(path):
        if t0 is None:
            t0 = ts
        for cmd, payload in commands(data):
            if cmd in PERIODIC:
                continue

            if cmd == 0x0A:
                # Heartbeat closes a block: compare and emit differences.
                if prev and block and block != prev:
                    rel = ts - t0
                    print(f"\n[{rel:8.2f}s] block changed")
                    for code in sorted(set(block) | set(prev)):
                        old = prev.get(code)
                        new = block.get(code)
                        if old == new:
                            continue
                        print(
                            f"   0x{code:02X}  {old.hex() if old else '(absent)'}"
                            f"  ->  {new.hex() if new else '(absent)'}"
                        )
                prev = dict(block)
                block = {}
            else:
                block[cmd] = payload

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
