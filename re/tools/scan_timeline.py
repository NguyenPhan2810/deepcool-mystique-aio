"""Print a timeline of one command's payload changes in a USBPcap file.

The settings captures were decoded by diffing whole config blocks, which is blind to a
command that changes on its own. This walks the packets in time order and prints only
changes, so a toggle can be matched to the second the owner clicked.

Usage: python scan_timeline.py <capture.pcap> <command-hex>
"""

from __future__ import annotations

import struct
import sys

PACKET_LEN = 48
MAGIC = b"\xaa\x2e"
FOOTER = b"HIDC"


def records(path):
    with open(path, "rb") as handle:
        header = handle.read(24)
        if len(header) < 24:
            return
        endian = ">"
        if header[:4] in (b"\xd4\xc3\xb2\xa1", b"\x4d\x3c\xb2\xa1"):
            endian = "<"
        while True:
            record = handle.read(16)
            if len(record) < 16:
                return
            sec, usec, incl_len, _ = struct.unpack(endian + "IIII", record)
            data = handle.read(incl_len)
            if len(data) < incl_len:
                return
            yield sec + usec / 1_000_000.0, data


def packets(blob):
    start = 0
    while True:
        index = blob.find(MAGIC, start)
        if index < 0:
            return
        packet = blob[index : index + PACKET_LEN]
        if len(packet) == PACKET_LEN and packet[42:46] == FOOTER:
            yield packet
        start = index + 1


def main() -> None:
    path = sys.argv[1]
    wanted = int(sys.argv[2], 16)

    first = None
    previous = None
    changes = 0
    total = 0
    for stamp, blob in records(path):
        for packet in packets(blob):
            if packet[2] != wanted:
                continue
            total += 1
            payload = packet[3:42]
            if payload != previous:
                if first is None:
                    first = stamp
                changes += 1
                offset = stamp - first
                print(f"{offset:8.2f}s  {payload[:8].hex(' ')}")
                previous = payload

    print(f"\n{total} packets, {changes} distinct payload transitions")


if __name__ == "__main__":
    main()
