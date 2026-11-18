"""Print the command window around each upload, and the full payload of the
interesting commands, from the decoded USBPcap capture."""

import json
import pathlib
import sys

DEFAULT = pathlib.Path(
    r"C:\Users\nguyen\dev\Deepcool-AIO-LCD\re\captures\upload-decoded.json"
)
WATCH = {0x0F, 0x14, 0x09}
INTERESTING = {0x02, 0x0A, 0x15, 0x16, 0x17}


def printable(payload_hex: str) -> str:
    raw = bytes.fromhex(payload_hex)
    try:
        text = raw.decode("ascii")
        if all(32 <= ord(c) < 127 for c in text.rstrip("\x00")) and text.strip("\x00"):
            return text.rstrip("\x00")
    except UnicodeDecodeError:
        pass
    return ""


def main(argv):
    path = pathlib.Path(argv[0]) if argv else DEFAULT
    doc = json.loads(path.read_text(encoding="utf-8"))
    cmds = doc["commands"]

    print("=== windows around 0x0F / 0x14 / 0x11 / 0x09 ===")
    for i, c in enumerate(cmds):
        if c["cmd"] not in WATCH:
            continue
        lo, hi = max(0, i - 4), min(len(cmds), i + 5)
        print(f"\n-- event 0x{c['cmd']:02X} at index {i}")
        for j in range(lo, hi):
            e = cmds[j]
            tag = ">>" if j == i else "  "
            ascii_hint = printable(e["payload"])
            print(
                f" {tag} 0x{e['cmd']:02X} {e['payload'][:78]}"
                + (f"   |{ascii_hint}|" if ascii_hint else "")
            )

    print("\n=== distinct payloads for 0x02, 0x0A, 0x15, 0x16, 0x17 ===")
    seen = {}
    for c in cmds:
        if c["cmd"] in INTERESTING:
            seen.setdefault((c["cmd"], c["payload"]), 0)
            seen[(c["cmd"], c["payload"])] += 1
    for (code, payload), n in sorted(seen.items()):
        hint = printable(payload)
        print(
            f"  0x{code:02X} x{n:<4} {payload[:78]}" + (f"   |{hint}|" if hint else "")
        )

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
