//! Turning an image or GIF into upload frames.
//!
//! Two properties are non-negotiable because violating them fails **silently** on the
//! panel (`re/findings/06-image-upload-protocol.md`, traps 2 and 3):
//!
//! 1. the JPEG must carry a JFIF APP0 segment immediately after SOI;
//! 2. the Huffman tables must be standard, not optimised.
//!
//! Property 1 is enforced and repaired here. Property 2 is a property of the encoder;
//! `image`'s JPEG encoder uses standard tables, and there is no optimise switch exposed
//! through this path. A hand-optimised JPEG passed straight through cannot be detected
//! without decoding the entropy stream, so it is documented rather than checked.
//!
//! # Fit modes
//!
//! The panel is 480x640 portrait. [`FitMode::Contain`] letterboxes (the old behaviour);
//! [`FitMode::Cover`] crops the overflow so the picture fills the panel, which is what
//! the vendor app does before upload. Cover is the default for the desktop app, because
//! a horizontal GIF letterboxed on a portrait panel is mostly black bands
//! (`re/findings/15-driver-verification.md`).
//!
//! This crate exists so the `mystique` CLI and the `mystique-ui` desktop app encode
//! frames through the same code. It performs file I/O but no USB access.

#![forbid(unsafe_code)]

pub mod fit;
pub mod preview;

use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

use image::codecs::gif::GifDecoder;
use image::codecs::jpeg::JpegEncoder;
use image::{AnimationDecoder, DynamicImage, ImageError, ImageFormat, RgbaImage};
use mystique_protocol::UploadFrame;
use thiserror::Error;

pub use fit::{fit_to_panel, FitMode};
pub use preview::{preview_data_url, PREVIEW_WIDTH};

/// Panel resolution, vendor-stated. Uploads are portrait.
pub const PANEL_WIDTH: u32 = 480;
pub const PANEL_HEIGHT: u32 = 640;

/// Quality is a display choice, not a protocol field. 90 keeps text legible without
/// producing frames much larger than the vendor's.
pub const JPEG_QUALITY: u8 = 90;

/// Everything decoded from one file, including the source size the panel never sees.
#[derive(Debug, Clone)]
pub struct LoadedMedia {
    pub frames: Vec<UploadFrame>,
    pub source_width: u32,
    pub source_height: u32,
    /// The fit mode the frames were actually encoded with.
    pub fit: FitMode,
}

/// A standard JFIF 1.01 APP0 segment: no density units, 1:1 aspect, no thumbnail.
const JFIF_APP0: [u8; 18] = [
    0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01,
    0x00, 0x00,
];

/// Everything that can go wrong while reading an image.
///
/// The path is carried on every variant so a caller can show which file failed without
/// guessing from the message.
#[derive(Debug, Error)]
pub enum MediaError {
    #[error("reading {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a decodable image: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: ImageError,
    },
    #[error("{path} is not a JPEG, PNG or GIF")]
    Unsupported { path: PathBuf },
    #[error("{path} contains no frames")]
    NoFrames { path: PathBuf },
    #[error("encoding JPEG: {0}")]
    Encode(#[from] ImageError),
    #[error("not a JPEG: missing SOI marker")]
    NotJpeg,
}

fn read_error(path: &Path, source: std::io::Error) -> MediaError {
    MediaError::Read {
        path: path.to_path_buf(),
        source,
    }
}

fn decode_error(path: &Path, source: ImageError) -> MediaError {
    MediaError::Decode {
        path: path.to_path_buf(),
        source,
    }
}

/// Loads one still image or every frame of a GIF, letterboxed (the CLI's behaviour).
///
/// The desktop app wants [`Cover`](FitMode::Cover); use [`load_with`] for that.
pub fn load(path: &Path) -> Result<Vec<UploadFrame>, MediaError> {
    Ok(load_with(path, FitMode::Contain)?.frames)
}

/// Loads one still image or every frame of a GIF with an explicit fit mode.
///
/// A JPEG already at exactly 480x640 is passed through byte-for-byte (after the JFIF
/// check) to avoid a needless recompression. Anything else is decoded, fitted to the
/// panel and re-encoded, because the panel only accepts portrait 480x640 uploads.
pub fn load_with(path: &Path, fit: FitMode) -> Result<LoadedMedia, MediaError> {
    let bytes = std::fs::read(path).map_err(|source| read_error(path, source))?;

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return load_gif(path, fit);
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let dimensions = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_dimensions().ok());
        if dimensions == Some((PANEL_WIDTH, PANEL_HEIGHT)) {
            return Ok(LoadedMedia {
                frames: vec![UploadFrame::new(ensure_jfif(&bytes)?)],
                source_width: PANEL_WIDTH,
                source_height: PANEL_HEIGHT,
                fit,
            });
        }
        let (source_width, source_height) = dimensions.unwrap_or((0, 0));
        let image = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg)
            .map_err(|source| decode_error(path, source))?;
        return Ok(LoadedMedia {
            frames: vec![UploadFrame::new(encode_frame(&image.to_rgba8(), fit)?)],
            source_width,
            source_height,
            fit,
        });
    }

    let reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|source| read_error(path, source))?;
    if !matches!(reader.format(), Some(ImageFormat::Jpeg | ImageFormat::Png)) {
        return Err(MediaError::Unsupported {
            path: path.to_path_buf(),
        });
    }
    let image = reader
        .decode()
        .map_err(|source| decode_error(path, source))?;
    let (source_width, source_height) = (image.width(), image.height());
    Ok(LoadedMedia {
        frames: vec![UploadFrame::new(encode_frame(&image.to_rgba8(), fit)?)],
        source_width,
        source_height,
        fit,
    })
}

fn load_gif(path: &Path, fit: FitMode) -> Result<LoadedMedia, MediaError> {
    let file = File::open(path).map_err(|source| read_error(path, source))?;
    let decoder =
        GifDecoder::new(BufReader::new(file)).map_err(|source| decode_error(path, source))?;

    let frames: Vec<image::Frame> = decoder
        .into_frames()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| decode_error(path, source))?;
    if frames.is_empty() {
        return Err(MediaError::NoFrames {
            path: path.to_path_buf(),
        });
    }

    let (source_width, source_height) = frames[0].buffer().dimensions();
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay_ms = numerator
            .checked_div(denominator)
            .unwrap_or(0)
            .min(u32::from(u16::MAX)) as u16;
        let jpeg = encode_frame(&frame.into_buffer(), fit)?;
        out.push(UploadFrame::new(jpeg).with_delay(delay_ms));
    }
    Ok(LoadedMedia {
        frames: out,
        source_width,
        source_height,
        fit,
    })
}

/// Fits the source into 480x640. The encoder's only entry point to [`fit_to_panel`].
pub(crate) fn encode_frame(source: &RgbaImage, fit: FitMode) -> Result<Vec<u8>, MediaError> {
    let fitted = fit_to_panel(source, fit);
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
        .encode_image(&DynamicImage::ImageRgb8(fitted))
        .map_err(MediaError::Encode)?;
    ensure_jfif(&jpeg)
}

/// Guarantees SOI followed immediately by a JFIF APP0 segment.
///
/// Returns the input unchanged when it already complies, so a genuine vendor-style
/// JPEG is byte-identical after the check.
pub fn ensure_jfif(jpeg: &[u8]) -> Result<Vec<u8>, MediaError> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return Err(MediaError::NotJpeg);
    }
    let compliant =
        jpeg.len() >= 11 && jpeg[2] == 0xFF && jpeg[3] == 0xE0 && &jpeg[6..11] == b"JFIF\0";
    if compliant {
        return Ok(jpeg.to_vec());
    }

    let mut out = Vec::with_capacity(jpeg.len() + JFIF_APP0.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&JFIF_APP0);
    out.extend_from_slice(&jpeg[2..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mystique_protocol::upload::check_jfif;

    fn tiny_png() -> Vec<u8> {
        // A 2x1 red/blue PNG built in memory to keep the test independent of files.
        let mut image = RgbaImage::new(2, 1);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        image.put_pixel(1, 0, image::Rgba([0, 0, 255, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("mystique-media-test");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn inserts_a_jfif_segment_when_missing() {
        // SOI + a fake APP1 + EOI.
        let input = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x02, 0xFF, 0xD9];
        let patched = ensure_jfif(&input).unwrap();
        assert_eq!(&patched[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
        assert_eq!(&patched[6..11], b"JFIF\0");
        assert_eq!(&patched[patched.len() - 2..], &[0xFF, 0xD9]);
    }

    #[test]
    fn leaves_a_compliant_jpeg_byte_identical() {
        let input = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xD9,
        ];
        assert_eq!(ensure_jfif(&input).unwrap(), input);
    }

    #[test]
    fn rejects_non_jpegs() {
        assert!(matches!(ensure_jfif(&[]), Err(MediaError::NotJpeg)));
        assert!(matches!(
            ensure_jfif(&[0x89, b'P', b'N', b'G']),
            Err(MediaError::NotJpeg)
        ));
    }

    #[test]
    fn encoded_frames_pass_the_protocol_check() {
        let rgba = RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 255]));
        let jpeg = encode_frame(&rgba, FitMode::Contain).unwrap();
        assert_eq!(&jpeg[..4], &[0xFF, 0xD8, 0xFF, 0xE0]);
        check_jfif(0, &jpeg).unwrap();
    }

    #[test]
    fn a_panel_sized_jpeg_is_passed_through_unchanged() {
        let rgba = RgbaImage::from_pixel(PANEL_WIDTH, PANEL_HEIGHT, image::Rgba([4, 5, 6, 255]));
        let encoded = encode_frame(&rgba, FitMode::Contain).unwrap();
        let path = temp_path("panel.jpg");
        std::fs::write(&path, &encoded).unwrap();

        let frames = load(&path).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].jpeg, encoded);
    }

    #[test]
    fn a_mis_sized_jpeg_is_fitted_and_reencoded() {
        let rgba = RgbaImage::from_pixel(200, 100, image::Rgba([7, 8, 9, 255]));
        let encoded = encode_frame(&rgba, FitMode::Contain).unwrap();
        let path = temp_path("small.jpg");
        std::fs::write(&path, &encoded).unwrap();

        let frames = load(&path).unwrap();
        assert_eq!(frames.len(), 1);
        check_jfif(0, &frames[0].jpeg).unwrap();
        let dimensions = image::ImageReader::new(Cursor::new(&frames[0].jpeg))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap();
        assert_eq!(dimensions, (PANEL_WIDTH, PANEL_HEIGHT));
    }

    #[test]
    fn load_with_reports_the_source_size_and_the_fit_used() {
        let rgba = RgbaImage::from_pixel(1000, 500, image::Rgba([7, 8, 9, 255]));
        let path = temp_path("source-size.png");
        image::save_buffer(
            &path,
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ColorType::Rgba8,
        )
        .unwrap();

        let loaded = load_with(&path, FitMode::Cover).unwrap();
        assert_eq!((loaded.source_width, loaded.source_height), (1000, 500));
        assert_eq!(loaded.fit, FitMode::Cover);
        assert_eq!(loaded.frames.len(), 1);
        check_jfif(0, &loaded.frames[0].jpeg).unwrap();
    }

    #[test]
    fn loads_a_png_from_disk() {
        let path = temp_path("tiny.png");
        std::fs::write(&path, tiny_png()).unwrap();

        let frames = load(&path).unwrap();
        assert_eq!(frames.len(), 1);
        check_jfif(0, &frames[0].jpeg).unwrap();
        assert!(frames[0].jpeg.len() > 100);
    }

    #[test]
    fn an_unknown_format_is_named() {
        let path = temp_path("not-an-image.bin");
        std::fs::write(&path, b"definitely not an image").unwrap();
        assert!(matches!(load(&path), Err(MediaError::Unsupported { .. })));
    }
}
