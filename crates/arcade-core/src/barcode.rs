//! Local QR/barcode creation and decoding through the upstream ZXing-C++ engine.
//!
//! Images are decoded through a size-limited reader, and generated files are
//! published only through Arcade's opaque, scoped output grants.

use crate::Arcade;
use arcade_contract::{ResultStatus, ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use image::{DynamicImage, GrayImage, ImageReader, Limits};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufReader, Cursor},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use zxingcpp::{BarcodeFormat, BarcodeFormats, FromStr as _, read, write};

const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 32_768;
const MAX_IMAGE_PIXELS: u64 = 32_000_000;
const MAX_BATCH_ITEMS: usize = 100;
const MAX_DECODED_SYMBOLS: usize = 128;

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    match manifest.id.as_str() {
        "arcade.barcode.qr-generate" => generate_one(manifest, request, runtime, cancelled, true),
        "arcade.barcode.barcode-generate" => {
            generate_one(manifest, request, runtime, cancelled, false)
        }
        "arcade.barcode.decode" => decode_one(manifest, request, runtime, cancelled),
        "arcade.barcode.batch" => generate_batch(manifest, request, runtime, cancelled),
        _ => Err(format!(
            "no barcode executor registered for {}",
            manifest.id
        )),
    }
}

fn generate_one(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    qr_only: bool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    let [input] = request.inputs.as_slice() else {
        return Err(format!("{} needs one text value", manifest.name));
    };
    if input.kind != ValueKind::Text
        || !input.mime.starts_with("text/") && input.mime != "structured/qr-payload"
    {
        return Err("Enter the content to encode as a QR code or barcode".into());
    }
    let payload = input.value.trim_end_matches(['\r', '\n']);
    if payload.trim().is_empty() {
        return Err("Enter some content to encode".into());
    }
    if payload.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "Barcode content must be {MAX_TEXT_BYTES} bytes or smaller"
        ));
    }

    let symbology = if qr_only {
        BarcodeFormat::QRCode
    } else {
        parse_symbology(option_str(request, "symbology", "Code128"))?
    };
    let format = output_format(option_str(request, "format", "png"))?;
    let scale = scale_option(request)?;
    let barcode = create_barcode(payload, symbology, request)?;
    let staging = staging_directory(runtime)?;
    let (staged_path, output_name) = render_barcode(
        &barcode,
        staging.path(),
        option_str(
            request,
            "outputName",
            if qr_only { "qr-code" } else { "barcode" },
        ),
        format,
        scale,
    )?;
    let selected = runtime
        .publish_staged_output(
            output_directory_token(request),
            &staged_path,
            &output_name,
            cancelled,
        )
        .map_err(|error| error.to_string())?;
    Ok(file_result(manifest, vec![selected]))
}

fn generate_batch(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    let [input] = request.inputs.as_slice() else {
        return Err("Paste one barcode value per line".into());
    };
    if input.kind != ValueKind::Text {
        return Err("Paste barcode values as text, one value per line".into());
    }
    let rows = parse_batch_rows(&input.value)?;
    if rows.is_empty() {
        return Err("Add at least one value to generate".into());
    }
    if rows.len() > MAX_BATCH_ITEMS {
        return Err(format!(
            "This batch supports up to {MAX_BATCH_ITEMS} values"
        ));
    }
    if rows.iter().any(|row| row.value.len() > MAX_TEXT_BYTES) {
        return Err(format!(
            "Each barcode value must be {MAX_TEXT_BYTES} bytes or smaller"
        ));
    }
    let symbology = parse_symbology(option_str(request, "symbology", "QRCode"))?;
    let format = output_format(option_str(request, "format", "png"))?;
    let scale = scale_option(request)?;
    let prefix = option_str(request, "prefix", "barcode");
    if prefix.len() > 160 || prefix.trim() != prefix || prefix.is_empty() {
        return Err("Choose a short filename prefix without surrounding spaces".into());
    }
    if prefix.contains(['/', '\\']) || prefix.chars().any(char::is_control) {
        return Err("Filename prefix cannot contain a path or control characters".into());
    }

    let staging = staging_directory(runtime)?;
    let mut files = Vec::with_capacity(rows.len() + 1);
    let mut label_images = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        check_cancelled(cancelled)?;
        let barcode = create_barcode(&row.value, symbology, request)?;
        let stem = format!("{prefix}-{:03}", index + 1);
        let (staged_path, output_name) =
            render_barcode(&barcode, staging.path(), &stem, format, scale)?;
        label_images.push((
            row.label.clone().unwrap_or_else(|| row.value.clone()),
            render_barcode_png(&barcode, scale)?,
        ));
        files.push(
            runtime
                .publish_staged_output(
                    output_directory_token(request),
                    &staged_path,
                    &output_name,
                    cancelled,
                )
                .map_err(|error| error.to_string())?,
        );
    }
    let sheet_path = staging.path().join("labels.pdf");
    let sheet_name = output_name(&format!("{prefix}-labels"), "pdf")?;
    write_label_sheet_pdf(&label_images, &sheet_path)?;
    files.push(
        runtime
            .publish_staged_output(
                output_directory_token(request),
                &sheet_path,
                &sheet_name,
                cancelled,
            )
            .map_err(|error| error.to_string())?,
    );
    let mut result = file_result(manifest, files);
    result.message = Some(format!(
        "Generated {} barcode images and a printable label sheet.",
        rows.len()
    ));
    result
        .metadata
        .insert("labelCount".into(), json!(rows.len()));
    result
        .metadata
        .insert("labelSheet".into(), json!(sheet_name));
    Ok(result)
}

#[derive(Debug, Clone)]
struct BatchRow {
    label: Option<String>,
    value: String,
}

fn parse_batch_rows(input: &str) -> Result<Vec<BatchRow>, String> {
    let delimiter = if input.lines().any(|line| line.contains('\t')) {
        b'\t'
    } else {
        b','
    };
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .delimiter(delimiter)
        .from_reader(input.as_bytes());
    let mut records = reader
        .records()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Could not read barcode rows: {error}"))?
        .into_iter()
        .filter(|record| record.iter().any(|field| !field.trim().is_empty()))
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Ok(Vec::new());
    }

    let first = records[0]
        .iter()
        .map(|field| field.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    let value_header = first.iter().position(|field| {
        matches!(
            field.as_str(),
            "value" | "data" | "content" | "code" | "barcode"
        )
    });
    let label_header = first
        .iter()
        .position(|field| field == "label" || field == "name");
    let has_header = value_header.is_some();
    let value_column = value_header.unwrap_or(if records[0].len() == 1 { 0 } else { 1 });
    let label_column = label_header
        .filter(|column| *column != value_column)
        .or_else(|| (!has_header && records[0].len() == 2).then_some(0));
    if has_header {
        records.remove(0);
    }

    let mut rows = Vec::with_capacity(records.len());
    for (index, record) in records.into_iter().enumerate() {
        if record.len() == 1 {
            let value = record[0].trim();
            if !value.is_empty() {
                rows.push(BatchRow {
                    label: None,
                    value: value.to_owned(),
                });
            }
            continue;
        }
        if record.len() > 32 || value_column >= record.len() {
            return Err(format!(
                "Barcode row {} does not contain the selected value column",
                index + 1
            ));
        }
        let value = record[value_column].trim();
        if value.is_empty() {
            return Err(format!(
                "Barcode row {} has an empty barcode value",
                index + 1
            ));
        }
        let label = label_column
            .and_then(|column| record.get(column))
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_owned);
        rows.push(BatchRow {
            label,
            value: value.to_owned(),
        });
    }
    Ok(rows)
}

fn render_barcode_png(barcode: &zxingcpp::Barcode, scale: u32) -> Result<Vec<u8>, String> {
    let rendered = barcode
        .to_image_with(&write().scale(scale as i32).add_quiet_zones(true))
        .map_err(|error| format!("ZXing could not render a label image: {error}"))?;
    let image = GrayImage::from(&rendered);
    let dynamic = DynamicImage::ImageLuma8(image);
    let mut bytes = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| format!("Could not prepare a label image: {error}"))?;
    Ok(bytes.into_inner())
}

fn write_label_sheet_pdf(labels: &[(String, Vec<u8>)], path: &Path) -> Result<(), String> {
    use printpdf::{
        BuiltinFont, Mm, Op, PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt,
        RawImage, TextItem, XObjectTransform,
    };

    const PAGE_WIDTH_MM: f32 = 210.0;
    const PAGE_HEIGHT_MM: f32 = 297.0;
    const MARGIN_MM: f32 = 8.0;
    const COLUMNS: usize = 3;
    const ROWS_PER_PAGE: usize = 6;
    const CELL_WIDTH_MM: f32 = (PAGE_WIDTH_MM - 2.0 * MARGIN_MM) / COLUMNS as f32;
    const CELL_HEIGHT_MM: f32 = 46.0;
    const IMAGE_MAX_WIDTH_MM: f32 = 48.0;
    const IMAGE_MAX_HEIGHT_MM: f32 = 28.0;

    let mut document = PdfDocument::new("Arcade Box Barcode Labels");
    let mut pages = Vec::new();
    for page_labels in labels.chunks(COLUMNS * ROWS_PER_PAGE) {
        let mut operations = Vec::with_capacity(page_labels.len() * 7);
        for (item_index, (label, image_bytes)) in page_labels.iter().enumerate() {
            let raw_image = RawImage::decode_from_bytes(image_bytes, &mut Vec::new())
                .map_err(|error| format!("Could not embed barcode in printable labels: {error}"))?;
            let image_id = document.add_image(&raw_image);
            let row = item_index / COLUMNS;
            let column = item_index % COLUMNS;
            let cell_left_mm = MARGIN_MM + column as f32 * CELL_WIDTH_MM;
            let cell_top_mm = MARGIN_MM + row as f32 * CELL_HEIGHT_MM;
            let image_width_mm = raw_image.width as f32 * 25.4 / 300.0;
            let image_height_mm = raw_image.height as f32 * 25.4 / 300.0;
            let scale = (IMAGE_MAX_WIDTH_MM / image_width_mm)
                .min(IMAGE_MAX_HEIGHT_MM / image_height_mm)
                .min(1.0);
            let rendered_width_mm = image_width_mm * scale;
            let rendered_height_mm = image_height_mm * scale;
            let image_left_mm = cell_left_mm + (CELL_WIDTH_MM - rendered_width_mm) / 2.0;
            let image_bottom_mm = PAGE_HEIGHT_MM - cell_top_mm - rendered_height_mm - 6.0;
            operations.push(Op::UseXobject {
                id: image_id,
                transform: XObjectTransform {
                    translate_x: Some(Pt::from(Mm(image_left_mm))),
                    translate_y: Some(Pt::from(Mm(image_bottom_mm))),
                    dpi: Some(300.0),
                    scale_x: Some(scale),
                    scale_y: Some(scale),
                    ..Default::default()
                },
            });
            operations.extend([
                Op::StartTextSection,
                Op::SetFont {
                    font: PdfFontHandle::Builtin(BuiltinFont::HelveticaBold),
                    size: Pt(9.0),
                },
                Op::SetTextCursor {
                    pos: Point::new(
                        Mm(cell_left_mm + 2.0),
                        Mm(PAGE_HEIGHT_MM - cell_top_mm - 5.0),
                    ),
                },
                Op::ShowText {
                    items: vec![TextItem::Text(printable_label_text(label, 54))],
                },
                Op::EndTextSection,
            ]);
        }
        pages.push(PdfPage::new(
            Mm(PAGE_WIDTH_MM),
            Mm(PAGE_HEIGHT_MM),
            operations,
        ));
    }
    let bytes = document
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    fs::write(path, bytes)
        .map_err(|error| format!("Could not save printable barcode labels: {error}"))
}

fn printable_label_text(label: &str, max_chars: usize) -> String {
    let mut output = label
        .chars()
        .map(|character| {
            if character.is_ascii_graphic() || character == ' ' {
                character
            } else {
                '?'
            }
        })
        .take(max_chars)
        .collect::<String>();
    if label.chars().count() > max_chars {
        output.truncate(max_chars.saturating_sub(1));
        output.push('…');
    }
    output
}

fn decode_one(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    let [input] = request.inputs.as_slice() else {
        return Err("Select one image containing a QR code or barcode".into());
    };
    if input.kind != ValueKind::Artifact || input.mime != "file/image" {
        return Err("Select an image file through Arcade Box".into());
    }
    let image = load_bounded_image(runtime, &input.value)?;
    check_cancelled(cancelled)?;
    let grayscale = image.to_luma8();
    let formats = BarcodeFormats::list(BarcodeFormat::AllReadable);
    let barcodes = read()
        .formats(formats)
        .try_harder(true)
        .try_rotate(true)
        .try_downscale(true)
        .max_number_of_symbols(MAX_DECODED_SYMBOLS as i32)
        .from(&grayscale)
        .map_err(|error| format!("ZXing could not inspect this image: {error}"))?;
    check_cancelled(cancelled)?;

    let results = barcodes
        .into_iter()
        .filter(|barcode| barcode.is_valid())
        .take(MAX_DECODED_SYMBOLS)
        .map(|barcode| {
            let position = barcode.position();
            json!({
                "format": barcode.format().to_string(),
                "contentType": barcode.content_type().to_string(),
                "text": barcode.text(),
                "position": {
                    "topLeft": {"x": position.top_left.x, "y": position.top_left.y},
                    "topRight": {"x": position.top_right.x, "y": position.top_right.y},
                    "bottomRight": {"x": position.bottom_right.x, "y": position.bottom_right.y},
                    "bottomLeft": {"x": position.bottom_left.x, "y": position.bottom_left.y}
                }
            })
        })
        .collect::<Vec<_>>();
    let decoded_count = results.len();
    let value = serde_json::to_string_pretty(&json!({
        "count": decoded_count,
        "results": results,
        "message": if results.is_empty() { Some("No readable barcode was found in this image.") } else { None }
    }))
    .map_err(|error| error.to_string())?;
    Ok(ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![ToolValue::text(value, "structured/barcode")],
        message: None,
        warnings: vec![],
        metadata: BTreeMap::from([
            ("provider".into(), json!("ZXing-C++")),
            ("decodedCount".into(), json!(decoded_count)),
        ]),
    })
}

fn load_bounded_image(runtime: &Arcade, token: &str) -> Result<DynamicImage, String> {
    let file = runtime
        .grants()
        .open_scoped(token)
        .map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "Selected image is larger than {} MB; choose a smaller image",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| format!("Cannot identify the selected image: {error}"))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| format!("Cannot read image dimensions: {error}"))?;
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || (width as u64).saturating_mul(height as u64) > MAX_IMAGE_PIXELS
    {
        return Err("This image is too large to scan safely. Resize it and try again.".into());
    }

    let file = runtime
        .grants()
        .open_scoped(token)
        .map_err(|error| error.to_string())?;
    let mut reader = ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .map_err(|error| format!("Cannot identify the selected image: {error}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|error| format!("Cannot decode the selected image: {error}"))
}

fn create_barcode(
    payload: &str,
    format: BarcodeFormat,
    request: &ToolRequest,
) -> Result<zxingcpp::Barcode, String> {
    let options = if format == BarcodeFormat::QRCode {
        let level = match option_str(request, "errorCorrection", "medium") {
            "low" => "L",
            "medium" => "M",
            "quartile" => "Q",
            "high" => "H",
            value => return Err(format!("Unknown QR error-correction level: {value}")),
        };
        format!("ec_level:{level}")
    } else {
        String::new()
    };
    zxingcpp::create(format)
        .options(options)
        .from_str(payload)
        .map_err(|error| format!("ZXing could not encode this value: {error}"))
}

fn render_barcode(
    barcode: &zxingcpp::Barcode,
    staging: &Path,
    requested_stem: &str,
    format: OutputFormat,
    scale: u32,
) -> Result<(PathBuf, String), String> {
    let output_name = output_name(requested_stem, format.extension())?;
    let staged_path = staging.join(match format {
        OutputFormat::Png => "barcode.png",
        OutputFormat::Svg => "barcode.svg",
    });
    let writer = write().scale(scale as i32).add_quiet_zones(true);
    match format {
        OutputFormat::Png => {
            let rendered = barcode
                .to_image_with(&writer)
                .map_err(|error| format!("ZXing could not render a PNG: {error}"))?;
            let image = GrayImage::from(&rendered);
            image
                .save_with_format(&staged_path, image::ImageFormat::Png)
                .map_err(|error| format!("Could not save the generated PNG: {error}"))?;
        }
        OutputFormat::Svg => {
            let svg = barcode
                .to_svg_with(&writer)
                .map_err(|error| format!("ZXing could not render an SVG: {error}"))?;
            fs::write(&staged_path, svg.as_bytes())
                .map_err(|error| format!("Could not save the generated SVG: {error}"))?;
        }
    }
    Ok((staged_path, output_name))
}

#[derive(Clone, Copy)]
enum OutputFormat {
    Png,
    Svg,
}

impl OutputFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }
}

fn output_format(value: &str) -> Result<OutputFormat, String> {
    match value {
        "png" => Ok(OutputFormat::Png),
        "svg" => Ok(OutputFormat::Svg),
        _ => Err("Choose PNG or SVG output".into()),
    }
}

fn parse_symbology(value: &str) -> Result<BarcodeFormat, String> {
    let normalized = match value.to_ascii_lowercase().as_str() {
        "code128" | "code-128" => "Code128",
        "code39" | "code-39" => "Code39",
        "ean13" | "ean-13" => "EAN13",
        "ean8" | "ean-8" => "EAN8",
        "upca" | "upc-a" | "upc" => "UPCA",
        "upce" | "upc-e" => "UPCE",
        "datamatrix" | "data-matrix" => "DataMatrix",
        "pdf417" | "pdf-417" => "PDF417",
        "aztec" => "Aztec",
        "qr" | "qrcode" | "qr-code" => "QRCode",
        _ => return Err(format!("Unsupported barcode format: {value}")),
    };
    let format = BarcodeFormat::from_str(normalized)
        .map_err(|error| format!("ZXing does not support {value}: {error}"))?;
    if !BarcodeFormats::list(BarcodeFormat::AllCreatable).contains(format) {
        return Err(format!("ZXing cannot generate {value} on this build"));
    }
    Ok(format)
}

fn scale_option(request: &ToolRequest) -> Result<u32, String> {
    let scale = request
        .options
        .get("scale")
        .and_then(Value::as_u64)
        .unwrap_or(4);
    if !(2..=12).contains(&scale) {
        return Err("Image scale must be between 2 and 12".into());
    }
    Ok(scale as u32)
}

fn output_name(stem: &str, extension: &str) -> Result<String, String> {
    let stem = stem.trim();
    if stem.is_empty()
        || stem.len() > 200
        || stem.contains(['/', '\\'])
        || stem.chars().any(char::is_control)
    {
        return Err("Choose a short output filename without a path".into());
    }
    let stem = stem.strip_suffix(&format!(".{extension}")).unwrap_or(stem);
    if stem.is_empty() {
        return Err("Output filename needs a name before its extension".into());
    }
    Ok(format!("{stem}.{extension}"))
}

fn output_directory_token(request: &ToolRequest) -> Option<&str> {
    request
        .options
        .get("destinationGrant")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
}

fn option_str<'a>(request: &'a ToolRequest, key: &str, default: &'a str) -> &'a str {
    request
        .options
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
}

fn staging_directory(runtime: &Arcade) -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("arcade-barcode-")
        .tempdir_in(runtime.artifact_staging_root())
        .map_err(|error| format!("Could not create a private barcode workspace: {error}"))
}

fn file_result(manifest: &ToolManifest, files: Vec<crate::grants::SelectedFile>) -> ToolResult {
    let output_names = files
        .iter()
        .map(|file| file.name.clone())
        .collect::<Vec<_>>();
    let output_bytes = files.iter().map(|file| file.size).collect::<Vec<_>>();
    let outputs = files
        .iter()
        .map(crate::grants::SelectedFile::as_tool_value)
        .collect::<Vec<_>>();
    ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs,
        message: None,
        warnings: vec![],
        metadata: BTreeMap::from([
            ("provider".into(), json!("ZXing-C++")),
            ("outputNames".into(), json!(output_names)),
            ("outputBytes".into(), json!(output_bytes)),
        ]),
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Barcode operation cancelled".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zxingcpp::{create, read, write};

    #[test]
    fn generated_qr_round_trips_through_the_decoder() {
        let text = "https://arcadebox.example/test?x=1";
        let barcode = create(BarcodeFormat::QRCode)
            .options("ec_level:Q")
            .from_str(text)
            .unwrap();
        let rendered = barcode.to_image_with(&write().scale(6)).unwrap();
        let image = GrayImage::from(&rendered);
        let decoded = read()
            .formats(BarcodeFormat::QRCode)
            .try_harder(true)
            .from(&image)
            .unwrap();
        assert_eq!(decoded.len(), 1);
        assert!(decoded[0].is_valid());
        assert_eq!(decoded[0].text(), text);
    }

    #[test]
    fn rejects_unsafe_output_names_and_unbounded_scale() {
        assert!(output_name("../secret", "png").is_err());
        assert_eq!(output_name("code", "png").unwrap(), "code.png");
        let request = ToolRequest {
            tool_id: "arcade.barcode.qr-generate".into(),
            inputs: vec![],
            options: json!({"scale": 1000}),
        };
        assert!(scale_option(&request).is_err());
    }
}
