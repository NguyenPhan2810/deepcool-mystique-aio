"""Hardware-free tests for the image/GIF upload plan and JPEG encoding.

Header and sequence tests always run. Encoding tests skip when Pillow is not
installed, which keeps the telemetry-only install testable.
"""

import hashlib
import io
import sys

import pytest

from mystique.image_upload import (
    FRAME_HEADER_SIZE,
    IMAGE_MODE_HEARTBEAT_PAYLOAD,
    IMAGE_MODE_SWITCH_PAYLOAD,
    JFIF_APP0_SEGMENT,
    MAX_FRAMES,
    ImageSupportMissing,
    ImageUploadError,
    build_finish_marker,
    build_frame_header,
    build_image_mode_handshake,
    build_upload_sequence,
    content_id,
    encode_frames,
    ensure_jfif,
)

CONTENT_ID = "0123456789abcdef0123456789abcdef"

# ITU T.81 Annex K.2 bit-length counts for the standard Huffman tables.
# optimize=False must produce these, not content-optimized counts.
STANDARD_DC_LUMA = bytes([0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0])
STANDARD_DC_CHROMA = bytes([0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0])
STANDARD_AC_LUMA = bytes([0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7D])
STANDARD_AC_CHROMA = bytes([0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77])


def _jpeg():
    return bytes.fromhex("FFD8FFE0") + bytes(range(1, 33))


def test_frame_header_is_byte_exact():
    jpeg = _jpeg()
    header = build_frame_header(1, 3, jpeg, 0x0123, CONTENT_ID)

    assert len(header) == FRAME_HEADER_SIZE
    assert header[0:4] == b"DCLd"
    assert header[4] == 0x02
    assert int.from_bytes(header[5:9], "little") == len(jpeg)
    assert int.from_bytes(header[9:13], "little") == sum(jpeg) & 0xFFFF
    assert header[13] == 3
    assert header[14] == 0
    assert int.from_bytes(header[15:17], "little") == 0x0123
    assert header[17] == 1
    assert header[18:20] == b"\x00\x00"
    assert header[20:52] == CONTENT_ID.encode("ascii")
    assert header[52:62] == bytes(10)
    assert int.from_bytes(header[62:64], "little") == sum(header[:62]) & 0xFFFF


def test_frame_header_checksum_covers_the_header_before_the_checksum_field():
    header = build_frame_header(0, 1, _jpeg(), 100, CONTENT_ID)
    stored = int.from_bytes(header[62:64], "little")

    assert stored == sum(header[:62]) & 0xFFFF
    assert stored != sum(header) & 0xFFFF

    tampered = bytearray(header)
    tampered[20] ^= 0x01  # any change in the content id must invalidate it
    assert int.from_bytes(tampered[62:64], "little") != sum(tampered[:62]) & 0xFFFF


@pytest.mark.parametrize(
    ("index", "total", "jpeg", "delay", "cid"),
    [
        (0, 1, b"", 100, CONTENT_ID),
        (1, 1, _jpeg(), 100, CONTENT_ID),
        (0, 0, _jpeg(), 100, CONTENT_ID),
        (0, MAX_FRAMES + 1, _jpeg(), 100, CONTENT_ID),
        (0, 1, _jpeg(), -1, CONTENT_ID),
        (0, 1, _jpeg(), 0x10000, CONTENT_ID),
        (0, 1, _jpeg(), 100, "too-short"),
        (0, 1, _jpeg(), 100, "zz" * 16),
    ],
)
def test_frame_header_rejects_invalid_fields(index, total, jpeg, delay, cid):
    with pytest.raises(ImageUploadError):
        build_frame_header(index, total, jpeg, delay, cid)


def test_finish_marker_is_zero_padded_ascii():
    marker = build_finish_marker()
    assert len(marker) == FRAME_HEADER_SIZE
    assert marker[:10] == b"dcldfinish"
    assert marker[10:] == bytes(FRAME_HEADER_SIZE - 10)


def test_content_id_is_md5_of_the_concatenated_frames():
    frames = [b"\xff\xd8one", b"\xff\xd8two"]
    assert content_id(frames) == hashlib.md5(b"".join(frames)).hexdigest()
    assert len(content_id(frames)) == 32


def test_upload_sequence_has_the_exact_verified_order():
    frame = _jpeg()
    operations = build_upload_sequence([frame], [100], content_id_value=CONTENT_ID)

    # 1 erase + 16 handshake + 1 begin + 1 frame + 1 finish + 1 commit
    # + 16 replay = 37 transfers.
    assert len(operations) == 37

    assert operations[0].command == 0x14
    assert operations[0].data[3:42] == bytes(39)

    handshake = operations[1:17]
    assert [operation.command for operation in handshake] == [
        0x0A,
        0x10,
        0x01,
        0x12,
        0x11,
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
    ]
    assert handshake[0].data[3:11] == IMAGE_MODE_SWITCH_PAYLOAD
    assert handshake[2].data[3:12] == IMAGE_MODE_HEARTBEAT_PAYLOAD
    assert all(operation.endpoint == 0x01 for operation in operations)
    assert all(operation.reply_endpoint == 0x81 for operation in handshake)

    assert operations[17].command == 0x0F  # begin upload
    assert operations[17].data[3:42] == bytes(39)

    assert all(
        operation.validate_reply is False
        for operation in operations
        if operation.command is not None
    )

    uploaded = operations[18]
    assert uploaded.reply_endpoint is None
    assert uploaded.data[:4] == b"DCLd"
    assert uploaded.data[64:] == frame
    assert int.from_bytes(uploaded.data[5:9], "little") == len(frame)
    assert uploaded.data[20:52] == CONTENT_ID.encode("ascii")

    assert operations[19].data == build_finish_marker()
    assert operations[19].reply_endpoint is None

    assert operations[20].command == 0x08
    assert operations[20].data[3:5] == b"\x01\x01"

    replay = operations[21:37]
    assert [operation.command for operation in replay] == [
        operation.command for operation in handshake
    ]
    assert [operation.data for operation in replay] == [
        operation.data for operation in handshake
    ]


def test_setup_steps_carry_the_ten_millisecond_settle():
    operations = build_upload_sequence([_jpeg()], [100])
    handshake = operations[1:17]
    assert all(operation.settle_ms == 0 for operation in handshake[:5])
    assert all(operation.settle_ms == 10 for operation in handshake[5:])


def test_two_frames_get_sequential_headers_and_one_content_id():
    frames = [_jpeg(), _jpeg() + b"second"]
    operations = build_upload_sequence(frames, [40, 250], content_id_value=CONTENT_ID)

    uploaded = [
        operation
        for operation in operations
        if operation.reply_endpoint is None and operation.data[:4] == b"DCLd"
    ]
    assert len(uploaded) == 2
    assert [operation.data[17] for operation in uploaded] == [0, 1]
    assert [operation.data[13] for operation in uploaded] == [2, 2]
    assert [
        int.from_bytes(operation.data[15:17], "little") for operation in uploaded
    ] == [
        40,
        250,
    ]
    assert {operation.data[20:52] for operation in uploaded} == {
        CONTENT_ID.encode("ascii")
    }
    assert uploaded[0].data[64:] == frames[0]
    assert uploaded[1].data[64:] == frames[1]


def test_image_mode_handshake_can_be_built_on_its_own():
    operations = build_image_mode_handshake(rotation=3, label="handshake")
    assert len(operations) == 16
    assert operations[0].description == "handshake"
    config = next(operation for operation in operations if operation.command == 0x02)
    assert config.data[5] == 3  # payload byte 2 is rotation


@pytest.mark.parametrize(
    ("frames", "delays"),
    [
        ([], []),
        ([_jpeg()], []),
        ([_jpeg()], [100, 100]),
        ([b""], [100]),
        ([_jpeg()] * (MAX_FRAMES + 1), [100] * (MAX_FRAMES + 1)),
    ],
)
def test_upload_sequence_rejects_invalid_frame_lists(frames, delays):
    with pytest.raises(ImageUploadError):
        build_upload_sequence(frames, delays)


def test_upload_sequence_rejects_an_invalid_content_id():
    with pytest.raises(ImageUploadError):
        build_upload_sequence([_jpeg()], [100], content_id_value="not-a-content-id")


def test_ensure_jfif_inserts_the_standard_segment_after_soi():
    without = bytes.fromhex("FFD8FFDB0004AAAA") + b"rest"
    fixed = ensure_jfif(without)

    assert fixed[:2] == b"\xff\xd8"
    assert fixed[2:20] == JFIF_APP0_SEGMENT
    assert fixed[20:] == without[2:]
    assert fixed[:4] == b"\xff\xd8\xff\xe0"
    assert b"JFIF\x00" in fixed[:24]
    assert ensure_jfif(fixed) == fixed  # idempotent


def test_ensure_jfif_rejects_non_jpeg_data():
    with pytest.raises(ImageUploadError):
        ensure_jfif(b"not a jpeg")


def test_missing_pillow_is_reported(monkeypatch, tmp_path):
    monkeypatch.setitem(sys.modules, "PIL", None)
    with pytest.raises(ImageSupportMissing):
        encode_frames(tmp_path / "anything.png")


# -- Pillow-dependent -------------------------------------------------------


def _dht_bit_counts(jpeg):
    """Return {table id: 16 bit-length counts} from the JPEG's DHT segments."""
    tables = {}
    index = 2
    while index + 4 <= len(jpeg):
        if jpeg[index] != 0xFF:
            break
        marker = jpeg[index + 1]
        if marker == 0xDA:  # start of scan: metadata is over
            break
        if marker == 0xD8 or 0xD0 <= marker <= 0xD9:
            index += 2
            continue
        length = int.from_bytes(jpeg[index + 2 : index + 4], "big")
        payload = jpeg[index + 4 : index + 2 + length]
        if marker == 0xC4:  # DHT
            offset = 0
            while offset + 17 <= len(payload):
                table_id = payload[offset]
                counts = payload[offset + 1 : offset + 17]
                tables[table_id] = bytes(counts)
                offset += 17 + sum(counts)
        index += 2 + length
    return tables


def test_encoded_jpeg_has_jfif_and_standard_huffman_tables(tmp_path):
    Image = pytest.importorskip("PIL.Image")
    source = tmp_path / "source.png"
    Image.new("RGB", (960, 1280), (200, 30, 30)).save(source)

    animation = encode_frames(source)

    assert len(animation) == 1
    jpeg = animation.frames[0]
    assert jpeg[:4] == b"\xff\xd8\xff\xe0"
    assert b"JFIF\x00" in jpeg[:24]

    tables = _dht_bit_counts(jpeg)
    assert tables[0x00] == STANDARD_DC_LUMA
    assert tables[0x01] == STANDARD_DC_CHROMA
    assert tables[0x10] == STANDARD_AC_LUMA
    assert tables[0x11] == STANDARD_AC_CHROMA

    decoded = Image.open(io.BytesIO(jpeg))
    assert decoded.size == (480, 640)


def test_contain_letterboxes_on_the_reference_background(tmp_path):
    Image = pytest.importorskip("PIL.Image")
    source = tmp_path / "square.png"
    Image.new("RGB", (100, 100), (255, 255, 255)).save(source)

    animation = encode_frames(source, "contain")
    decoded = Image.open(io.BytesIO(animation.frames[0])).convert("RGB")

    assert sum(decoded.getpixel((5, 5))) < 120  # #0b0f14 background
    assert sum(decoded.getpixel((240, 320))) > 600  # white image in the middle


def test_cover_fills_the_frame(tmp_path):
    Image = pytest.importorskip("PIL.Image")
    source = tmp_path / "square.png"
    Image.new("RGB", (100, 100), (255, 255, 255)).save(source)

    animation = encode_frames(source, "cover")
    decoded = Image.open(io.BytesIO(animation.frames[0])).convert("RGB")

    assert decoded.size == (480, 640)
    assert sum(decoded.getpixel((5, 5))) > 600


def test_gif_frames_and_delays_are_preserved(tmp_path):
    Image = pytest.importorskip("PIL.Image")
    first = Image.new("RGB", (120, 160), (255, 0, 0))
    second = Image.new("RGB", (120, 160), (0, 255, 0))
    source = tmp_path / "animation.gif"
    first.save(
        source,
        format="GIF",
        save_all=True,
        append_images=[second],
        duration=[50, 200],
        loop=0,
    )

    animation = encode_frames(source)

    assert len(animation) == 2
    assert animation.delays_ms == (50, 200)
    assert all(frame[:4] == b"\xff\xd8\xff\xe0" for frame in animation.frames)


def test_static_image_gets_the_default_delay(tmp_path):
    Image = pytest.importorskip("PIL.Image")
    source = tmp_path / "static.png"
    Image.new("RGB", (10, 10), (0, 0, 255)).save(source)

    animation = encode_frames(source)

    assert animation.delays_ms == (100,)
