#!/usr/bin/env python3
"""Build the conformance fixtures under re/fixtures/ from the live captures.

Inputs are the JSON files written by re/tools/MystiqueCapture. Nothing here
talks to the device. The device identifier is never copied: fixtures carry
either a template placeholder or a response whose body was captured as all
zero. Response checksums for zero-body replies are recomputed and checked
against the capture's own checksumValid flag.

Run from the repository root:  python re/tools/make_fixtures.py
"""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CAP = ROOT / "re" / "tools" / "captures"
OUT = ROOT / "re" / "fixtures"

PIPES = {
    0x12: (0x01, 0x81),
    0x02: (0x01, 0x81),
    0x03: (0x01, 0x81),
    0x04: (0x01, 0x81),
    0x05: (0x01, 0x81),
    0x06: (0x01, 0x81),
    0x07: (0x01, 0x81),
    0x08: (0x01, 0x81),
    0x0B: (0x01, 0x81),
    0x15: (0x01, 0x81),
    0x16: (0x01, 0x81),
    0x17: (0x01, 0x81),
    0x0A: (0x01, 0x81),
    0x10: (0x02, 0x82),
    0x01: (0x02, 0x82),
}

ZERO_BODY = bytes(38)
HIDC = bytes([0x48, 0x49, 0x44, 0x43])


def load(name: str) -> dict:
    return json.loads((CAP / name).read_text(encoding="utf-8"))


def packet(text: str) -> bytes:
    return bytes.fromhex(text.replace(" ", ""))


def hexs(data: bytes | bytearray) -> str:
    return " ".join(f"{b:02X}" for b in data)


def checksum16(data: bytes) -> int:
    return sum(data[:46]) & 0xFFFF


def request_block(text: str, command: int, expect_valid: bool = True) -> dict:
    raw = packet(text)
    assert len(raw) == 48, (len(raw), text)
    assert raw[0] in (0xAA, 0xAB) and raw[1] == 0x2E
    assert raw[2] == command
    want = checksum16(raw)
    got = raw[46] | raw[47] << 8
    if expect_valid:
        assert want == got, (hex(want), hex(got))
    out, inn = PIPES[command]
    block = {
        "hex": hexs(raw),
        "length": 48,
        "command": f"0x{command:02X}",
        "checksumHex": f"0x{got:04X}",
        "checksumAlgorithm": "LE16 sum of bytes 0..45 inclusive (HIDC included)",
    }
    if not expect_valid:
        block["checksumValidInPacket"] = False
        block["correctChecksumHex"] = f"0x{want:04X}"
    return block


def zero_body_response(command: int, echo: int, result: int) -> bytes:
    body = bytes([0x55, 0x2E, echo, result]) + ZERO_BODY + HIDC
    total = checksum16(body)
    return body + bytes([total & 0xFF, total >> 8])


def transport(command: int) -> dict:
    out, inn = PIPES[command]
    return {"out": f"0x{out:02X}", "in": f"0x{inn:02X}", "packetLength": 48}


def write(name: str, payload: dict) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / name
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {path.relative_to(ROOT)}")


def response_from_capture(ex: dict) -> tuple[bytes | None, dict]:
    """Return (full bytes if known/reconstructable, capture facts)."""
    r = ex["response"]
    assert r["received"], "fixture cannot use a timed-out exchange"
    if r.get("fullHex"):
        raw = packet(r["fullHex"])
        assert len(raw) == 48
        assert checksum16(raw) == (raw[46] | raw[47] << 8)
        return raw, r
    return None, r


def main() -> None:
    # ---- 0x12 info -------------------------------------------------------
    info = load("01-info.json")
    ex = info["exchanges"][0]
    req = request_block(ex["request"]["packetHex"], 0x12)
    write(
        "01-info-0x12.json",
        {
            "fixture": "info-0x12",
            "proves": (
                "The 0x12 reply is 48 bytes, echoes 0x12, reports result 0x00, and carries a "
                "32-character uppercase-hex identifier at bytes 4..35 followed by six zero bytes. "
                "The identifier is redacted here; its length and alphabet are pinned, its value is not."
            ),
            "capturedFrom": "re/tools/captures/01-info.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x12),
            "request": req,
            "response": {
                "received": True,
                "length": 48,
                "hexTemplate": (
                    "55 2E 12 00 <identifier: 32 uppercase hex characters, bytes 4..35> "
                    "00 00 00 00 00 00 48 49 44 43 <checksum LE16>"
                ),
                "commandEcho": "0x12",
                "resultCode": "0x00",
                "identifier": {
                    "offset": 4,
                    "length": 32,
                    "alphabet": "0-9 A-F",
                    "redacted": True,
                },
                "zeroPadding": {"offset": 36, "length": 6},
                "tailHex": "48 49 44 43",
                "checksum": (
                    "device-specific: sum of bytes 0..45 includes the identifier, so the "
                    "expected checksum cannot be published. The capture verified it matches "
                    "the device's own reply."
                ),
                "checksumValidAgainstCapture": True,
            },
            "notes": [
                "A 64-byte read buffer was supplied; the device transferred exactly 48 bytes. "
                "No 64-byte response has ever been observed.",
                "Twice-captured (01-info.json and 01b-info.json) with identical prefix, length, "
                "result code and body shape; the identifier was identical, the value is not recorded.",
            ],
        },
    )

    # ---- 0x10 status, machine-info --------------------------------------
    status = load("02-status.json")
    ex = status["exchanges"][0]
    raw, facts = response_from_capture(ex)
    if raw is None:
        # Captured before the tool started emitting full-hex for zero bodies.
        # Reconstruct from the captured rotation/mode bytes and the same rule,
        # then prove it byte-identical to the later 03-init status capture.
        echo = int(facts["commandEcho"], 16)
        result = int(facts["statusByte"], 16)
        rotation = int(facts["rotationByte"], 16)
        mode = int(facts["modeByte"], 16)
        assert facts["bodyNonZeroCount"] == 0
        assert facts["checksumValid"] is True
        raw = bytes([0x55, 0x2E, echo, result, rotation, mode]) + bytes(36) + HIDC
        total = checksum16(raw)
        raw += bytes([total & 0xFF, total >> 8])
        later = load("03-init.json")["exchanges"][13]["response"]["fullHex"]
        assert hexs(raw) == later, (hexs(raw), later)
    write(
        "02-status-machine-info-0x10.json",
        {
            "fixture": "status-machine-info-0x10",
            "proves": (
                "A 0x10 status read in Machine Info mode returns rotation 0 at byte 4 and mode "
                "0xFF at byte 5, with bytes 6..41 all zero. The whole 48-byte reply, checksum "
                "included, is byte-exact and contains no identifier."
            ),
            "capturedFrom": "re/tools/captures/02-status.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x10),
            "request": request_block(ex["request"]["packetHex"], 0x10),
            "response": {
                "received": True,
                "length": 48,
                "hex": hexs(raw),
                "commandEcho": "0x10",
                "resultCode": "0x00",
                "rotationByte": "0x00",
                "modeByte": "0xFF",
                "decoded": "Machine Info, 0 degrees",
                "checksumHex": f"0x{raw[46] | raw[47] << 8:04X}",
                "checksumValid": True,
            },
            "notes": [
                "Captured three times in a row, byte-identical each time.",
                "Image/GIF mode (mode 0x00) was never reached, so no fixture exists for it.",
            ],
        },
    )

    # ---- init sequence ---------------------------------------------------
    init = load("03-init.json")
    steps = []
    for ex in init["exchanges"]:
        cmd = int(ex["request"]["command"], 16)
        req = request_block(ex["request"]["packetHex"], cmd)
        raw, facts = response_from_capture(ex)
        if raw is None and cmd == 0x12:
            steps.append(
                {
                    "name": ex["name"],
                    "request": req,
                    "response": {
                        "received": True,
                        "length": 48,
                        "hexTemplate": (
                            "55 2E 12 00 <identifier: 32 uppercase hex characters> "
                            "00 00 00 00 00 00 48 49 44 43 <checksum LE16>"
                        ),
                        "commandEcho": "0x12",
                        "resultCode": f"0x{int(facts['statusByte'], 16):02X}",
                        "checksumValidAgainstCapture": True,
                        "identifierRedacted": True,
                    },
                }
            )
            continue
        if raw is None:
            echo = int(facts["commandEcho"], 16)
            result = int(facts["statusByte"], 16)
            assert facts["bodyNonZeroCount"] == 0
            assert facts["checksumValid"] is True
            raw = zero_body_response(cmd, echo, result)
            derived = True
        else:
            derived = False
        steps.append(
            {
                "name": ex["name"],
                "request": req,
                "response": {
                    "received": True,
                    "length": 48,
                    "hex": hexs(raw),
                    "commandEcho": f"0x{raw[2]:02X}",
                    "resultCode": f"0x{raw[3]:02X}",
                    "checksumHex": f"0x{raw[46] | raw[47] << 8:04X}",
                    "checksumValid": True,
                    "checksumRecomputed": derived,
                },
            }
        )

    write(
        "03-init-sequence.json",
        {
            "fixture": "init-sequence",
            "proves": (
                "The full thirteen-command handshake, in the non-ascending order the device "
                "requires, with every request and every reply. 0x15, 0x16, 0x17 and 0x0A reply "
                "with command echo 0x00, not their own command byte; all other steps echo. "
                "A 0x10 read after the sequence reports mode 0xFF."
            ),
            "capturedFrom": "re/tools/captures/03-init.json",
            "capturedAtUtc": init["capturedAtUtc"],
            "transport": {"out": "0x01", "in": "0x81", "packetLength": 48},
            "steps": steps,
            "notes": [
                "Settle time used: 10 ms per step, 100 ms after 0x0A, exactly as MystiquePackets.",
                "Response hexes for zero-body replies were reconstructed from the capture's "
                "echo/result bytes and recomputed checksums. The capture itself recorded "
                "checksumValid=true and bodyNonZeroCount=0 for each of them.",
                "0x12 is the only step whose reply carries the identifier; its exact reply is "
                "in 01-info-0x12.json and is not repeated here.",
                "The first status read after init is the last entry and is byte-identical to "
                "02-status-machine-info-0x10.json.",
            ],
        },
    )

    # ---- 0x01 baseline ---------------------------------------------------
    tel = load("04-telemetry-baseline.json")
    ex = tel["exchanges"][0]
    raw, _ = response_from_capture(ex)
    assert raw is not None
    write(
        "04-telemetry-baseline-0x01.json",
        {
            "fixture": "telemetry-baseline-0x01",
            "proves": (
                "A well-formed 39-byte 0x01 telemetry frame is acknowledged with a 48-byte reply, "
                "command echo 0x01, result 0x00, empty body and a valid checksum."
            ),
            "capturedFrom": "re/tools/captures/04-telemetry-baseline.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x01),
            "request": request_block(ex["request"]["packetHex"], 0x01),
            "response": {
                "received": True,
                "length": 48,
                "hex": hexs(raw),
                "commandEcho": "0x01",
                "resultCode": "0x00",
                "checksumHex": f"0x{raw[46] | raw[47] << 8:04X}",
                "checksumValid": True,
            },
            "decodedPayload": {
                "cpuTemperatureC": 36,
                "cpuUsagePercent": 9,
                "memoryLoadPercent": 39,
                "gpuTemperatureC": 48,
                "cpuClockMhz": 4300,
                "note": "Payload built by MystiquePackets.BuildDisplayPayload; this fixture is "
                "the exact frame the Windows service emits for those values.",
            },
        },
    )

    # ---- checksum mismatch ----------------------------------------------
    cs = load("05-checksum.json")
    ex = cs["exchanges"][1]
    raw, _ = response_from_capture(ex)
    assert raw is not None
    write(
        "05-telemetry-checksum-mismatch.json",
        {
            "fixture": "telemetry-checksum-mismatch",
            "proves": (
                "The device validates the 16-bit request checksum. A frame identical to the "
                "baseline except for checksum+1 is still answered, but result byte 3 is 0x02 "
                "instead of 0x00."
            ),
            "capturedFrom": "re/tools/captures/05-checksum.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x01),
            "request": request_block(
                ex["request"]["packetHex"], 0x01, expect_valid=False
            ),
            "response": {
                "received": True,
                "length": 48,
                "hex": hexs(raw),
                "commandEcho": "0x01",
                "resultCode": "0x02",
                "checksumHex": f"0x{raw[46] | raw[47] << 8:04X}",
                "checksumValid": True,
            },
            "notes": [
                "checksum-1 produces the identical 0x02 reply (same capture, exchange 4), so the "
                "check is an equality test, not a range or parity heuristic.",
                "The device always answers with a valid response checksum, including error replies.",
            ],
        },
    )

    # ---- marker mismatch -------------------------------------------------
    ex = cs["exchanges"][7]
    raw, _ = response_from_capture(ex)
    assert raw is not None
    write(
        "06-telemetry-marker-mismatch.json",
        {
            "fixture": "telemetry-marker-mismatch",
            "proves": (
                "The HIDC trailer is validated independently of the checksum. With bytes 42..45 "
                "zeroed and a checksum that is correct for that packet, the device replies with "
                "result 0x01 (not 0x02)."
            ),
            "capturedFrom": "re/tools/captures/05-checksum.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x01),
            "request": request_block(ex["request"]["packetHex"], 0x01),
            "response": {
                "received": True,
                "length": 48,
                "hex": hexs(raw),
                "commandEcho": "0x01",
                "resultCode": "0x01",
                "checksumHex": f"0x{raw[46] | raw[47] << 8:04X}",
                "checksumValid": True,
            },
            "notes": [
                "0x01 and 0x02 are therefore distinct error classes: 0x02 = sum mismatch, "
                "0x01 = trailer marker mismatch (or a superset error that marker failure hits).",
                "Which check runs first when both are broken was NOT established; that experiment "
                "was voided when the device stopped answering (see findings).",
            ],
        },
    )

    # ---- checksum coverage ----------------------------------------------
    ex = cs["exchanges"][5]
    raw, _ = response_from_capture(ex)
    assert raw is not None
    write(
        "07-checksum-covers-hidc.json",
        {
            "fixture": "checksum-covers-hidc",
            "proves": (
                "The device's checksum covers bytes 0..45 including the HIDC trailer. A packet "
                "whose checksum was computed over bytes 0..41 only is rejected with result 0x02, "
                "even though the HIDC bytes themselves are intact."
            ),
            "capturedFrom": "re/tools/captures/05-checksum.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x01),
            "request": request_block(
                ex["request"]["packetHex"], 0x01, expect_valid=False
            ),
            "response": {
                "received": True,
                "length": 48,
                "hex": hexs(raw),
                "commandEcho": "0x01",
                "resultCode": "0x02",
                "checksumHex": f"0x{raw[46] | raw[47] << 8:04X}",
                "checksumValid": True,
            },
            "notes": [
                "This is the only difference from the accepted baseline; payload bytes are "
                "identical, so 0x02 is attributable solely to the checksum coverage.",
            ],
        },
    )

    # ---- 0x10 with bad checksum -----------------------------------------
    ex = cs["exchanges"][9]
    raw, _ = response_from_capture(ex)
    assert raw is not None
    write(
        "08-status-checksum-mismatch.json",
        {
            "fixture": "status-checksum-mismatch",
            "proves": (
                "A bad checksum on 0x10 is reported as result 0x02, but the command is still "
                "executed: the reply still carries rotation 0x00 and mode 0xFF. Error reporting "
                "does not suppress the status payload."
            ),
            "capturedFrom": "re/tools/captures/05-checksum.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x10),
            "request": request_block(
                ex["request"]["packetHex"], 0x10, expect_valid=False
            ),
            "response": {
                "received": True,
                "length": 48,
                "hex": hexs(raw),
                "commandEcho": "0x10",
                "resultCode": "0x02",
                "rotationByte": "0x00",
                "modeByte": "0xFF",
                "checksumHex": f"0x{raw[46] | raw[47] << 8:04X}",
                "checksumValid": True,
            },
            "notes": [
                "Implication for a Linux driver: result 0x02 means 'your sum was wrong', not "
                "'the request was discarded'. Never treat 0x02 as proof the device ignored it.",
                "Whether a 0x01 frame with result 0x02 is applied to the panel is unknown: the "
                "display has no read-back and no camera view was available.",
            ],
        },
    )

    # ---- bad sync --------------------------------------------------------
    bad = load("08-bad-sync.json")
    ex = bad["exchanges"][0]
    req = request_block(ex["request"]["packetHex"], 0x01)  # asserts checksum is valid
    write(
        "09-silent-drop-bad-sync.json",
        {
            "fixture": "silent-drop-bad-sync",
            "proves": (
                "A packet whose first byte is 0xAB instead of 0xAA is not answered at all: no "
                "error reply, no result code, only a 1505 ms transfer timeout."
            ),
            "capturedFrom": "re/tools/captures/08-bad-sync.json",
            "capturedAtUtc": ex["startedAtUtc"],
            "transport": transport(0x01),
            "request": req,
            "response": {
                "received": False,
                "length": 0,
                "timeoutMs": ex["elapsedMs"],
                "win32Error": 121,
            },
            "warning": (
                "THIS PACKET WEDGED THE DEVICE. After it, the bulk OUT endpoint NAKed every "
                "subsequent 48-byte write for about 24 minutes (16:01:10 to 16:25:20 UTC), "
                "including valid frames, before recovering by a means that was not observed. "
                "Host-side recovery (abort/flush/reset pipe, SET_INTERFACE, CLEAR_FEATURE(HALT), "
                "SET_CONFIGURATION, libusb_reset_device) did not restore it. Do not reproduce this "
                "test on a live AIO; the safe conclusion is already established: never emit a "
                "packet that does not start with AA 2E."
            ),
        },
    )

    # ---- no unsolicited traffic -----------------------------------------
    drain = load("07-drain.json")
    write(
        "10-no-unsolicited-traffic.json",
        {
            "fixture": "no-unsolicited-traffic",
            "proves": (
                "The device is strictly request/response. Four reads with no preceding write, on "
                "both IN endpoints, all timed out after 1.5 s."
            ),
            "capturedFrom": "re/tools/captures/07-drain.json",
            "capturedAtUtc": drain["capturedAtUtc"],
            "reads": drain["reads"],
            "notes": [
                "Captured before the wedge experiment, on a healthy device.",
                "A Linux driver does not need a background reader to drain unsolicited events.",
            ],
        },
    )


if __name__ == "__main__":
    main()
