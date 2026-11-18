"""Byte-exact tests for the pure protocol layer.

These mirror ``Mystique.Tests/MystiquePacketsTests.cs`` and add the edge cases the
Linux port introduces. They need no hardware and no pyusb.
"""

import subprocess
import sys
from pathlib import Path

import pytest

from mystique.protocol import (
    MAX_PAYLOAD_LENGTH,
    PACKET_LENGTH,
    DeviceStatus,
    DisplayMode,
    PayloadTooLong,
    ResponseError,
    TelemetryFrame,
    build_display_payload,
    build_init_sequence,
    build_packet,
    format_response,
    format_response_ascii,
    is_in_machine_info_mode,
    parse_status,
    validate_response,
)

PACKAGE_ROOT = Path(__file__).resolve().parents[1]


def test_packet_has_fixed_frame_structure():
    packet = build_packet(0x12, b"")

    assert len(packet) == PACKET_LENGTH
    assert packet[0] == 0xAA
    assert packet[1] == 0x2E
    assert packet[2] == 0x12
    assert packet[42:46] == b"HIDC"


def test_checksum_is_little_endian_sum_of_first_46_bytes():
    # 0xAA + 0x2E + 0x12 + 'H' + 'I' + 'D' + 'C' = 514 = 0x0202
    packet = build_packet(0x12, b"")
    assert packet[46] == 0x02
    assert packet[47] == 0x02


def test_checksum_accounts_for_payload_bytes():
    # 514 - 0x12 + 0x01 + 55 = 552 = 0x0228
    packet = build_packet(0x01, [55])
    assert packet[46] == 0x28
    assert packet[47] == 0x02
    assert sum(packet[:46]) & 0xFFFF == packet[46] | (packet[47] << 8)


def test_payload_lands_at_offset_three():
    packet = build_packet(0x01, [0x11, 0x22, 0x33])
    assert packet[3:6] == b"\x11\x22\x33"
    assert packet[6] == 0x00


def test_payload_longer_than_thirty_nine_bytes_is_rejected():
    with pytest.raises(PayloadTooLong):
        build_packet(0x01, bytes(MAX_PAYLOAD_LENGTH + 1))


def test_payload_accepts_any_bytes_like_iterable():
    assert build_packet(0x01, bytearray([0x11])) == build_packet(0x01, [0x11])
    assert build_packet(0x01, b"\x11") == build_packet(0x01, [0x11])


def test_display_payload_places_every_field_at_its_documented_offset():
    frame = TelemetryFrame(
        cpu_temperature_c=55,
        cpu_usage_percent=12,
        memory_load_percent=40,
        gpu_temperature_c=48,
        cpu_clock_mhz=4600,
    )

    payload = build_display_payload(frame)

    assert len(payload) == MAX_PAYLOAD_LENGTH
    assert payload[0] == 55  # CPU temperature
    assert payload[3] == 12  # CPU usage
    assert payload[6] == 40  # memory load
    assert payload[8] == 0x09  # GPU icon bucket for >= 38 C
    assert payload[9] == 0x03
    assert payload[11] == 48  # GPU temperature
    assert payload[12] == 0x05
    assert payload[14] == 0x06
    assert payload[15] == 0x0C
    assert payload[17] == 0x07
    assert payload[18] == 4  # GHz integer part
    assert payload[20] == 60  # hundredths, floored at 0x0A
    assert payload[21] == 0x70  # 6000 low byte
    assert payload[22] == 0x17  # 6000 high byte
    assert payload[24] == payload[21]
    assert payload[25] == payload[22]


@pytest.mark.parametrize(
    ("gpu_temp", "expected"),
    [(0, 0x05), (34, 0x05), (35, 0x06), (37, 0x06), (38, 0x09), (127, 0x09)],
)
def test_gpu_icon_bucket_follows_temperature_thresholds(gpu_temp, expected):
    frame = TelemetryFrame(50, 10, 40, gpu_temp, 4000)
    assert build_display_payload(frame)[8] == expected


def test_clock_hundredths_never_drop_below_the_floor():
    # 4005 MHz -> 4005 % 1000 / 10 == 0, which must be raised to 0x0A.
    frame = TelemetryFrame(50, 10, 40, 40, 4005)
    assert build_display_payload(frame)[20] == 0x0A


def test_zero_frame_still_carries_the_fixed_constants():
    payload = build_display_payload(TelemetryFrame(0, 0, 0, 0, 0))
    assert payload[8] == 0x05
    assert payload[20] == 0x0A
    assert payload[21] == 0x00
    assert payload[22] == 0x00
    assert payload[24:] == b"\x00" * 15


def test_init_sequence_has_thirteen_commands_in_the_verified_order():
    sequence = build_init_sequence(0)
    assert len(sequence) == 13
    assert [step.command for step in sequence] == [
        0x12,
        0x02,
        0x03,
        0x04,
        0x07,
        0x08,
        0x05,
        0x0B,
        0x06,
        0x15,
        0x16,
        0x17,
        0x0A,
    ]


def test_init_sequence_payloads_are_byte_exact():
    expected = [
        (0x12, b"", 10),
        (0x02, b"\x01\x00\x00\x01\x24", 10),
        (0x03, b"\x01", 10),
        (0x04, b"\x05\x00\x00\x01", 10),
        (0x07, b"\x00\x02", 10),
        (0x08, b"\x00\x04", 10),
        (0x05, b"\x01\x01", 10),
        (0x0B, b"", 10),
        (0x06, b"\x01", 10),
        (0x15, b"\x2d\x2d", 10),
        (0x16, b"\x2d\x2d", 10),
        (0x17, b"\x2d\x2d", 10),
        (0x0A, b"\xea\x07\x02\x02\x02\x27\x21\x00", 100),
    ]
    assert [
        (s.command, s.payload, s.settle_ms) for s in build_init_sequence(0)
    ] == expected


@pytest.mark.parametrize("rotation", [0, 1, 2, 3])
def test_rotation_is_payload_byte_two_of_command_02(rotation):
    command_02 = next(s for s in build_init_sequence(rotation) if s.command == 0x02)
    assert command_02.payload == bytes((0x01, 0x00, rotation, 0x01, 0x24))


@pytest.mark.parametrize("rotation", [-1, 4, 255])
def test_rotation_above_three_is_rejected(rotation):
    with pytest.raises(ValueError):
        build_init_sequence(rotation)


def test_command_zero_a_gets_the_longer_settle_delay():
    sequence = build_init_sequence(0)
    assert next(s for s in sequence if s.command == 0x0A).settle_ms == 100
    assert all(s.settle_ms == 10 for s in sequence if s.command != 0x0A)


def test_every_init_packet_is_a_valid_48_byte_frame():
    for step in build_init_sequence(2):
        packet = build_packet(step.command, step.payload)
        assert len(packet) == PACKET_LENGTH
        assert packet[:3] == bytes((0xAA, 0x2E, step.command))
        assert packet[42:46] == b"HIDC"
        assert sum(packet[:46]) & 0xFFFF == packet[46] | (packet[47] << 8)


def test_valid_response_is_accepted():
    validate_response(b"\x55\x2e\x10\x00", 0x10)
    validate_response(b"\x55\x2e\x00\x00", 0x10)  # 0x00 is a wildcard ack


def test_short_response_is_rejected():
    with pytest.raises(ResponseError):
        validate_response(b"\x55\x2e", 0x10)


def test_wrong_response_header_is_rejected():
    with pytest.raises(ResponseError):
        validate_response(b"\xaa\x2e\x10", 0x10)


def test_mismatched_response_command_is_rejected():
    with pytest.raises(ResponseError):
        validate_response(b"\x55\x2e\x11", 0x10)


def test_validation_error_names_the_mismatch_without_leaking_the_body():
    response = b"\x55\x2e\x11\x00" + b"AABBCCDD" + b"\x00" * 28
    with pytest.raises(ResponseError) as caught:
        validate_response(response, 0x10)
    message = str(caught.value)
    assert "0x11" in message
    assert "AABBCCDD" not in message


def test_status_parses_mode_and_rotation():
    status = parse_status(b"\x55\x2e\x10\x00\x02\xff")

    assert status == DeviceStatus(DisplayMode.MACHINE_INFO, 0xFF, 2)
    assert status.rotation_degrees == 180
    assert is_in_machine_info_mode(b"\x55\x2e\x10\x00\x02\xff")


def test_status_recognises_image_mode():
    assert parse_status(b"\x55\x2e\x10\x00\x00\x00").mode is DisplayMode.IMAGE_OR_GIF


def test_status_with_unknown_mode_is_not_machine_info():
    status = parse_status(b"\x55\x2e\x10\x00\x01\x42")
    assert status.mode is DisplayMode.UNKNOWN
    assert status.raw_mode == 0x42
    assert status.rotation == 1


def test_status_too_short_to_carry_mode_is_unknown():
    status = parse_status(b"\x55\x2e\x10")
    assert status.mode is DisplayMode.UNKNOWN
    assert status.rotation == 0
    assert not is_in_machine_info_mode(b"\x55\x2e\x10")


def test_status_short_of_rotation_still_reports_mode():
    status = parse_status(b"\x55\x2e\x10\x00")
    assert status.mode is DisplayMode.UNKNOWN
    assert status.rotation == 0


def test_format_redacts_the_device_identifier_by_default():
    response = bytes.fromhex("55 2E 12 00 41 41 42 42 43 43 44 44")

    formatted = format_response(response)

    assert formatted == "55 2E 12 00 .. .. .. .. .. .. .. .."
    assert "41 41" not in formatted


def test_format_can_show_the_device_identifier_when_explicitly_requested():
    response = bytes.fromhex("55 2E 12 00 41 41 42 42 43 43 44 44")
    assert format_response(response, show_device_id=True) == (
        "55 2E 12 00 41 41 42 42 43 43 44 44"
    )


def test_format_handles_responses_shorter_than_the_redaction_window():
    assert format_response(bytes.fromhex("55 2E 10 00 07")) == "55 2E 10 00 .."


def test_format_handles_empty_and_oversized_responses():
    assert format_response(b"") == ""
    assert format_response(bytes(64)).startswith("00 00 00 00 ..")


def test_ascii_formatting_also_redacts_the_device_identifier():
    response = bytes.fromhex("55 2E 12 00 41 41 42 42 43 43 44 44")

    text = format_response_ascii(response)

    assert "AABBCCDD" not in text
    assert text[:11] == "U.........."


def test_ascii_redaction_covers_the_whole_body_not_just_the_first_identifier():
    # A live capture carried a second stable 24-char hex identifier after the
    # short one. Masking only bytes 4-11 leaked it.
    response = bytearray(48)
    response[0:3] = b"\x55\x2e\x12"
    response[4:36] = b"AABBCCDD00112233445566778899AABB"
    response[42:46] = b"HIDC"

    text = format_response_ascii(bytes(response))

    assert "AABBCCDD" not in text
    assert "00112233" not in text
    assert "8899AABB" not in text
    assert "HIDC" in text  # the frame marker is structure, not identity


def test_ascii_formatting_can_show_the_device_identifier():
    response = bytes.fromhex("55 2E 12 00 41 41 42 42 43 43 44 44")
    assert "AABBCCDD" in format_response_ascii(response, show_device_id=True)


def test_from_raw_clamps_every_field_to_the_wire_range():
    frame = TelemetryFrame.from_raw(
        cpu_temperature=130.4,
        cpu_usage=101,
        memory_load=-3,
        gpu_temperature=127.6,
        cpu_clock=50000.6,
    )

    assert frame == TelemetryFrame(127, 100, 0, 127, 50001)


def test_from_raw_rounds_half_to_even_like_csharp_math_round():
    frame = TelemetryFrame.from_raw(0.5, 1.5, 2.5, 51.5, 4199.5)
    assert frame.cpu_temperature_c == 0
    assert frame.cpu_usage_percent == 2
    assert frame.memory_load_percent == 2
    assert frame.gpu_temperature_c == 52
    assert frame.cpu_clock_mhz == 4200


def test_protocol_imports_without_pyusb_available():
    """protocol.py must stay importable where pyusb/libusb are absent."""
    script = "\n".join(
        [
            "import sys",
            "class BlockUsb:",
            "    def find_spec(self, name, path=None, target=None):",
            "        if name == 'usb' or name.startswith('usb.'):",
            "            raise ImportError('usb blocked for test')",
            "        return None",
            "sys.meta_path.insert(0, BlockUsb())",
            "import mystique.protocol",
            "import mystique",
            "print('ok')",
        ]
    )
    result = subprocess.run(
        [sys.executable, "-c", script],
        cwd=str(PACKAGE_ROOT),
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "ok"
