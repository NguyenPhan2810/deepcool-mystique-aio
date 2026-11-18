"""Pure wire-format layer for the DeepCool MYSTIQUE AIO LCD.

This is a byte-for-byte port of ``Mystique.Core.Device/MystiquePackets.cs`` and
``Mystique.Core.Contracts/TelemetryFrame.cs``. It performs no I/O, imports no USB
library, and never writes to the console, so it imports and unit-tests on any OS.

Everything here was confirmed against a live device on Windows; ``re/CONTEXT.md``
records how. Anything still unknown is marked ``TODO`` and is *not* guessed at.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Iterable, Optional, Tuple

__all__ = [
    "PACKET_LENGTH",
    "MAX_PAYLOAD_LENGTH",
    "READ_BUFFER_LENGTH",
    "INIT_PIPE_OUT",
    "INIT_PIPE_IN",
    "DATA_PIPE_OUT",
    "DATA_PIPE_IN",
    "COMMAND_INFO",
    "COMMAND_STATUS",
    "COMMAND_TELEMETRY",
    "COMMAND_DISPLAY_CONFIG",
    "COMMAND_SETUP_08",
    "COMMAND_MODE_SWITCH",
    "COMMAND_BEGIN_IMAGE_UPLOAD",
    "COMMAND_SERIAL",
    "COMMAND_ERASE_IMAGES",
    "REQUEST_MAGIC",
    "RESPONSE_MAGIC",
    "FRAME_MARKER",
    "ProtocolError",
    "PayloadTooLong",
    "ResponseError",
    "DisplayMode",
    "DeviceStatus",
    "InitCommand",
    "TelemetryFrame",
    "build_packet",
    "build_display_payload",
    "build_init_sequence",
    "validate_response",
    "parse_status",
    "is_in_machine_info_mode",
    "format_response",
    "format_response_ascii",
    "command_name",
]

PACKET_LENGTH = 48
MAX_PAYLOAD_LENGTH = 39

#: Read buffer size used by the Windows reference (``new byte[64]``).
READ_BUFFER_LENGTH = 64

#: Endpoint pair used for initialization and info commands.
INIT_PIPE_OUT = 0x01
INIT_PIPE_IN = 0x81

#: Endpoint pair used for status and telemetry.
DATA_PIPE_OUT = 0x02
DATA_PIPE_IN = 0x82

COMMAND_INFO = 0x12
COMMAND_STATUS = 0x10
COMMAND_TELEMETRY = 0x01
COMMAND_DISPLAY_CONFIG = 0x02

# Image/GIF upload commands, confirmed from the public DeepCool Mystique
# protocol record (see mystique/image_upload.py for provenance).
COMMAND_SETUP_08 = 0x08
COMMAND_MODE_SWITCH = 0x0A
COMMAND_BEGIN_IMAGE_UPLOAD = 0x0F
COMMAND_SERIAL = 0x11
COMMAND_ERASE_IMAGES = 0x14

# ---------------------------------------------------------------------------
# Image / GIF upload is implemented in mystique/image_upload.py.
#
# It is not made of 48-byte command packets alone: each frame is a 64-byte
# "DCLd" header followed by the raw JPEG in ONE bulk write, then a "dcldfinish"
# marker and a 0x08 commit. See that module for the exact sequence and the
# unresolved limitation about returning to Machine Info afterwards.
# ---------------------------------------------------------------------------

REQUEST_MAGIC = b"\xaa\x2e"
RESPONSE_MAGIC = b"\x55\x2e"
FRAME_MARKER = b"HIDC"

#: Response bytes 4..41 inclusive are the per-device identifier block.
#: A live capture showed a 12-char ASCII id at 4..11 followed by a second stable
#: 24-char hex identifier, so the whole body is masked rather than just 4..11.
_DEVICE_ID_OFFSET = 4
_DEVICE_ID_END = 42  # exclusive; "HIDC" starts at 42
_FORMAT_PREFIX_LENGTH = 12


class ProtocolError(Exception):
    """Base class for wire-format failures."""


class PayloadTooLong(ProtocolError, ValueError):
    """A command payload exceeded the 39-byte payload area."""


class ResponseError(ProtocolError, ValueError):
    """A reply did not have the expected shape."""


class DisplayMode(Enum):
    """What the panel is currently showing, as reported by command 0x10."""

    UNKNOWN = "Unknown"
    MACHINE_INFO = "MachineInfo"
    IMAGE_OR_GIF = "ImageOrGif"


@dataclass(frozen=True)
class DeviceStatus:
    """Decoded reply to status command 0x10.

    ``raw_mode`` is kept because only ``0xFF`` and ``0x00`` are understood; any
    other value is reported as ``DisplayMode.UNKNOWN`` without guessing.
    """

    mode: DisplayMode
    raw_mode: int
    rotation: int

    @property
    def rotation_degrees(self) -> int:
        return self.rotation * 90


@dataclass(frozen=True)
class InitCommand:
    """One step of the Machine Info initialization handshake."""

    command: int
    payload: bytes
    settle_ms: int


@dataclass(frozen=True)
class TelemetryFrame:
    """The five values the Machine Info display shows.

    Mirrors the C# record: build frames with :meth:`from_raw` so every field is
    already clamped to what the wire format allows.
    """

    cpu_temperature_c: int
    cpu_usage_percent: int
    memory_load_percent: int
    gpu_temperature_c: int
    cpu_clock_mhz: int

    @classmethod
    def from_raw(
        cls,
        cpu_temperature: float,
        cpu_usage: float,
        memory_load: float,
        gpu_temperature: float,
        cpu_clock: float,
    ) -> "TelemetryFrame":
        """Round and clamp raw sensor values exactly like ``TelemetryFrame.FromRaw``.

        ``round()`` is used deliberately: Python rounds half to even, which is
        what C# ``Math.Round`` does by default. Do not replace it with ``floor``.
        """
        return cls(
            cpu_temperature_c=_clamp_rounded(cpu_temperature, 0, 127),
            cpu_usage_percent=_clamp_rounded(cpu_usage, 0, 100),
            memory_load_percent=_clamp_rounded(memory_load, 0, 100),
            gpu_temperature_c=_clamp_rounded(gpu_temperature, 0, 127),
            cpu_clock_mhz=_clamp_rounded(cpu_clock, 0, 0xFFFF),
        )


def _clamp_rounded(value: float, minimum: int, maximum: int) -> int:
    return min(max(int(round(value)), minimum), maximum)


def build_packet(command: int, payload: Iterable[int] = b"") -> bytes:
    """Build the fixed 48-byte request frame.

    Layout (verified, see re/CONTEXT.md)::

        0  1  2   3..41            42..45   46..47
        AA 2E CMD payload (max 39) HIDC     LE16 sum of bytes 0..45

    The checksum is a plain sum, not a CRC.
    """
    payload = bytes(payload)
    if len(payload) > MAX_PAYLOAD_LENGTH:
        raise PayloadTooLong(
            f"payload must be at most {MAX_PAYLOAD_LENGTH} bytes, got {len(payload)}"
        )

    packet = bytearray(PACKET_LENGTH)
    packet[0] = 0xAA
    packet[1] = 0x2E
    packet[2] = command
    packet[3 : 3 + len(payload)] = payload
    packet[42:46] = FRAME_MARKER

    checksum = sum(packet[:46]) & 0xFFFF
    packet[46] = checksum & 0xFF
    packet[47] = checksum >> 8
    return bytes(packet)


def build_display_payload(frame: TelemetryFrame) -> bytes:
    """Build the 39-byte telemetry payload (command 0x01).

    **Empirical reproduction.** Bytes 0, 3, 6, 18 and 20 are understood as CPU
    temperature, CPU usage, memory load, whole GHz, and the clock-fraction
    marker. The block at 8/9/11..17/21..25 is only partly understood: the
    ``05|06|09`` selector tracks GPU temperature bands, the constants are
    fixed, and the fraction field is duplicated at 21..22 and 24..25 with no
    known reason. This function reproduces ``MystiquePackets.BuildDisplayPayload``
    exactly. Do not "clean it up" without a capture that proves the device
    accepts something else.
    """
    payload = bytearray(MAX_PAYLOAD_LENGTH)
    payload[0] = frame.cpu_temperature_c
    payload[3] = frame.cpu_usage_percent
    payload[6] = frame.memory_load_percent
    payload[8] = (
        0x05
        if frame.gpu_temperature_c < 35
        else 0x06
        if frame.gpu_temperature_c < 38
        else 0x09
    )
    payload[9] = 0x03
    payload[11] = frame.gpu_temperature_c
    payload[12] = 0x05
    payload[14] = 0x06
    payload[15] = 0x0C
    payload[17] = 0x07
    payload[18] = frame.cpu_clock_mhz // 1000
    payload[20] = max(0x0A, frame.cpu_clock_mhz % 1000 // 10)

    decimal_display = (frame.cpu_clock_mhz % 1000 // 10) * 100
    payload[21] = decimal_display & 0xFF
    payload[22] = decimal_display >> 8
    payload[24] = payload[21]
    payload[25] = payload[22]
    return bytes(payload)


def build_init_sequence(rotation: int) -> Tuple[InitCommand, ...]:
    """The 13-command Machine Info handshake, in the verified order.

    The command bytes are deliberately not ascending. Do not sort them: the
    order was confirmed against the device, not inferred.
    """
    if not 0 <= rotation <= 3:
        raise ValueError(
            f"rotation must be 0-3 (0/90/180/270 degrees), got {rotation!r}"
        )

    return (
        InitCommand(0x12, b"", 10),
        InitCommand(0x02, bytes((0x01, 0x00, rotation, 0x01, 0x24)), 10),
        InitCommand(0x03, b"\x01", 10),
        InitCommand(0x04, b"\x05\x00\x00\x01", 10),
        InitCommand(0x07, b"\x00\x02", 10),
        InitCommand(0x08, b"\x00\x04", 10),
        InitCommand(0x05, b"\x01\x01", 10),
        InitCommand(0x0B, b"", 10),
        InitCommand(0x06, b"\x01", 10),
        InitCommand(0x15, b"\x2d\x2d", 10),
        InitCommand(0x16, b"\x2d\x2d", 10),
        InitCommand(0x17, b"\x2d\x2d", 10),
        # The public protocol note spells this as eight bytes ending 0x00; the
        # C# reference uses seven. Both are wire-identical because the payload
        # area is zero-filled, so the trailing constant is written explicitly.
        InitCommand(0x0A, b"\xea\x07\x02\x02\x02\x27\x21\x00", 100),
    )


def validate_response(response: bytes, command: int) -> None:
    """Raise :class:`ResponseError` unless *response* answers *command*.

    A reply whose command byte is 0x00 is accepted: the device uses it as a
    wildcard acknowledgement for some commands.

    TODO: the reply's own ``HIDC`` marker and checksum are not verified because
    the Windows reference does not verify them either. Whether the device fills
    them in on every reply is an open question, not a fact.
    """
    if len(response) < 3:
        raise ResponseError(
            f"short response to command 0x{command:02X}: {len(response)} bytes"
        )
    if response[0:2] != RESPONSE_MAGIC:
        raise ResponseError(f"unexpected response header: {format_response(response)}")
    if response[2] != command and response[2] != 0x00:
        raise ResponseError(
            f"response command mismatch: sent 0x{command:02X}, "
            f"received 0x{response[2]:02X}"
        )


def parse_status(response: bytes) -> DeviceStatus:
    """Decode a 0x10 reply. Byte 4 is rotation, byte 5 is mode."""
    raw_mode = response[5] if len(response) >= 6 else 0
    if len(response) < 6:
        mode = DisplayMode.UNKNOWN
    elif raw_mode == 0xFF:
        mode = DisplayMode.MACHINE_INFO
    elif raw_mode == 0x00:
        mode = DisplayMode.IMAGE_OR_GIF
    else:
        mode = DisplayMode.UNKNOWN

    rotation = response[4] if len(response) >= 5 else 0
    return DeviceStatus(mode=mode, raw_mode=raw_mode, rotation=rotation)


def is_in_machine_info_mode(status_response: bytes) -> bool:
    return parse_status(status_response).mode is DisplayMode.MACHINE_INFO


def format_response(response: bytes, show_device_id: bool = False) -> str:
    """Hex-dump the first 12 reply bytes, masking the identifier body by default.

    Redaction is the default because everything that logs must be safe to share;
    revealing the per-device identifier requires an explicit opt-in.
    """
    length = min(len(response), _FORMAT_PREFIX_LENGTH)
    return " ".join(
        ".." if _should_redact(i, show_device_id) else f"{response[i]:02X}"
        for i in range(length)
    )


def format_response_ascii(response: bytes, show_device_id: bool = False) -> str:
    """Render a reply as printable ASCII, masking the identifier body by default."""
    return "".join(
        "."
        if _should_redact(i, show_device_id) or not 0x20 <= byte <= 0x7E
        else chr(byte)
        for i, byte in enumerate(response)
    )


def _should_redact(index: int, show_device_id: bool) -> bool:
    return not show_device_id and _DEVICE_ID_OFFSET <= index < _DEVICE_ID_END


def command_name(command: int) -> Optional[str]:
    """Human-readable name for the commands this driver knows. Unknown -> None."""
    return {
        COMMAND_INFO: "info",
        COMMAND_STATUS: "status",
        COMMAND_TELEMETRY: "telemetry",
        COMMAND_DISPLAY_CONFIG: "display-config",
        COMMAND_SETUP_08: "setup-08",
        COMMAND_MODE_SWITCH: "mode-switch",
        COMMAND_BEGIN_IMAGE_UPLOAD: "begin-image-upload",
        COMMAND_SERIAL: "serial",
        COMMAND_ERASE_IMAGES: "erase-images",
    }.get(command)
