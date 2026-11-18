"""List every vendor packet in a USBPcap file, grouped by command byte.

The question it answers: does any command other than 0x01 carry a value that changes over
time? If the panel shows fan and pump RPM that the host never sends, those bytes must be
absent here too — and then the readings belong to the device, not to the driver.

Usage: python scan_commands.py <capture.pcap>
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


def packets(blob: bytes):
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
    seen: dict[int, set[bytes]] = defaultdict(set)
    counts: dict[int, int] = defaultdict(int)
    for blob in records(path):
        for packet in packets(blob):
            command = packet[2]
            counts[command] += 1
            if len(seen[command]) < 6:
                seen[command].add(packet[3:42])

    for command in sorted(counts):
        payloads = sorted(seen[command])
        print(
            f"0x{command:02X}: {counts[command]} packets, {len(payloads)} distinct shown"
        )
        for payload in payloads:
            print("    " + " ".join(f"{byte:02X}" for byte in payload))


if __name__ == "__main__":
    main()
