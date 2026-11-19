"""Report every USB endpoint the MYSTIQUE device used in a USBPcap file.

The question: is the LED/temperature-warning control on another endpoint (interrupt,
control) that an AA2E-only scan would miss? The device is class 0xFF with four bulk
endpoints per the Windows code, but that was never confirmed from a descriptor dump, and
a second interface would only show up as traffic on endpoints we never looked at.

Usage: python scan_endpoints.py <capture.pcap>
"""

from __future__ import annotations

import struct
import sys
from collections import Counter


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


def usbpcap_header(blob):
    """USBPcap pseudo-header: headerLen u16, irpId u64, status u32, function u16,
    info u8, bus u16, device u16, endpoint u8, transfer u8, dataLength u32."""
    if len(blob) < 27:
        return None
    header_len = struct.unpack_from("<H", blob, 0)[0]
    bus, device = struct.unpack_from("<HH", blob, 17)
    endpoint = blob[21]
    transfer = blob[22]
    return header_len, bus, device, endpoint, transfer


def main() -> None:
    path = sys.argv[1]
    seen = Counter()
    device_key = None

    # First pass: find the (bus, device) that carries AA 2E packets.
    for blob in records(path):
        if b"\xaa\x2e" not in blob:
            continue
        header = usbpcap_header(blob)
        if header is None:
            continue
        _, bus, device, endpoint, transfer = header
        seen[(bus, device, endpoint, transfer)] += 1
        device_key = (bus, device)

    if device_key is None:
        print("no AA2E packets found")
        return

    print(f"device bus={device_key[0]} address={device_key[1]}")

    # Second pass: every packet for that device, by endpoint and transfer type.
    all_endpoints = Counter()
    for blob in records(path):
        header = usbpcap_header(blob)
        if header is None:
            continue
        _, bus, device, endpoint, transfer = header
        if (bus, device) != device_key:
            continue
        all_endpoints[(endpoint, transfer)] += 1

    for (endpoint, transfer), count in sorted(all_endpoints.items()):
        print(f"  endpoint 0x{endpoint:02X}  transfer {transfer}  {count} packets")


if __name__ == "__main__":
    main()
