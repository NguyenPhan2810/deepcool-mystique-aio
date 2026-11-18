//! Mapping a source image onto the 480x640 panel.
//!
//! [`FitMode::Contain`] is the old behaviour: scale to fit, centre on black. The panel
//! then shows bands wherever the aspect ratio differs.
//!
//! [`FitMode::Cover`] is what the vendor app does before upload
//! (`re/findings/15-driver-verification.md`): crop the source to the panel's aspect
//! ratio, then scale to the panel. A horizontal GIF fills the portrait panel instead of
//! sitting between two black bands. Cropping before resizing also keeps the intermediate
//! image small, which matters for a 4:1 source.

use image::imageops::FilterType;
use image::{Rgb, RgbImage, RgbaImage};

use crate::{PANEL_HEIGHT, PANEL_WIDTH};

/// How a source image is mapped onto the 480x640 panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FitMode {
    /// Scale to fill and crop the overflow, centred. No blank bands. The default.
    #[default]
    Cover,
    /// Scale to fit and letterbox the remainder on black. Nothing is cropped.
    Contain,
}

impl FitMode {
    /// The lowercase word the IPC uses. Stable: the frontend sends it back.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
        }
    }

    /// Parses the IPC spelling. Case-insensitive; anything else is rejected rather than
    /// silently defaulted, so a typo cannot crop a picture the owner did not expect.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "cover" => Some(Self::Cover),
            "contain" => Some(Self::Contain),
            _ => None,
        }
    }
}

/// Fits the source into 480x640 with the requested mode.
pub fn fit_to_panel(source: &RgbaImage, fit: FitMode) -> RgbImage {
    let (width, height) = source.dimensions();
    if width == 0 || height == 0 {
        return RgbImage::new(PANEL_WIDTH, PANEL_HEIGHT);
    }

    match fit {
        FitMode::Contain => contain(source),
        FitMode::Cover => cover(source),
    }
}

fn contain(source: &RgbaImage) -> RgbImage {
    let (width, height) = source.dimensions();
    let scale = (f64::from(PANEL_WIDTH) / f64::from(width))
        .min(f64::from(PANEL_HEIGHT) / f64::from(height));
    let target_width = ((f64::from(width) * scale).round() as u32).clamp(1, PANEL_WIDTH);
    let target_height = ((f64::from(height) * scale).round() as u32).clamp(1, PANEL_HEIGHT);

    let resized =
        image::imageops::resize(source, target_width, target_height, FilterType::Lanczos3);
    let mut canvas = RgbImage::new(PANEL_WIDTH, PANEL_HEIGHT);
    let offset_x = (PANEL_WIDTH - target_width) / 2;
    let offset_y = (PANEL_HEIGHT - target_height) / 2;

    for (x, y, pixel) in resized.enumerate_pixels() {
        canvas.put_pixel(
            x + offset_x,
            y + offset_y,
            Rgb([pixel[0], pixel[1], pixel[2]]),
        );
    }
    canvas
}

fn cover(source: &RgbaImage) -> RgbImage {
    let (width, height) = source.dimensions();
    let panel_aspect = f64::from(PANEL_WIDTH) / f64::from(PANEL_HEIGHT);
    let source_aspect = f64::from(width) / f64::from(height);

    let (crop_width, crop_height) = if source_aspect > panel_aspect {
        // Too wide: keep full height, trim the left and right edges.
        let crop = ((f64::from(height) * panel_aspect).round() as u32).clamp(1, width);
        (crop, height)
    } else {
        // Too tall (or exact): keep full width, trim the top and bottom.
        let crop = ((f64::from(width) / panel_aspect).round() as u32).clamp(1, height);
        (width, crop)
    };
    let offset_x = (width - crop_width) / 2;
    let offset_y = (height - crop_height) / 2;

    let cropped = image::imageops::crop_imm(source, offset_x, offset_y, crop_width, crop_height);
    let resized = image::imageops::resize(
        &cropped.to_image(),
        PANEL_WIDTH,
        PANEL_HEIGHT,
        FilterType::Lanczos3,
    );
    let mut canvas = RgbImage::new(PANEL_WIDTH, PANEL_HEIGHT);
    for (x, y, pixel) in resized.enumerate_pixels() {
        canvas.put_pixel(x, y, Rgb([pixel[0], pixel[1], pixel[2]]));
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_portrait_and_landscape_sources_into_the_panel() {
        let landscape = RgbaImage::from_pixel(1000, 500, image::Rgba([1, 2, 3, 255]));
        let fitted = fit_to_panel(&landscape, FitMode::Contain);
        assert_eq!(fitted.dimensions(), (PANEL_WIDTH, PANEL_HEIGHT));
        // 1000x500 scales to 480x240, leaving black bands top and bottom.
        assert_eq!(fitted.get_pixel(0, 0), &Rgb([0, 0, 0]));
        assert_eq!(fitted.get_pixel(240, 320), &Rgb([1, 2, 3]));

        let portrait = RgbaImage::from_pixel(500, 1000, image::Rgba([9, 9, 9, 255]));
        let fitted = fit_to_panel(&portrait, FitMode::Contain);
        assert_eq!(fitted.dimensions(), (PANEL_WIDTH, PANEL_HEIGHT));
        assert_eq!(fitted.get_pixel(0, 0), &Rgb([0, 0, 0]));
        assert_eq!(fitted.get_pixel(240, 320), &Rgb([9, 9, 9]));
    }

    #[test]
    fn cover_fills_the_panel_without_blank_bands() {
        let landscape = RgbaImage::from_pixel(1000, 500, image::Rgba([1, 2, 3, 255]));
        let fitted = fit_to_panel(&landscape, FitMode::Cover);
        assert_eq!(fitted.dimensions(), (PANEL_WIDTH, PANEL_HEIGHT));
        // Every corner comes from the source: no letterbox.
        for (x, y) in [
            (0, 0),
            (PANEL_WIDTH - 1, 0),
            (0, PANEL_HEIGHT - 1),
            (PANEL_WIDTH - 1, PANEL_HEIGHT - 1),
            (240, 320),
        ] {
            assert_eq!(fitted.get_pixel(x, y), &Rgb([1, 2, 3]), "pixel {x},{y}");
        }
    }

    #[test]
    fn cover_crops_the_centre_and_keeps_the_edges_of_a_tall_source() {
        // 300 wide, 1200 tall: cover keeps full width, trims top and bottom.
        let mut tall = RgbaImage::from_pixel(300, 1200, image::Rgba([0, 0, 0, 255]));
        // A red band at the vertical centre only.
        for y in 590..610 {
            for x in 0..300 {
                tall.put_pixel(x, y, image::Rgba([200, 10, 10, 255]));
            }
        }
        let fitted = fit_to_panel(&tall, FitMode::Cover);
        assert_eq!(fitted.dimensions(), (PANEL_WIDTH, PANEL_HEIGHT));
        assert_eq!(fitted.get_pixel(240, 320), &Rgb([200, 10, 10]));
        assert_eq!(fitted.get_pixel(240, 0), &Rgb([0, 0, 0]));
    }

    #[test]
    fn an_empty_source_becomes_a_black_panel_instead_of_panicking() {
        let empty = RgbaImage::new(0, 0);
        assert_eq!(
            fit_to_panel(&empty, FitMode::Cover).dimensions(),
            (PANEL_WIDTH, PANEL_HEIGHT)
        );
    }

    #[test]
    fn fit_mode_spellings_are_explicit() {
        assert_eq!(FitMode::parse("cover"), Some(FitMode::Cover));
        assert_eq!(FitMode::parse(" Cover "), Some(FitMode::Cover));
        assert_eq!(FitMode::parse("CONTAIN"), Some(FitMode::Contain));
        assert_eq!(FitMode::parse("stretch"), None);
        assert_eq!(FitMode::default(), FitMode::Cover);
        assert_eq!(FitMode::Cover.label(), "cover");
        assert_eq!(FitMode::Contain.label(), "contain");
    }
}
