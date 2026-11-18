"""Summarise the decoded upload: command order and per-frame chunk counts."""

import json
import pathlib
import sys
from collections import Counter

DEFAULT = pathlib.Path(
    r"C:\Users\nguyen\dev\Deepcool-AIO-LCD\re\captures\upload-decoded.json"
)


def main(argv):
    path = pathlib.Path(argv[0]) if argv else DEFAULT
    doc = json.loads(path.read_text(encoding="utf-8"))
    cmds = doc["commands"]
    frames = doc["frames"]

    counts = Counter(c["cmd"] for c in cmds)
    print("command byte counts")
    for code, n in sorted(counts.items()):
        print(f"  0x{code:02X}  {n}")

    print("\nnon-telemetry command sequence, in capture order")
    for c in cmds:
        if c["cmd"] in (0x01, 0x10):
            continue
        payload = c["payload"].rstrip("0")
        print(f"  0x{c['cmd']:02X}  {payload or '(empty)'}")

    print("\nframes")
    total_chunks = 0
    for f in frames:
        chunks = (f["jpeg_len"] + 63) // 64
        total_chunks += chunks
        print(
            f"  idx={f['frame_index']:<2} frames={f['frame_count']:<2} "
            f"delay={f['delay_ms']:<5} jpeg={f['jpeg_len']:<7} "
            f"chunks={chunks:<5} id={f['content_id']} hdr_sum_ok={f['header_sum_ok']}"
        )
    print(f"\ntotal image chunks across all frames: {total_chunks}")

    # The vendor's content ids are lowercase hex; note whether any uppercase appear.
    ids = [f["content_id"] for f in frames]
    lower = sum(1 for i in ids if i == i.lower())
    print(
        f"content ids: {len(ids)} total, {lower} lowercase, {len(ids) - lower} mixed/upper"
    )

    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
