//! Small, bounded image previews for opaque artifact grants.
//!
//! Image bytes stay on the native side for clipboard writes. The only image
//! data sent to the webview is a 512px PNG preview, never a filesystem path.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageReader, Limits};
use std::{
    io::{Cursor, Read},
    sync::{Arc, LazyLock},
};
use tauri::State;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio::sync::Semaphore;

const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMAGE_EDGE: u32 = 8_192;
const MAX_IMAGE_PIXELS: u64 = 16_000_000;
const MAX_DECODE_BYTES: u64 = 96 * 1024 * 1024;
const MAX_SVG_SHAPES: usize = 1_000_000;
const PREVIEW_EDGE: u32 = 512;
const SVG_SCALE: u32 = 8;
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const ZINT_SVG_DOCTYPE: &str = "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">";
static IMAGE_WORK: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResultPreview {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

#[tauri::command]
pub async fn image_result_preview(
    token: String,
    runtime: State<'_, std::sync::Arc<arcade_core::Arcade>>,
) -> Result<ImageResultPreview, String> {
    let runtime = runtime.inner().clone();
    let permit = IMAGE_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| format!("Image preview service is unavailable: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let bytes = read_granted_artifact(runtime.grants(), &token)?;
        let image = decode_artifact_image(&bytes)?;
        let width = image.width();
        let height = image.height();
        let thumbnail = image.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE);
        let mut encoded = Cursor::new(Vec::new());
        thumbnail
            .write_to(&mut encoded, image::ImageFormat::Png)
            .map_err(|error| format!("Could not encode the image preview: {error}"))?;
        Ok(ImageResultPreview {
            data_url: format!(
                "data:image/png;base64,{}",
                STANDARD.encode(encoded.into_inner())
            ),
            width,
            height,
        })
    })
    .await
    .map_err(|error| format!("Image preview task failed: {error}"))?
}

#[tauri::command]
pub async fn copy_image_result(
    token: String,
    app: tauri::AppHandle,
    runtime: State<'_, std::sync::Arc<arcade_core::Arcade>>,
) -> Result<(), String> {
    let runtime = runtime.inner().clone();
    let permit = IMAGE_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| format!("Image copy service is unavailable: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let bytes = read_granted_artifact(runtime.grants(), &token)?;
        let image = decode_artifact_image(&bytes)?;
        let rgba = image.into_rgba8();
        let width = rgba.width();
        let height = rgba.height();
        let clipboard_image = tauri::image::Image::new_owned(rgba.into_raw(), width, height);
        app.clipboard()
            .write_image(&clipboard_image)
            .map_err(|error| format!("Could not copy the image to the clipboard: {error}"))
    })
    .await
    .map_err(|error| format!("Image copy task failed: {error}"))?
}

fn read_granted_artifact(
    grants: &arcade_core::grants::FileGrants,
    token: &str,
) -> Result<Vec<u8>, String> {
    if grants.verify_type(token, "file/image").is_err()
        && grants.verify_type(token, "file/svg").is_err()
    {
        return Err("This result is not a supported image artifact".into());
    }
    let file = grants
        .open_scoped(token)
        .map_err(|error| format!("Could not open the image artifact: {error}"))?;
    let size = file
        .metadata()
        .map_err(|error| format!("Could not inspect the image artifact: {error}"))?
        .len();
    if size == 0 || size > MAX_SOURCE_BYTES {
        return Err("This image is empty or exceeds the 32 MB preview limit".into());
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read the image artifact: {error}"))?;
    if bytes.len() as u64 != size || bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err("The image artifact changed while it was being read".into());
    }
    Ok(bytes)
}

fn decode_artifact_image(bytes: &[u8]) -> Result<DynamicImage, String> {
    if looks_like_svg(bytes) {
        return rasterize_qr_svg(bytes);
    }

    let dimension_reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("Cannot identify this image: {error}"))?;
    let (width, height) = dimension_reader
        .into_dimensions()
        .map_err(|error| format!("Cannot read image dimensions: {error}"))?;
    check_dimensions(width, height)?;

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("Cannot identify this image: {error}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("Cannot safely decode this image: {error}"))?;
    check_dimensions(image.width(), image.height())?;
    Ok(image)
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let header = &bytes[start..bytes.len().min(start.saturating_add(512))];
    let header = String::from_utf8_lossy(header).to_ascii_lowercase();
    header.starts_with("<svg") || header.starts_with("<?xml") && header.contains("<svg")
}

/// Rasterize the deliberately small SVG subset emitted by ZXing's QR writer.
/// We accept a path made from axis-aligned rectangles and a single plain
/// background rectangle. SVG styling, links, and active content are rejected.
fn rasterize_qr_svg(bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err("This SVG exceeds the 2 MB preview limit".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "The SVG image is not valid UTF-8")?;
    let safe_text = strip_known_svg_doctype(text)?;
    let document = roxmltree::Document::parse(&safe_text)
        .map_err(|error| format!("Cannot safely read this SVG image: {error}"))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some(SVG_NAMESPACE) {
        return Err("This SVG is not a supported QR image".into());
    }
    for attribute in root.attributes() {
        let allowed = match attribute.name() {
            "version" => attribute.value() == "1.1",
            "viewBox" | "width" | "height" => true,
            "stroke" => attribute.value() == "none",
            _ => false,
        };
        if !allowed {
            return Err("This SVG contains unsupported drawing attributes".into());
        }
    }
    let is_viewbox_grid = root.attribute("viewBox").is_some();
    let (width, height) = if let Some(view_box) = root.attribute("viewBox") {
        parse_view_box(view_box)?
    } else {
        (
            parse_dimension(root.attribute("width"))?,
            parse_dimension(root.attribute("height"))?,
        )
    };
    check_dimensions(width, height)?;
    if root
        .attribute("width")
        .is_some_and(|value| parse_dimension(Some(value)).ok() != Some(width))
        || root
            .attribute("height")
            .is_some_and(|value| parse_dimension(Some(value)).ok() != Some(height))
    {
        return Err("This SVG uses inconsistent image dimensions".into());
    }

    let mut shapes = Vec::new();
    let mut has_background = false;
    let mut has_zint_description = false;
    let mut has_group = false;
    check_element_text(root, true)?;
    for child in root.children().filter(|node| node.is_element()) {
        if child.tag_name().namespace() != Some(SVG_NAMESPACE) {
            return Err("This SVG contains unsupported drawing elements".into());
        }
        match child.tag_name().name() {
            "desc" if child.text() == Some("Zint Generated Symbol") => {
                if !child.attributes().next().is_none() || has_zint_description {
                    return Err("This SVG contains unsupported description data".into());
                }
                check_element_text(child, false)?;
                has_zint_description = true;
            }
            "path" => parse_svg_path_node(child, width, height, &mut shapes)?,
            "rect" => parse_svg_rect_node(child, width, height, &mut shapes, &mut has_background)?,
            "g" => {
                if has_group || !is_zint_barcode_group(child) {
                    return Err("This SVG contains unsupported drawing groups".into());
                }
                check_element_text(child, true)?;
                has_group = true;
                for shape in child.children().filter(|node| node.is_element()) {
                    if shape.tag_name().namespace() != Some(SVG_NAMESPACE) {
                        return Err("This SVG contains unsupported drawing elements".into());
                    }
                    match shape.tag_name().name() {
                        "path" => parse_svg_path_node(shape, width, height, &mut shapes)?,
                        "rect" => parse_svg_rect_node(
                            shape,
                            width,
                            height,
                            &mut shapes,
                            &mut has_background,
                        )?,
                        _ => return Err("This SVG contains unsupported drawing elements".into()),
                    }
                }
            }
            _ => return Err("This SVG contains unsupported drawing elements".into()),
        }
    }
    if shapes.is_empty() {
        return Err("This SVG does not contain QR modules".into());
    }
    let max_painted_pixels = (width as u64) * (height as u64);
    let painted_pixels = shapes.iter().try_fold(0u64, |total, shape| {
        total.checked_add((shape.width as u64) * (shape.height as u64))
    });
    if painted_pixels.is_none_or(|count| count > max_painted_pixels) {
        return Err("This SVG contains too much drawing data to preview".into());
    }

    let scale = if is_viewbox_grid {
        SVG_SCALE.min((PREVIEW_EDGE * 4 / width.max(height)).max(1))
    } else {
        1
    };
    let output_width = width
        .checked_mul(scale)
        .ok_or("The QR image is too large to preview")?;
    let output_height = height
        .checked_mul(scale)
        .ok_or("The QR image is too large to preview")?;
    let mut rgba = vec![255; rgba_len(output_width, output_height)?];
    for shape in shapes {
        let left = shape.x * scale;
        let top = shape.y * scale;
        let right = left + shape.width * scale;
        let bottom = top + shape.height * scale;
        for row in top..bottom {
            for column in left..right {
                let offset = ((row * output_width + column) * 4) as usize;
                rgba[offset] = 0;
                rgba[offset + 1] = 0;
                rgba[offset + 2] = 0;
                rgba[offset + 3] = 255;
            }
        }
    }
    image::RgbaImage::from_raw(output_width, output_height, rgba)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "Could not prepare the QR image preview".into())
}

fn parse_view_box(value: &str) -> Result<(u32, u32), String> {
    let parts = value
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "0" || parts[1] != "0" {
        return Err("This SVG uses an unsupported QR coordinate system".into());
    }
    let width = parts[2]
        .parse::<u32>()
        .map_err(|_| "This SVG has an invalid QR width")?;
    let height = parts[3]
        .parse::<u32>()
        .map_err(|_| "This SVG has an invalid QR height")?;
    check_dimensions(width, height)?;
    Ok((width, height))
}

#[derive(Clone, Copy)]
struct SvgRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn strip_known_svg_doctype(text: &str) -> Result<String, String> {
    if text.contains("<!ENTITY") {
        return Err("SVG entity declarations are not supported for preview".into());
    }
    let Some(start) = text.find("<!DOCTYPE") else {
        return Ok(text.to_owned());
    };
    let end = text[start..]
        .find('>')
        .map(|offset| start + offset + 1)
        .ok_or("The SVG document declaration is incomplete")?;
    if &text[start..end] != ZINT_SVG_DOCTYPE {
        return Err("This SVG uses an unsupported document declaration".into());
    }
    Ok(format!("{}{}", &text[..start], &text[end..]))
}

fn parse_dimension(value: Option<&str>) -> Result<u32, String> {
    let dimension = parse_svg_uint(value, "This SVG does not include a previewable QR size")?;
    if dimension == 0 || dimension > MAX_IMAGE_EDGE {
        return Err("This SVG is too large to preview".into());
    }
    Ok(dimension)
}

fn parse_svg_uint(value: Option<&str>, missing: &'static str) -> Result<u32, String> {
    value
        .ok_or(missing)?
        .parse::<u32>()
        .map_err(|_| "This SVG has an invalid image dimension".into())
}

fn is_zint_barcode_group(node: roxmltree::Node<'_, '_>) -> bool {
    node.attributes().all(|attribute| match attribute.name() {
        "id" => attribute.value() == "barcode",
        "fill" => attribute.value() == "#000000",
        _ => false,
    }) && node.attribute("id") == Some("barcode")
        && node.attribute("fill") == Some("#000000")
}

fn check_element_text(node: roxmltree::Node<'_, '_>, whitespace_only: bool) -> Result<(), String> {
    for child in node.children() {
        if whitespace_only
            && child.is_text()
            && child.text().is_some_and(|text| !text.trim().is_empty())
        {
            return Err("This SVG contains unsupported text content".into());
        }
        if !whitespace_only && child.is_element() {
            return Err("This SVG contains unsupported text content".into());
        }
        if child.is_pi() {
            return Err("This SVG contains unsupported processing instructions".into());
        }
    }
    Ok(())
}

fn parse_svg_path_node(
    node: roxmltree::Node<'_, '_>,
    width: u32,
    height: u32,
    shapes: &mut Vec<SvgRect>,
) -> Result<(), String> {
    check_element_text(node, true)?;
    for attribute in node.attributes() {
        let allowed =
            attribute.name() == "d" || attribute.name() == "fill" && attribute.value() == "#000000";
        if !allowed {
            return Err("This SVG path contains unsupported drawing attributes".into());
        }
    }
    let path = node
        .attribute("d")
        .ok_or("This SVG path has no drawing data")?;
    shapes.extend(parse_qr_path(path, width, height)?);
    Ok(())
}

fn parse_svg_rect_node(
    node: roxmltree::Node<'_, '_>,
    width: u32,
    height: u32,
    shapes: &mut Vec<SvgRect>,
    has_background: &mut bool,
) -> Result<(), String> {
    check_element_text(node, true)?;
    for attribute in node.attributes() {
        if !matches!(attribute.name(), "x" | "y" | "width" | "height" | "fill") {
            return Err("This SVG rectangle contains unsupported drawing attributes".into());
        }
    }
    let rect = SvgRect {
        x: parse_svg_uint(node.attribute("x"), "This SVG rectangle has no x position")?,
        y: parse_svg_uint(node.attribute("y"), "This SVG rectangle has no y position")?,
        width: parse_dimension(node.attribute("width"))?,
        height: parse_dimension(node.attribute("height"))?,
    };
    if rect.x.saturating_add(rect.width) > width || rect.y.saturating_add(rect.height) > height {
        return Err("This SVG draws outside its declared image dimensions".into());
    }
    match node.attribute("fill") {
        Some("#FFFFFF" | "#ffffff" | "white") => {
            if rect.x != 0
                || rect.y != 0
                || rect.width != width
                || rect.height != height
                || *has_background
            {
                return Err("This SVG contains an unsupported background rectangle".into());
            }
            *has_background = true;
        }
        Some("#000000" | "#000" | "black") => shapes.push(rect),
        _ => return Err("This SVG uses an unsupported rectangle color".into()),
    }
    Ok(())
}

fn parse_qr_path(value: &str, width: u32, height: u32) -> Result<Vec<SvgRect>, String> {
    if value.len() > MAX_SVG_BYTES {
        return Err("This SVG path is too large to preview".into());
    }
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut shapes = Vec::new();
    while index < bytes.len() {
        skip_svg_spaces(bytes, &mut index);
        if index == bytes.len() {
            break;
        }
        expect_byte(bytes, &mut index, b'M')?;
        let x = parse_unsigned(bytes, &mut index)?;
        expect_number_separator(bytes, &mut index)?;
        let y = parse_unsigned(bytes, &mut index)?;
        expect_byte(bytes, &mut index, b'h')?;
        let dx = parse_signed(bytes, &mut index)?;
        expect_byte(bytes, &mut index, b'v')?;
        let dy = parse_signed(bytes, &mut index)?;
        expect_byte(bytes, &mut index, b'h')?;
        let close_dx = parse_signed(bytes, &mut index)?;
        let close = match bytes.get(index) {
            Some(b'z' | b'Z') => {
                index += 1;
                true
            }
            _ => false,
        };
        if dx <= 0 || dy <= 0 || close_dx != -dx || !close {
            return Err("This SVG uses an unsupported QR path".into());
        }
        let rect_width = dx as u32;
        let rect_height = dy as u32;
        if x.saturating_add(rect_width) > width || y.saturating_add(rect_height) > height {
            return Err("This SVG draws outside its declared QR size".into());
        }
        shapes.push(SvgRect {
            x,
            y,
            width: rect_width,
            height: rect_height,
        });
        if shapes.len() > MAX_SVG_SHAPES {
            return Err("This SVG contains too many QR modules to preview".into());
        }
    }
    if shapes.is_empty() {
        return Err("This SVG does not contain QR modules".into());
    }
    Ok(shapes)
}

fn expect_number_separator(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    let before = *index;
    if bytes.get(*index) == Some(&b',') {
        *index += 1;
    }
    skip_svg_spaces(bytes, index);
    if before == *index {
        return Err("This SVG uses an unsupported QR path".into());
    }
    Ok(())
}

fn skip_svg_spaces(bytes: &[u8], index: &mut usize) {
    while bytes.get(*index).is_some_and(u8::is_ascii_whitespace) {
        *index += 1;
    }
}

fn expect_byte(bytes: &[u8], index: &mut usize, expected: u8) -> Result<(), String> {
    if bytes.get(*index) != Some(&expected) {
        return Err("This SVG uses an unsupported QR path".into());
    }
    *index += 1;
    Ok(())
}

fn parse_unsigned(bytes: &[u8], index: &mut usize) -> Result<u32, String> {
    let start = *index;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    if start == *index {
        return Err("This SVG uses an unsupported QR path".into());
    }
    std::str::from_utf8(&bytes[start..*index])
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "This SVG uses an unsupported QR path".into())
}

fn parse_signed(bytes: &[u8], index: &mut usize) -> Result<i32, String> {
    let start = *index;
    if matches!(bytes.get(*index), Some(b'-' | b'+')) {
        *index += 1;
    }
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    if start == *index {
        return Err("This SVG uses an unsupported QR path".into());
    }
    std::str::from_utf8(&bytes[start..*index])
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "This SVG uses an unsupported QR path".into())
}

fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_IMAGE_EDGE
        || height > MAX_IMAGE_EDGE
        || (width as u64).saturating_mul(height as u64) > MAX_IMAGE_PIXELS
    {
        return Err("This image is too large to preview or copy safely".into());
    }
    Ok(())
}

fn rgba_len(width: u32, height: u32) -> Result<usize, String> {
    check_dimensions(width, height)?;
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "The image is too large to copy safely".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_contract::{ToolRequest, ToolValue};
    use arcade_core::Arcade;
    use std::io::Write;

    fn runtime() -> Arcade {
        Arcade::in_memory().unwrap()
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([10, 20, 30, 255]),
        ));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn svg(path: &str) -> String {
        format!(
            "<svg xmlns=\"{SVG_NAMESPACE}\" version=\"1.1\" viewBox=\"0 0 2 2\" stroke=\"none\"><path d=\"{path}\"/></svg>"
        )
    }

    #[test]
    fn rasterizes_only_the_qr_svg_subset() {
        let image = rasterize_qr_svg(svg("M0,0h1v1h-1zM1,1h1v1h-1z").as_bytes()).unwrap();
        assert_eq!((image.width(), image.height()), (16, 16));
        let rgba = image.to_rgba8();
        assert_eq!(rgba.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(rgba.get_pixel(8, 8).0, [0, 0, 0, 255]);
        assert_eq!(rgba.get_pixel(8, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn rejects_active_or_unrecognized_svg_content() {
        assert!(
            rasterize_qr_svg(
                format!("<svg xmlns=\"{SVG_NAMESPACE}\" viewBox=\"0 0 2 2\"><script/></svg>")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            rasterize_qr_svg(
                svg("M0,0h1v1h-1z")
                    .replace("<path", "<path onclick=\"alert(1)\"")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(rasterize_qr_svg(svg("M0,0h2v1h-1z").as_bytes()).is_err());
    }

    #[test]
    fn raster_decoding_and_dimensions_are_bounded_before_decode() {
        let image = decode_artifact_image(&png_bytes(8, 4)).unwrap();
        assert_eq!((image.width(), image.height()), (8, 4));
        assert!(check_dimensions(MAX_IMAGE_EDGE + 1, 1).is_err());
        assert!(check_dimensions(4_097, 4_097).is_err());
    }

    #[test]
    fn rejects_invalid_and_revoked_artifact_grants() {
        let runtime = runtime();
        assert!(read_granted_artifact(runtime.grants(), "not-a-grant").is_err());

        let mut file = tempfile::NamedTempFile::new_in(runtime.artifact_staging_root()).unwrap();
        file.write_all(&png_bytes(2, 2)).unwrap();
        file.flush().unwrap();
        let selected = runtime.grants().grant(file.path()).unwrap();
        runtime.grants().revoke(&selected.token);
        assert!(read_granted_artifact(runtime.grants(), &selected.token).is_err());
    }

    #[test]
    fn generated_qr_svg_preview_round_trips_through_barcode_decoder() {
        let runtime = runtime();
        let payload = "https://arcadebox.example/qr-preview?source=svg";
        let generated = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.barcode.qr-generate".into(),
                inputs: vec![ToolValue::text(payload, "text/plain")],
                options: serde_json::json!({"format":"svg", "scale":6}),
            })
            .unwrap();
        assert_eq!(generated.status, arcade_contract::ResultStatus::Success);
        assert_eq!(generated.outputs[0].mime, "file/svg");
        let svg_token = &generated.outputs[0].value;
        let svg = read_granted_artifact(runtime.grants(), svg_token).unwrap();
        let preview = decode_artifact_image(&svg).unwrap();
        assert!(preview.width() > 100);
        assert_eq!(preview.width(), preview.height());

        let mut png = tempfile::NamedTempFile::new_in(runtime.artifact_staging_root()).unwrap();
        preview.write_to(&mut png, image::ImageFormat::Png).unwrap();
        png.flush().unwrap();
        let preview_grant = runtime.grants().grant(png.path()).unwrap();
        let decoded = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.barcode.decode".into(),
                inputs: vec![preview_grant.as_tool_value()],
                options: serde_json::Value::Null,
            })
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(&decoded.outputs[0].value).unwrap();
        assert_eq!(data["results"][0]["text"], payload);
    }
}
