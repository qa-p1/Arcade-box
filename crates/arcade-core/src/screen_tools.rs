//! Narrow image helpers for trusted screen actions. Renderer requests contain
//! opaque file grants only; source paths and full-size pixels stay in core.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageReader, Limits, RgbaImage};
use serde::Serialize;
use std::io::{BufReader, Cursor};

const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DIMENSION: u32 = 32_768;
const MAX_PREVIEW_EDGE: u32 = 512;
const MAGNIFIER_SOURCE_EDGE: u32 = 11;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePreview {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampledPixel {
    pub x: u32,
    pub y: u32,
    pub rgba: [u8; 4],
    pub hex: String,
    pub rgb: [u8; 3],
    pub hsl: [f64; 3],
    pub magnifier_data_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenMeasurement {
    pub start: [u32; 2],
    pub end: [u32; 2],
    pub source_width: u32,
    pub source_height: u32,
    pub width_pixels: u32,
    pub height_pixels: u32,
    pub horizontal_distance: u32,
    pub vertical_distance: u32,
    pub diagonal_distance: f64,
}

pub fn create_image_preview(
    grants: &crate::grants::FileGrants,
    token: &str,
    requested_max_edge: u32,
) -> Result<ImagePreview, String> {
    let max_edge = requested_max_edge.clamp(1, MAX_PREVIEW_EDGE);
    let image = decode_granted_image(grants, token)?;
    let source_width = image.width();
    let source_height = image.height();
    if source_width == 0 || source_height == 0 {
        return Err("The selected image has no pixels to preview".into());
    }
    let thumbnail = image.thumbnail(max_edge, max_edge).to_rgba8();
    let data_url = png_data_url(&DynamicImage::ImageRgba8(thumbnail.clone()))?;
    Ok(ImagePreview {
        data_url,
        width: thumbnail.width(),
        height: thumbnail.height(),
        source_width,
        source_height,
    })
}

pub fn sample_image_pixel(
    grants: &crate::grants::FileGrants,
    token: &str,
    x: u32,
    y: u32,
) -> Result<SampledPixel, String> {
    let image = decode_granted_image(grants, token)?.to_rgba8();
    if x >= image.width() || y >= image.height() {
        return Err(format!(
            "Pixel coordinates must be within the image ({} × {})",
            image.width(),
            image.height()
        ));
    }
    let rgba = image.get_pixel(x, y).0;
    let rgb = [rgba[0], rgba[1], rgba[2]];
    let radius = (MAGNIFIER_SOURCE_EDGE / 2) as i64;
    let mut magnifier = RgbaImage::new(MAGNIFIER_SOURCE_EDGE, MAGNIFIER_SOURCE_EDGE);
    for patch_y in 0..MAGNIFIER_SOURCE_EDGE {
        for patch_x in 0..MAGNIFIER_SOURCE_EDGE {
            let source_x =
                (x as i64 + patch_x as i64 - radius).clamp(0, image.width() as i64 - 1) as u32;
            let source_y =
                (y as i64 + patch_y as i64 - radius).clamp(0, image.height() as i64 - 1) as u32;
            magnifier.put_pixel(patch_x, patch_y, *image.get_pixel(source_x, source_y));
        }
    }
    Ok(SampledPixel {
        x,
        y,
        rgba,
        hex: if rgba[3] == 255 {
            format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2], rgba[3])
        },
        rgb,
        hsl: rgb_to_hsl(rgb),
        magnifier_data_url: png_data_url(&DynamicImage::ImageRgba8(magnifier))?,
    })
}

pub fn measure_screen_points(
    grants: &crate::grants::FileGrants,
    token: &str,
    start: [u32; 2],
    end: [u32; 2],
) -> Result<ScreenMeasurement, String> {
    let (source_width, source_height) = image_dimensions(grants, token)?;
    for [x, y] in [start, end] {
        if x >= source_width || y >= source_height {
            return Err(format!(
                "Measurement points must be within the image ({} × {})",
                source_width, source_height
            ));
        }
    }
    let horizontal_distance = start[0].abs_diff(end[0]);
    let vertical_distance = start[1].abs_diff(end[1]);
    Ok(ScreenMeasurement {
        start,
        end,
        source_width,
        source_height,
        width_pixels: horizontal_distance.saturating_add(1),
        height_pixels: vertical_distance.saturating_add(1),
        horizontal_distance,
        vertical_distance,
        diagonal_distance: f64::from(horizontal_distance).hypot(f64::from(vertical_distance)),
    })
}

fn image_dimensions(grants: &crate::grants::FileGrants, token: &str) -> Result<(u32, u32), String> {
    let file = grants
        .open_scoped(token)
        .map_err(|error| format!("Could not open the selected screen image grant: {error}"))?;
    let size = file
        .metadata()
        .map_err(|error| format!("Could not inspect the selected screen image: {error}"))?
        .len();
    if size == 0 || size > MAX_SOURCE_BYTES {
        return Err("The selected image is empty or exceeds the 128 MB screen-image limit".into());
    }
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| format!("Could not identify the selected image: {error}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let dimensions = reader
        .into_dimensions()
        .map_err(|error| format!("Could not inspect selected screen dimensions: {error}"))?;
    if dimensions.0 == 0 || dimensions.1 == 0 {
        return Err("The selected image has no pixels to measure".into());
    }
    Ok(dimensions)
}

fn decode_granted_image(
    grants: &crate::grants::FileGrants,
    token: &str,
) -> Result<DynamicImage, String> {
    let file = grants
        .open_scoped(token)
        .map_err(|error| format!("Could not open the selected screen image grant: {error}"))?;
    let size = file
        .metadata()
        .map_err(|error| format!("Could not inspect the selected screen image: {error}"))?
        .len();
    if size == 0 || size > MAX_SOURCE_BYTES {
        return Err("The selected image is empty or exceeds the 128 MB screen-image limit".into());
    }
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| format!("Could not identify the selected image: {error}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|error| format!("Could not decode the selected screen image safely: {error}"))
}

fn png_data_url(image: &DynamicImage) -> Result<String, String> {
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| format!("Could not encode a screen-image preview: {error}"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(bytes.into_inner())
    ))
}

fn rgb_to_hsl(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|value| value as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let lightness = (max + min) / 2.0;
    if delta == 0.0 {
        return [0.0, 0.0, lightness * 100.0];
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    [hue, saturation * 100.0, lightness * 100.0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn previews_and_samples_only_a_valid_opaque_image_grant() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("pixels.png");
        let mut pixels = RgbaImage::new(4, 3);
        pixels.put_pixel(2, 1, Rgba([250, 20, 40, 128]));
        pixels.save(&path).unwrap();
        let grants = crate::grants::FileGrants::default();
        let selected = grants.grant(&path).unwrap();

        let preview = create_image_preview(&grants, &selected.token, 512).unwrap();
        assert_eq!((preview.source_width, preview.source_height), (4, 3));
        assert!(preview.data_url.starts_with("data:image/png;base64,"));

        let sample = sample_image_pixel(&grants, &selected.token, 2, 1).unwrap();
        assert_eq!(sample.rgba, [250, 20, 40, 128]);
        assert_eq!(sample.hex, "#FA142880");
        assert_eq!(sample.rgb, [250, 20, 40]);
        assert!(sample.hsl[0] > 0.0);
        assert!(
            sample
                .magnifier_data_url
                .starts_with("data:image/png;base64,")
        );
        assert!(sample_image_pixel(&grants, &selected.token, 4, 1).is_err());
        let measurement = measure_screen_points(&grants, &selected.token, [0, 0], [3, 2]).unwrap();
        assert_eq!(measurement.width_pixels, 4);
        assert_eq!(measurement.height_pixels, 3);
        assert_eq!(measurement.diagonal_distance, 13.0_f64.sqrt());
        assert!(measure_screen_points(&grants, &selected.token, [4, 0], [3, 2]).is_err());
        assert!(create_image_preview(&grants, "not-a-grant", 512).is_err());
    }
}
