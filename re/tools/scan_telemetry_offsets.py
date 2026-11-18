"""Report which bytes any 0x01 telemetry frame ever carried in a USBPcap file.

Purpose: the vendor's C# `BuildDisplayPayload` writes only five sensor values and a few
constants. If the panel shows CPU-fan and pump RPM, either those bytes exist in the frame
and are written by a path we have not read, or the panel gets them from the device itself.
This answers the first half: it lists every byte position that was ever non-zero in a
captured 0x01 payload.

Usage: python scan_telemetry_offsets.py <capture.pcap>
"""

from __future__ import annotations

import struct
import sys
from collections import defaultdict

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
            _, _, incl_len, _ = struct.unpack(endian + "IIII", record)
            data = handle.read(incl_len)
            if len(data) < incl_len:
                return
            yield data


def frames(blob: bytes):
    start = 0
    while True:
        index = blob.find(MAGIC, start)
        if index < 0:
            return
        packet = blob[index : index + PACKET_LEN]
        if len(packet) == PACKET_LEN and packet[42:46] == FOOTER and packet[2] == 0x01:
            yield packet[3:42]
        start = index + 1


def main() -> None:
    path = sys.argv[1]
    seen: dict[int, set[int]] = defaultdict(set)
    count = 0
    for blob in records(path):
        for payload in frames(blob):
            count += 1
            for offset, value in enumerate(payload):
                if len(seen[offset]) < 12:
                    seen[offset].add(value)
    print(f"{path}: {count} telemetry frames")

    unknown_nonzero = []
    for offset in range(39):
        values = sorted(seen.get(offset, set()))
        if values and values != [0]:
            print(
                f"  offset {offset:2d}: {', '.join(f'{value:02X}' for value in values)}"
            )
            unknown_nonzero.append(offset)
    if not unknown_nonzero:
        print("  every payload byte was zero in every frame")


if __name__ == "__main__":
    main()
