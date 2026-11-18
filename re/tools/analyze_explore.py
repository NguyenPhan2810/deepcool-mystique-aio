"""Summarise protocol-explorer reports: which command bytes the device recognises.

A recognised command echoes its own byte in the reply. Everything else gets the
same neutral `55 2E 00` ACK, which is an acknowledgement of *receipt*, not of
understanding.
"""

import json
import pathlib
import sys

REPORTS = pathlib.Path(__file__).resolve().parents[1] / "reports"


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def main(argv):
    files = sorted(REPORTS.glob("explore-*.json"))
    if argv:
        files = [REPORTS / a for a in argv]
    if not files:
        print("no reports found in", REPORTS)
        return 1

    for path in files:
        doc = load(path)
        entries = doc["entries"]
        recognised = [e for e in entries if e["echo"] != 0]
        print(
            f"\n{path.name}  phase={doc.get('phase')}  mode={doc.get('mode')}  "
            f"entries={len(entries)}"
        )
        print(f"  recognised (echoed): {len(recognised)}")
        for e in recognised:
            print(
                f"    0x{e['cmd']:02X}  pipe={e['pipe']:<5} result={e['result_code']} "
                f"len={e['reply_len']}  {e['reply_hex_redacted'][:58]}"
            )
        silent = len(entries) - len(recognised)
        print(f"  neutral-ACK only:   {silent}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
