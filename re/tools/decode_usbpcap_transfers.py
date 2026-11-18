"""Parse USBPcap transfer boundaries and report the MYSTIQUE upload structure.

The naive byte scanner cannot tell where one USB transfer ends and the next
begins, so it cannot answer the question that decides our driver's design:
does the vendor send the 64-byte DCLd header and the image in ONE transfer, or
as separate writes?

This walks the pcap and the USBPcap pseudo-header properly, so each payload is
a real URB.

USBPcap pseudo-header (all fields little-endian):
    0  u16 headerLen
    2  u64 irpId
    10 u32 status
    14 u16 function
    16 u8  info          (bit 0 = direction: 1 = IN)
    17 u16 bus
    19 u16 device
    21 u8  endpoint
    22 u8  transfer      (0=iso, 1=interrupt, 2=control, 3=bulk)
    23 u32 dataLength
Then transfer-specific fields, then the data.
"""

from __future__ import annotations

import struct
import sys
from collections import Counter, defaultdict


def iter_pcap(path):
    with open(path, "rb") as fh:
        header = fh.read(24)
        if len(header) < 24:
            return
        magic = header[:4]
        if magic in (b"\xd4\xc3\xb2\xa1", b"\x4d\x3c\xb2\xa1"):
            endian = "<"
        elif magic in (b"\xa1\xb2\xc3\xd4", b"\xa1\xb2\x3c\x4d"):
            endian = ">"
        else:
            raise SystemExit(f"unknown pcap magic {magic!r}")

        while True:
            rec = fh.read(16)
            if len(rec) < 16:
                return
            _, _, incl_len, _ = struct.unpack(endian + "IIII", rec)
            data = fh.read(incl_len)
            if len(data) < incl_len:
                return
            yield data


def parse_usbpcap(data):
    if len(data) < 27:
        return None
    header_len = struct.unpack_from("<H", data, 0)[0]
    info = data[16]
    endpoint = data[21]
    transfer = data[22]
    data_length = struct.unpack_from("<I", data, 23)[0]

    # Control transfers carry a stage byte; isochronous carries three dwords.
    extra = 0
    if transfer == 2:
        extra = 1
    elif transfer == 0:
        extra = 12

    start = header_len + extra
    if start > len(data):
        return None
    payload = data[start : start + data_length]
    return {
        "direction": "in" if info & 1 else "out",
        "endpoint": endpoint,
        "transfer": transfer,
        "len": data_length,
        "payload": payload,
    }


def main(argv):
    if not argv:
        print(__doc__)
        return 1

    path = argv[0]
    counts = Counter()
    dcl_urbs = []
    jpeg_urbs = []
    size_hist = defaultdict(int)

    for frame in iter_pcap(path):
        urb = parse_usbpcap(frame)
        if urb is None:
            continue
        counts[(urb["direction"], urb["endpoint"], urb["transfer"])] += 1

        pl = urb["payload"]
        if pl[:4] == b"DCLd":
            jpeg_len = struct.unpack_from("<I", pl, 5)[0] if len(pl) >= 13 else 0
            dcl_urbs.append(
                {
                    "len": urb["len"],
                    "payload_bytes_captured": len(pl),
                    "jpeg_len_field": jpeg_len,
                    "combined": urb["len"] == 64 + jpeg_len,
                    "endpoint": urb["endpoint"],
                    "direction": urb["direction"],
                }
            )
        elif pl[:2] == b"\xff\xd8":
            jpeg_urbs.append(
                {"len": urb["len"], "captured": len(pl), "endpoint": urb["endpoint"]}
            )
            size_hist[urb["len"] // 1000] += 1

    print("transfer counts by (dir, endpoint, type):")
    for key, n in sorted(counts.items()):
        print(f"  {key[0]:>3}  ep=0x{key[1]:02x}  type={key[2]}  {n}")

    print(f"\nURBs starting with 'DCLd': {len(dcl_urbs)}")
    for u in dcl_urbs[:20]:
        print(
            f"  urb_len={u['len']:<7} captured={u['payload_bytes_captured']:<7} "
            f"jpeg_len_field={u['jpeg_len_field']:<7} combined={u['combined']} "
            f"ep=0x{u['endpoint']:02x} {u['direction']}"
        )

    print(f"\nURBs starting with JPEG SOI: {len(jpeg_urbs)}")
    for u in jpeg_urbs[:10]:
        print(
            f"  urb_len={u['len']:<7} captured={u['captured']:<7} ep=0x{u['endpoint']:02x}"
        )

    if size_hist:
        print("\nJPEG URB size histogram (KB, rounded down):")
        for kb in sorted(size_hist):
            print(f"  {kb} KB  {'#' * min(60, size_hist[kb])}  ({size_hist[kb]})")

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
