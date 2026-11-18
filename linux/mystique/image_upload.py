"""Image/GIF upload: pure frame builders plus Pillow encoding.

Protocol source
---------------
The image path is not in the Windows C# reference. It is implemented from the
public, hardware-confirmed record in ``dinoyoshi/serial-lcd-panel-client``
(MIT), which ported the machine-info protocol from ``mymymy1303/qt-deepcool``
(MIT) and derived the upload path from ``usbmon`` captures of genuine
DeepCreative traffic:

* https://github.com/dinoyoshi/serial-lcd-panel-client
* ``docs/PROTOCOL_DEEPCOOL_MYSTIQUE.md`` in that repository
* ``src/devices/deepcoolMystique/{protocol,device,imageUpload}.ts``

Key facts:
* ``0x14`` erase, ``0x0F`` begin, ``0x11`` serial, and the same setup steps as
  the Machine Info init between them
* one ``DCLd`` 64-byte header immediately followed by the JPEG, sent as **one**
  bulk write (two writes make libusb insert a ZLP at the 64-byte boundary and
  the device silently ignores the upload)
* literal ``dcldfinish`` marker, ``0x08`` commit with ``[0x01, 0x01]``
* the full image-mode handshake again after the data

This module is pure: no USB imports, no sysfs. Pillow is imported only inside
:func:`encode_frames`, so the package and its tests work without Pillow.
"""

from __future__ import annotations

import hashlib
import io
import re
from dataclasses import dataclass
from typing import List, Optional, Sequence, Tuple

from .protocol import (
    COMMAND_BEGIN_IMAGE_UPLOAD,
    COMMAND_ERASE_IMAGES,
    COMMAND_INFO,
    COMMAND_MODE_SWITCH,
    COMMAND_SERIAL,
    COMMAND_SETUP_08,
    COMMAND_STATUS,
    COMMAND_TELEMETRY,
    INIT_PIPE_IN,
    INIT_PIPE_OUT,
    build_init_sequence,
    build_packet,
)

__all__ = [
    "UPLOAD_WIDTH",
    "UPLOAD_HEIGHT",
    "IMAGE_MODE_SWITCH_PAYLOAD",
    "IMAGE_MODE_HEARTBEAT_PAYLOAD",
    "FRAME_HEADER_SIZE",
    "FRAME_MAGIC",
    "UPLOAD_FINISH_MARKER",
    "JFIF_APP0_SEGMENT",
    "ImageUploadError",
    "ImageSupportMissing",
    "Operation",
    "EncodedAnimation",
    "build_frame_header",
    "build_finish_marker",
    "build_image_mode_handshake",
    "build_upload_sequence",
    "content_id",
    "ensure_jfif",
    "encode_frames",
]

#: Hardware-confirmed upload size. The genuine captured JPEGs were 480x640
#: portrait; this is used rather than guessing the panel's native resolution.
UPLOAD_WIDTH = 480
UPLOAD_HEIGHT = 640

#: ModeSwitch payload for Image/GIF mode. The trailing 0x00 is spelled out by
#: the public note; the payload area is zero-filled, so it is wire-identical to
#: a 7-byte payload. Bytes 5-6 were observed as 0x3a/0x09 and 0x3a/0x1d; the
#: first captured instance is used, as in the reference implementation.
IMAGE_MODE_SWITCH_PAYLOAD = bytes.fromhex("EA 07 08 09 11 3A 09 00")

#: Unexplained display-update payload sent on endpoint 1 during the image-mode
#: handshake. Reproduced verbatim from the capture; it looks like a heartbeat.
IMAGE_MODE_HEARTBEAT_PAYLOAD = bytes.fromhex("00 00 00 00 00 00 54 00 1E")

COMMIT_IMAGE_PAYLOAD = b"\x01\x01"
ERASE_PAYLOAD = b""
BEGIN_UPLOAD_PAYLOAD = b""

FRAME_HEADER_SIZE = 64
FRAME_MAGIC = b"DCLd"
UPLOAD_FINISH_MARKER = b"dcldfinish"

MIN_FRAME_DELAY_MS = 20
DEFAULT_FRAME_DELAY_MS = 100
MAX_FRAME_DELAY_MS = 0xFFFF
MAX_FRAMES = 0xFF

JPEG_QUALITY = 85
#: Background used for the letterbox in ``contain`` mode, from the reference
#: implementation (#0b0f14).
BACKGROUND_RGB = (0x0B, 0x0F, 0x14)

#: Standard JFIF APP0: marker + 16-byte payload, no density units, no thumbnail.
JFIF_APP0_SEGMENT = bytes.fromhex("FFE000104A46494600010100000100010000")

_CONTENT_ID_PATTERN = re.compile(r"^[0-9a-fA-F]{32}$")


class ImageUploadError(ValueError):
    """The image cannot be encoded or the upload plan is invalid."""


class ImageSupportMissing(ImageUploadError):
    """Pillow is not installed, so image/GIF encoding is unavailable."""


@dataclass(frozen=True)
class Operation:
    """One transfer in an upload plan.

    ``reply_endpoint`` is None for raw frame data and the finish marker, which
    the device does not answer. Command operations read one 64-byte reply and
    accept a missing reply (a timeout) because the public implementation does;
    a stall, removal, or short read is still a fault. Image replies are drained
    but not parsed: the reference ignores their contents, and this flow is
    synchronized by command order, not reply matching.
    """

    endpoint: int
    data: bytes
    reply_endpoint: Optional[int] = None
    reply_optional: bool = False
    validate_reply: bool = True
    settle_ms: int = 0
    description: str = ""

    @property
    def command(self) -> Optional[int]:
        """Command byte when this is a framed command packet, else None."""
        if len(self.data) >= 3 and self.data[:2] == b"\xaa\x2e":
            return self.data[2]
        return None


@dataclass(frozen=True)
class EncodedAnimation:
    """JPEG frames and per-frame delays, ready for :func:`build_upload_sequence`."""

    frames: Tuple[bytes, ...]
    delays_ms: Tuple[int, ...]

    def __len__(self) -> int:
        return len(self.frames)


def build_frame_header(
    frame_index: int,
    total_frames: int,
    jpeg: bytes,
    delay_ms: int,
    content_id_value: str,
) -> bytes:
    """Build the 64-byte ``DCLd`` header for one frame.

    Layout, confirmed against captured genuine headers::

        0..3    "DCLd"
        4       0x02
        5..8    JPEG length, u32 LE
        9..12   sum-of-JPEG-bytes mod 65536, u32 LE
        13      total frame count, u8
        15..16  per-frame delay in ms, u16 LE
        17      0-based frame index, u8
        20..51  32-char ASCII hex content id, constant for the upload
        62..63  sum of header bytes 0..61, u16 LE
    """
    if not 1 <= total_frames <= MAX_FRAMES:
        raise ImageUploadError(
            f"total_frames must be 1-{MAX_FRAMES}, got {total_frames}"
        )
    if not 0 <= frame_index < total_frames:
        raise ImageUploadError(
            f"frame_index must be 0-{total_frames - 1}, got {frame_index}"
        )
    if not jpeg:
        raise ImageUploadError("a JPEG frame cannot be empty")
    if len(jpeg) > 0xFFFFFFFF:
        raise ImageUploadError(f"JPEG frame is too large: {len(jpeg)} bytes")
    if not 0 <= delay_ms <= MAX_FRAME_DELAY_MS:
        raise ImageUploadError(
            f"delay_ms must be 0-{MAX_FRAME_DELAY_MS}, got {delay_ms}"
        )
    if not _CONTENT_ID_PATTERN.match(content_id_value):
        raise ImageUploadError(
            "content id must be exactly 32 ASCII hex characters, "
            f"got {content_id_value!r}"
        )

    header = bytearray(FRAME_HEADER_SIZE)
    header[0:4] = FRAME_MAGIC
    header[4] = 0x02
    header[5:9] = len(jpeg).to_bytes(4, "little")
    header[9:13] = (sum(jpeg) & 0xFFFF).to_bytes(4, "little")
    header[13] = total_frames
    header[15:17] = delay_ms.to_bytes(2, "little")
    header[17] = frame_index
    header[20:52] = content_id_value.encode("ascii")

    header[62:64] = (sum(header[:62]) & 0xFFFF).to_bytes(2, "little")
    return bytes(header)


def build_finish_marker() -> bytes:
    """The 64-byte zero-padded ``dcldfinish`` marker sent after the last frame."""
    return UPLOAD_FINISH_MARKER.ljust(FRAME_HEADER_SIZE, b"\x00")


def content_id(frames: Sequence[bytes]) -> str:
    """MD5 hex of the concatenated JPEG bytes, as the public implementation does.

    The docs note the firmware appears not to validate the derivation, but the
    field must be 32 ASCII hex characters and constant across the upload. MD5
    is what the hardware-confirmed implementation uses.
    """
    digest = hashlib.md5(b"".join(frames), usedforsecurity=False)
    return digest.hexdigest()


def _command_operation(
    command: int,
    payload: bytes = b"",
    *,
    settle_ms: int = 0,
    description: str = "",
) -> Operation:
    return Operation(
        endpoint=INIT_PIPE_OUT,
        data=build_packet(command, payload),
        reply_endpoint=INIT_PIPE_IN,
        reply_optional=True,
        validate_reply=False,
        settle_ms=settle_ms,
        description=description,
    )


def build_image_mode_handshake(rotation: int = 0, label: str = "") -> List[Operation]:
    """The ModeSwitch+setup handshake DeepCreative sends before and after data.

    Order is copied from the capture: image-mode switch, status request,
    unexplained display heartbeat, device info, serial request, then the shared
    setup steps (Config through the display-label commands). The front and back
    halves of the init sequence are excluded because they are sent explicitly
    or replaced by the image-mode switch.
    """
    operations = [
        _command_operation(
            COMMAND_MODE_SWITCH, IMAGE_MODE_SWITCH_PAYLOAD, description=label
        ),
        _command_operation(COMMAND_STATUS),
        _command_operation(COMMAND_TELEMETRY, IMAGE_MODE_HEARTBEAT_PAYLOAD),
        _command_operation(COMMAND_INFO),
        _command_operation(COMMAND_SERIAL),  # serial request, image handshake only
    ]

    # INIT_SEQUENCE.slice(1, -1): Config through DisplayLabel17. build_init_sequence
    # ends with the Machine Info mode switch, which is not part of this handshake.
    for step in build_init_sequence(rotation)[1:-1]:
        operations.append(
            _command_operation(step.command, step.payload, settle_ms=step.settle_ms)
        )
    return operations


def build_upload_sequence(
    frames: Sequence[bytes],
    delays_ms: Sequence[int],
    *,
    rotation: int = 0,
    content_id_value: Optional[str] = None,
) -> List[Operation]:
    """Build the complete image upload as an ordered list of transfers.

    The sequence is: erase stored images, image-mode handshake, begin upload,
    one combined header+JPEG write per frame, finish marker, commit, then the
    image-mode handshake again. Erase is unconditional: without it a second
    upload silently keeps the previous content.
    """
    if not frames:
        raise ImageUploadError("an upload needs at least one frame")
    if len(frames) > MAX_FRAMES:
        raise ImageUploadError(f"too many frames: {len(frames)} (maximum {MAX_FRAMES})")
    if len(delays_ms) != len(frames):
        raise ImageUploadError(
            f"delays_ms has {len(delays_ms)} entries for {len(frames)} frames"
        )
    for index, frame in enumerate(frames):
        if not frame:
            raise ImageUploadError(f"frame {index} is empty")
    for index, delay in enumerate(delays_ms):
        if not 0 <= delay <= MAX_FRAME_DELAY_MS:
            raise ImageUploadError(
                f"frame {index} delay must be 0-{MAX_FRAME_DELAY_MS} ms, got {delay}"
            )

    upload_id = content_id(frames) if content_id_value is None else content_id_value
    if not _CONTENT_ID_PATTERN.match(upload_id):
        raise ImageUploadError(
            f"content id must be exactly 32 ASCII hex characters, got {upload_id!r}"
        )

    operations: List[Operation] = [
        _command_operation(
            COMMAND_ERASE_IMAGES, ERASE_PAYLOAD, description="erase stored images"
        )
    ]
    operations.extend(
        build_image_mode_handshake(rotation, label="image-mode handshake")
    )
    operations.append(
        _command_operation(
            COMMAND_BEGIN_IMAGE_UPLOAD,
            BEGIN_UPLOAD_PAYLOAD,
            description="begin image upload",
        )
    )

    total = len(frames)
    for index, (frame, delay) in enumerate(zip(frames, delays_ms)):
        header = build_frame_header(index, total, frame, delay, upload_id)
        operations.append(
            Operation(
                endpoint=INIT_PIPE_OUT,
                # One buffer, one write. Never split these.
                data=header + frame,
                reply_endpoint=None,
                description=f"frame {index + 1}/{total} ({len(frame)} bytes)",
            )
        )

    operations.append(
        Operation(
            endpoint=INIT_PIPE_OUT,
            data=build_finish_marker(),
            reply_endpoint=None,
            description="finish marker",
        )
    )
    operations.append(
        _command_operation(
            COMMAND_SETUP_08, COMMIT_IMAGE_PAYLOAD, description="commit image"
        )
    )
    operations.extend(
        build_image_mode_handshake(rotation, label="image-mode handshake (replay)")
    )
    return operations


def ensure_jfif(jpeg: bytes) -> bytes:
    """Insert the standard JFIF APP0 segment right after SOI when absent.

    Pillow emits it by default, but the panel's decoder silently rejects JPEGs
    without it, so this is asserted rather than assumed.
    """
    if not jpeg.startswith(b"\xff\xd8"):
        raise ImageUploadError("frame is not a JPEG (missing SOI marker)")
    if jpeg[2:4] == b"\xff\xe0":
        return jpeg
    return jpeg[:2] + JFIF_APP0_SEGMENT + jpeg[2:]


def encode_frames(
    source,
    fit: str = "contain",
) -> EncodedAnimation:
    """Decode an image/GIF and encode 480x640 JPEG frames for the panel.

    ``fit`` is ``contain`` (letterbox on #0b0f14) or ``cover`` (centre crop),
    matching the public implementation's sharp resize modes. Pillow is imported
    here so telemetry-only installs never need it.
    """
    if fit not in ("contain", "cover"):
        raise ImageUploadError(f"fit must be 'contain' or 'cover', got {fit!r}")

    try:
        from PIL import Image, ImageOps
    except ImportError as exc:  # pragma: no cover - depends on the machine
        raise ImageSupportMissing(
            "image/GIF support needs Pillow: pip install 'deepcool-mystique[image]'"
        ) from exc

    try:
        image = Image.open(source)
    except OSError as exc:
        raise ImageUploadError(f"cannot decode image {source!r}: {exc}") from exc

    frames: List[bytes] = []
    delays: List[int] = []
    try:
        frame_count = getattr(image, "n_frames", 1)
        if frame_count > MAX_FRAMES:
            raise ImageUploadError(
                f"too many frames: {frame_count} (maximum {MAX_FRAMES})"
            )

        for index in range(frame_count):
            image.seek(index)
            duration = int(image.info.get("duration", 0) or 0)
            delays.append(
                max(
                    MIN_FRAME_DELAY_MS,
                    min(MAX_FRAME_DELAY_MS, duration or DEFAULT_FRAME_DELAY_MS),
                )
            )

            frame = ImageOps.exif_transpose(image.convert("RGB"))
            if fit == "contain":
                frame = ImageOps.pad(
                    frame,
                    (UPLOAD_WIDTH, UPLOAD_HEIGHT),
                    method=Image.Resampling.LANCZOS,
                    color=BACKGROUND_RGB,
                    centering=(0.5, 0.5),
                )
            else:
                frame = ImageOps.fit(
                    frame,
                    (UPLOAD_WIDTH, UPLOAD_HEIGHT),
                    method=Image.Resampling.LANCZOS,
                    centering=(0.5, 0.5),
                )

            buffer = io.BytesIO()
            # optimize=False is load-bearing: the panel rejects content-optimized
            # Huffman tables, and the standard tables are what genuine uploads
            # use. Pillow uses libjpeg's standard tables when optimization is off.
            frame.save(
                buffer,
                format="JPEG",
                quality=JPEG_QUALITY,
                optimize=False,
            )
            frames.append(ensure_jfif(buffer.getvalue()))
    except ImageUploadError:
        raise
    except (OSError, ValueError, EOFError) as exc:
        raise ImageUploadError(f"cannot encode image {source!r}: {exc}") from exc
    finally:
        image.close()

    return EncodedAnimation(frames=tuple(frames), delays_ms=tuple(delays))
