"""Decode a USBPcap recording of the MYSTIQUE LCD.

Rather than parse the full pcap/USBPcap framing (which varies by transfer type),
this scans the raw capture bytes for the two structures that matter and validates
them:

  * 48-byte command packets: AA 2E <cmd> <39 payload> "HIDC" <sum16>
  * DCLd frame headers:      "DCLd" + 60 bytes, then a JPEG of the stated length

Scanning is safe because both structures are self-validating: a false AA 2E
almost never has "HIDC" at offset 42 and a matching checksum.

Usage:
    python decode_usbpcap.py <capture.pcap> [--json out.json]
"""

from __future__ import annotations

import json
import pathlib
import sys

PACKET_LEN = 48
PAYLOAD_LEN = 39
FOOTER = b"HIDC"
DCLD_MAGIC = b"DCLd"
DCLD_LEN = 64


def checksum16(data: bytes) -> int:
    return sum(data) & 0xFFFF


def find_command_packets(blob: bytes):
    """Yield every valid 48-byte request packet found in the capture."""
    out = []
    start = 0
    while True:
        i = blob.find(b"\xaa\x2e", start)
        if i < 0:
            return out
        start = i + 1
        if i + PACKET_LEN > len(blob):
            continue
        pkt = blob[i : i + PACKET_LEN]
        if pkt[42:46] != FOOTER:
            continue
        if checksum16(pkt[:46]) != int.from_bytes(pkt[46:48], "little"):
            continue
        out.append(
            {
                "offset": i,
                "cmd": pkt[2],
                "payload": pkt[3 : 3 + PAYLOAD_LEN].hex(),
            }
        )


def find_dcld_headers(blob: bytes):
    """Yield every plausible DCLd frame header, with the JPEG that follows it."""
    out = []
    start = 0
    while True:
        i = blob.find(DCLD_MAGIC, start)
        if i < 0:
            return out
        start = i + 1
        if i + DCLD_LEN > len(blob):
            continue

        header = blob[i : i + DCLD_LEN]
        jpeg_len = int.from_bytes(header[5:9], "little")
        jpeg_sum = int.from_bytes(header[9:13], "little")

        # Reject headers whose stated length is implausible for a panel image.
        if not (0 < jpeg_len <= 2_000_000):
            continue

        entry = {
            "offset": i,
            "byte4": header[4],
            "jpeg_len": jpeg_len,
            "jpeg_sum_field": jpeg_sum,
            "frame_count": header[13],
            "delay_ms": int.from_bytes(header[15:17], "little"),
            "frame_index": header[17],
            "content_id": header[20:52].decode("ascii", "replace"),
            "header_sum_ok": checksum16(header[:62])
            == int.from_bytes(header[62:64], "little"),
        }

        body = blob[i + DCLD_LEN : i + DCLD_LEN + jpeg_len]
        if len(body) == jpeg_len:
            entry["jpeg_present"] = True
            entry["jpeg_soi"] = body[:2] == b"\xff\xd8"
            entry["jpeg_has_jfif"] = (
                body[2:4] == b"\xff\xe0" and b"JFIF\x00" in body[:20]
            )
            entry["jpeg_sum_ok"] = checksum16(body) == jpeg_sum
            entry["jpeg_first16"] = body[:16].hex()
        else:
            entry["jpeg_present"] = False

        out.append(entry)


def main(argv):
    if not argv:
        print(__doc__)
        return 1

    path = pathlib.Path(argv[0])
    blob = path.read_bytes()
    print(f"capture: {path.name}  {len(blob):,} bytes")

    cmds = find_command_packets(blob)
    print(f"\ncommand packets: {len(cmds)}")
    for c in cmds:
        payload = c["payload"].rstrip("0")
        print(f"  0x{c['cmd']:02X}  payload={payload or '(empty)'}")

    headers = find_dcld_headers(blob)
    print(f"\nDCLd frame headers: {len(headers)}")
    for h in headers:
        print(
            f"  len={h['jpeg_len']:<7} frames={h['frame_count']} idx={h['frame_index']} "
            f"delay={h['delay_ms']} id={h['content_id']} "
            f"hdr_sum_ok={h['header_sum_ok']} "
            f"jpeg={('present' if h['jpeg_present'] else 'absent')}"
            + (
                f" soi={h['jpeg_soi']} jfif={h['jpeg_has_jfif']} sum_ok={h['jpeg_sum_ok']}"
                if h["jpeg_present"]
                else ""
            )
        )

    # The terminator tells us the exact casing this firmware expects.
    for spelling in (b"dcldfinish", b"DCLdfinish", b"DCLDFINISH"):
        n = blob.count(spelling)
        if n:
            print(f"\nterminator {spelling!r}: {n} occurrence(s)")

    if "--json" in argv:
        out = pathlib.Path(argv[argv.index("--json") + 1])
        out.write_text(
            json.dumps({"commands": cmds, "frames": headers}, indent=2),
            encoding="utf-8",
        )
        print(f"\nwrote {out}")

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
