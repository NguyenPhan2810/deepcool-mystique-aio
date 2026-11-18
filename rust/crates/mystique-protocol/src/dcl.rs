//! The 64-byte `DCLd` frame header and the upload content id.
//!
//! Source: the 2026-11-18 USBPcap capture of the vendor app on this **360**
//! (`re/captures/usbpcap-upload.pcap`, decoded to `re/captures/upload-decoded.json`;
//! written up in `re/findings/12-usbpcap-360-protocol.md`). All 14 captured headers
//! pass the checksum below, so the layout is now observed here rather than inherited
//! from the 240.
//!
//! ```text
//! off   size  field
//! 0     4     "DCLd"
//! 4     1     frame kind: 01 static, 02 animated (observed; semantics inferred)
//! 5     4     JPEG length, u32 LE
//! 9     4     sum of JPEG bytes mod 65536, u32 LE (upper two bytes zero)
//! 13    1     total frame count; 0 for a static image
//! 14    1     unused
//! 15    2     per-frame delay in ms, u16 LE; 0 for a static image
//! 17    1     0-based frame index
//! 18    2     unused
//! 20    32    content/session id, 32 lowercase ASCII hex chars
//! 52    10    unused
//! 62    2     sum of header bytes 0..61, u16 LE
//! ```
//!
//! The transfer boundary is not part of the header but is equally load-bearing: the
//! header is its own 64-byte USB transfer and the JPEG follows in 64-byte transfers.
//! [`crate::upload`] models that split so it cannot be collapsed into one buffer.

use md5::{Digest, Md5};

use crate::packet::checksum16;

pub const DCLD_HEADER_LEN: usize = 64;
pub const DCLD_MAGIC: [u8; 4] = *b"DCLd";

/// Header byte 4 for a single still image. Observed on both static uploads.
pub const DCLD_KIND_STATIC: u8 = 0x01;
/// Header byte 4 for a multi-frame upload. Observed on all twelve GIF frames.
pub const DCLD_KIND_ANIMATED: u8 = 0x02;

/// The upload terminator.
///
/// Spelling: the capture contains `dcldfinish` three times, once per upload, and never
/// `DCLdfinish`. The transfer is 55 bytes — ten ASCII bytes followed by 45 zeros — not
/// the 64 bytes the 240-derived notes claimed. 55 is what the URBs actually carry.
pub const FINISH_MAGIC: &[u8] = b"dcldfinish";
pub const FINISH_LEN: usize = 55;

pub const HEADER_CHECKSUM_COVERAGE: usize = 62;

/// A stable 32-lowercase-hex identifier sent in the frame header.
///
/// The device does not validate it. The capture also proves it is **not** a content
/// hash: two byte-identical static JPEGs were uploaded with different ids
/// (`9b498283…` and `86d9ab0f…`). It is a per-upload correlation/session tag.
///
/// Because this crate has no randomness source, an MD5 over the concatenated JPEG
/// bytes is used as a deterministic placeholder. It is stable for a given upload and
/// unique enough to correlate logs; it is not a security primitive.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentId([u8; 32]);

impl ContentId {
    /// MD5 over the concatenated JPEG byte streams, rendered as lowercase hex.
    ///
    /// This is deterministic: two uploads of the same bytes get the same id. The
    /// vendor instead uses a fresh id per upload; see [`ContentId::for_upload`] when
    /// that distinction matters.
    pub fn for_jpegs<'a>(jpegs: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut hasher = Md5::new();
        for jpeg in jpegs {
            hasher.update(jpeg);
        }
        Self::render(&hasher.finalize())
    }

    /// MD5 over a caller-supplied nonce followed by the JPEG bytes, lowercase hex.
    ///
    /// The capture shows the vendor's id is per upload, not a content hash: two
    /// byte-identical static JPEGs were uploaded with different ids. Pass a timestamp
    /// or random value as `nonce` to reproduce that behaviour. The device does not
    /// validate the value, so this is a correlation tag rather than a security field.
    pub fn for_upload<'a>(jpegs: impl IntoIterator<Item = &'a [u8]>, nonce: &[u8]) -> Self {
        let mut hasher = Md5::new();
        hasher.update(nonce);
        for jpeg in jpegs {
            hasher.update(jpeg);
        }
        Self::render(&hasher.finalize())
    }

    fn render(digest: &[u8]) -> Self {
        let mut out = [0u8; 32];
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for (index, byte) in digest.iter().take(16).enumerate() {
            out[index * 2] = HEX[usize::from(byte >> 4)];
            out[index * 2 + 1] = HEX[usize::from(byte & 0x0F)];
        }
        Self(out)
    }

    /// Parses exactly 32 ASCII hex characters, either case, and stores lowercase.
    pub fn parse(text: &str) -> Result<Self, ContentIdError> {
        let bytes = text.as_bytes();
        if bytes.len() != 32 {
            return Err(ContentIdError::Length(bytes.len()));
        }
        let mut out = [0u8; 32];
        for (index, byte) in bytes.iter().enumerate() {
            if !byte.is_ascii_hexdigit() {
                return Err(ContentIdError::NotHex(index));
            }
            out[index] = byte.to_ascii_lowercase();
        }
        Ok(Self(out))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Always valid ASCII because every constructor restricts the alphabet.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("00000000000000000000000000000000")
    }
}

impl std::fmt::Debug for ContentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for ContentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl serde::Serialize for ContentId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentIdError {
    #[error("content id is {0} characters; expected 32")]
    Length(usize),
    #[error("content id character {0} is not ASCII hex")]
    NotHex(usize),
}

/// Builds the 64-byte header for one frame.
///
/// The JPEG checksum is computed here, from `jpeg`, so the length and checksum fields
/// can never disagree with the bytes that follow the header. `frame_count` is the
/// captured discriminator: the vendor sends `0` for a static image and the real count
/// for a multi-frame upload, and header byte 4 follows it (`01` static, `02` animated).
pub fn build_header(
    jpeg: &[u8],
    frame_count: u8,
    delay_ms: u16,
    frame_index: u8,
    content_id: &ContentId,
) -> [u8; DCLD_HEADER_LEN] {
    let mut header = [0u8; DCLD_HEADER_LEN];
    header[0..4].copy_from_slice(&DCLD_MAGIC);
    header[4] = if frame_count == 0 {
        DCLD_KIND_STATIC
    } else {
        DCLD_KIND_ANIMATED
    };

    // A JPEG longer than u32::MAX cannot be transferred by this protocol at all.
    // Saturating keeps this function infallible and non-panicking; the upload plan
    // rejects such input earlier via its own length checks.
    let length = u32::try_from(jpeg.len()).unwrap_or(u32::MAX);
    header[5..9].copy_from_slice(&length.to_le_bytes());

    let jpeg_sum = u32::from(checksum16(jpeg));
    header[9..13].copy_from_slice(&jpeg_sum.to_le_bytes());

    header[13] = frame_count;
    header[15..17].copy_from_slice(&delay_ms.to_le_bytes());
    header[17] = frame_index;
    header[20..52].copy_from_slice(content_id.as_bytes());

    let header_sum = checksum16(&header[..HEADER_CHECKSUM_COVERAGE]);
    header[62..64].copy_from_slice(&header_sum.to_le_bytes());
    header
}

/// `"dcldfinish"` zero-padded to 55 bytes, the length the vendor's transfer carries.
pub fn build_finish() -> [u8; FINISH_LEN] {
    let mut packet = [0u8; FINISH_LEN];
    packet[..FINISH_MAGIC.len()].copy_from_slice(FINISH_MAGIC);
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn id() -> ContentId {
        ContentId::parse(ID).unwrap()
    }

    #[test]
    fn header_field_offsets() {
        let jpeg = [0x11u8; 100];
        let header = build_header(&jpeg, 3, 40, 1, &id());

        assert_eq!(&header[0..4], b"DCLd");
        assert_eq!(header[4], DCLD_KIND_ANIMATED);
        assert_eq!(u32::from_le_bytes(header[5..9].try_into().unwrap()), 100);
        assert_eq!(
            u32::from_le_bytes(header[9..13].try_into().unwrap()),
            100 * 0x11
        );
        assert_eq!(header[13], 3);
        assert_eq!(header[14], 0);
        assert_eq!(u16::from_le_bytes([header[15], header[16]]), 40);
        assert_eq!(header[17], 1);
        assert_eq!(header[18], 0);
        assert_eq!(header[19], 0);
        assert_eq!(&header[20..52], ID.as_bytes());
        assert_eq!(&header[52..62], &[0u8; 10]);
        assert_eq!(
            u16::from_le_bytes([header[62], header[63]]),
            checksum16(&header[..62])
        );
        assert_eq!(header.len(), DCLD_HEADER_LEN);
    }

    #[test]
    fn static_frames_use_count_zero_and_kind_one() {
        let header = build_header(&[1, 2, 3], 0, 0, 0, &id());
        assert_eq!(header[4], DCLD_KIND_STATIC);
        assert_eq!(header[13], 0, "static frame_count is 0 in the capture");
        assert_eq!(u16::from_le_bytes([header[15], header[16]]), 0);
        assert_eq!(header[17], 0);
    }

    #[test]
    fn animated_frames_use_kind_two_and_the_real_count() {
        let header = build_header(&[1, 2, 3], 12, 858, 7, &id());
        assert_eq!(header[4], DCLD_KIND_ANIMATED);
        assert_eq!(header[13], 12);
        assert_eq!(u16::from_le_bytes([header[15], header[16]]), 858);
        assert_eq!(header[17], 7);
    }

    #[test]
    fn header_sum_covers_only_the_first_sixty_two_bytes() {
        let header = build_header(&[1, 2, 3], 1, 0, 0, &id());
        assert_eq!(
            checksum16(&header[..62]),
            u16::from_le_bytes([header[62], header[63]])
        );
        // Recomputing over the whole header (including the checksum field) gives a
        // different value; the device sums 0..61 only.
        assert_ne!(
            checksum16(&header),
            u16::from_le_bytes([header[62], header[63]])
        );
    }

    #[test]
    fn jpeg_sum_wraps_at_sixteen_bits_and_stays_in_the_low_half() {
        let jpeg = [0xFFu8; 300]; // 76500 -> 0x12B74, truncated to 0x2B74
        let header = build_header(&jpeg, 1, 0, 0, &id());
        let field = &header[9..13];
        assert_eq!(field[2], 0);
        assert_eq!(field[3], 0);
        assert_eq!(u16::from_le_bytes([field[0], field[1]]), 76500u32 as u16);
    }

    #[test]
    fn content_id_is_md5_lowercase_hex() {
        let id = ContentId::for_jpegs([b"abc".as_slice()]);
        assert_eq!(id.as_str(), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(id.as_str().len(), 32);
        assert!(id
            .as_str()
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn content_id_is_deterministic_and_order_sensitive() {
        let a = ContentId::for_jpegs([b"one".as_slice(), b"two".as_slice()]);
        let b = ContentId::for_jpegs([b"one".as_slice(), b"two".as_slice()]);
        let c = ContentId::for_jpegs([b"two".as_slice(), b"one".as_slice()]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn a_per_upload_id_changes_with_the_nonce() {
        let jpegs = [b"same".as_slice()];
        let a = ContentId::for_upload(jpegs, b"nonce-a");
        let b = ContentId::for_upload(jpegs, b"nonce-b");
        let c = ContentId::for_upload(jpegs, b"nonce-a");
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert!(a
            .as_str()
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase()));
    }

    #[test]
    fn content_id_parser_rejects_junk_without_panicking() {
        assert_eq!(
            ContentId::parse("abc").unwrap_err(),
            ContentIdError::Length(3)
        );
        assert_eq!(
            ContentId::parse("ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ").unwrap_err(),
            ContentIdError::NotHex(0)
        );
        let upper = ContentId::parse("900150983CD24FB0D6963F7D28E17F72").unwrap();
        assert_eq!(upper.as_str(), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn finish_is_zero_padded_to_fifty_five_bytes() {
        let finish = build_finish();
        assert_eq!(&finish[..10], b"dcldfinish");
        assert!(finish[10..].iter().all(|byte| *byte == 0));
        assert_eq!(finish.len(), FINISH_LEN);
        assert_eq!(finish.len(), 55);
    }
}
