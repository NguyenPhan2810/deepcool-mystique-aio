//! A small PNG data URL of an encoded frame, for the upload preview.
//!
//! The preview decodes the exact JPEG that will be sent, so it shows the crop and the
//! encoder's result rather than a second rendering of the source. It is a convenience:
//! a failure here must never block an upload, so every error becomes `None`.

use std::io::Cursor;

use image::imageops::FilterType;
use image::ImageFormat;
use mystique_protocol::UploadFrame;

use crate::{PANEL_HEIGHT, PANEL_WIDTH};

/// Width of the thumbnail. The panel is portrait, so the height follows exactly.
pub const PREVIEW_WIDTH: u32 = 120;

/// A `data:image/png;base64,...` URL for one frame, or `None` if it cannot be produced.
pub fn preview_data_url(frame: &UploadFrame) -> Option<String> {
    let image = image::load_from_memory(&frame.jpeg).ok()?;
    let height = (PREVIEW_WIDTH * PANEL_HEIGHT + PANEL_WIDTH / 2) / PANEL_WIDTH;
    let thumbnail = image.resize(PREVIEW_WIDTH, height, FilterType::Lanczos3);
    let mut png = Vec::new();
    thumbnail
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", base64(&png)))
}

/// Standard base64 with padding. Twenty lines beats a dependency for one thumbnail.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[(a >> 2) as usize] as char);
        out.push(ALPHABET[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(((b & 0x0F) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(c & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn base64_matches_the_reference_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_png_data_url_is_produced_from_a_real_jpeg() {
        let rgba =
            RgbaImage::from_pixel(PANEL_WIDTH / 4, PANEL_HEIGHT / 4, Rgba([40, 50, 60, 255]));
        let frame = UploadFrame::new(crate::encode_frame(&rgba, crate::FitMode::Cover).unwrap());
        let url = preview_data_url(&frame).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(url.len() > 200);
    }

    #[test]
    fn a_broken_frame_yields_none_rather_than_panicking() {
        assert!(preview_data_url(&UploadFrame::new(vec![0x00, 0x01])).is_none());
    }
}
