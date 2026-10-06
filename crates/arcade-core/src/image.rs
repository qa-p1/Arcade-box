//! Image operations through a verified, system-first libvips provider.
//!
//! The selected file is linked into a private directory under a fixed name.
//! This also prevents libvips' filename-option syntax from interpreting a
//! user-controlled source filename as an operation argument.

use crate::{
    artifacts::{publish_without_overwrite, validate_portable_filename},
    grants::FileGrants,
    process::{self, ProcessSpec},
    provider::{
        ImageModelProvider, ProviderInfo, discover_background_removal_models,
        discover_upscale_models, discover_vips,
    },
};
use arcade_contract::{ResultStatus, ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

mod compose;
mod pii;
mod target_size;

const MAX_DIMENSION: u32 = 100_000;
const MAX_OUTPUT_PIXELS: u64 = 500_000_000;

#[derive(Clone, Copy, PartialEq)]
enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Tiff,
    /// Read-only sources: edits save them as JPEG (HEIF/AVIF) or PNG.
    Heif,
    Avif,
    Gif,
    Bmp,
}

impl ImageFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "png" => Ok(Self::Png),
            "jpeg" | "jpg" => Ok(Self::Jpeg),
            "webp" => Ok(Self::Webp),
            "tiff" | "tif" => Ok(Self::Tiff),
            _ => Err(format!("Unsupported output image format: {value}")),
        }
    }

    fn from_file(path: &Path) -> Result<Self, String> {
        let kind = infer::get_from_path(path)
            .map_err(|error| format!("Cannot inspect selected image: {error}"))?
            .ok_or("Selected file does not have a recognized image signature")?;
        match kind.mime_type() {
            "image/png" => Ok(Self::Png),
            "image/jpeg" => Ok(Self::Jpeg),
            "image/webp" => Ok(Self::Webp),
            "image/tiff" => Ok(Self::Tiff),
            "image/heif" | "image/heic" | "image/heif-sequence" | "image/heic-sequence" => {
                Ok(Self::Heif)
            }
            "image/avif" => Ok(Self::Avif),
            "image/gif" => Ok(Self::Gif),
            "image/bmp" => Ok(Self::Bmp),
            other => Err(format!(
                "This image format ({other}) is not enabled in the current libvips adapter"
            )),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Tiff => "tiff",
            Self::Heif => "heic",
            Self::Avif => "avif",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
        }
    }

    /// The format edits are saved in: the source format when libvips can
    /// write it, otherwise JPEG for photos and PNG for graphics.
    fn writable(self) -> Self {
        match self {
            Self::Heif | Self::Avif => Self::Jpeg,
            Self::Gif | Self::Bmp => Self::Png,
            other => other,
        }
    }

    fn capability(self) -> &'static str {
        match self {
            Self::Png => "image:save:png",
            Self::Jpeg => "image:save:jpeg",
            Self::Webp => "image:save:webp",
            Self::Tiff => "image:save:tiff",
            Self::Heif | Self::Avif => "image:load:heif",
            Self::Gif => "image:load:gif",
            Self::Bmp => "image:load:bmp",
        }
    }

    fn output_argument(self, path: &Path, quality: u32, strip: bool) -> OsString {
        let mut value = path.as_os_str().to_os_string();
        let mut options = Vec::new();
        if matches!(self, Self::Jpeg | Self::Webp) {
            options.push(format!("Q={quality}"));
        }
        if strip {
            options.push("strip".into());
        }
        if !options.is_empty() {
            value.push(format!("[{}]", options.join(",")));
        }
        value
    }
}

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &crate::Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    if matches!(
        manifest.id.as_str(),
        "arcade.image.combine"
            | "arcade.image.watermark"
            | "arcade.image.favicon"
            | "arcade.image.passport"
    ) {
        return compose::execute(manifest, request, runtime, cancelled);
    }
    let grants = runtime.grants();
    if manifest.id == "arcade.image.compare" {
        return compare_images(manifest, request, grants, cancelled);
    }
    let [input] = request.inputs.as_slice() else {
        return Err(format!("{} needs one selected image", manifest.name));
    };
    if input.kind != ValueKind::Artifact || input.mime != "file/image" {
        return Err("Select one image through Arcade Box".into());
    }
    let source = grants
        .resolve(&input.value)
        .map_err(|error| error.to_string())?;
    match manifest.id.as_str() {
        "arcade.image.metadata" => {
            return metadata_tool(manifest, request, &source, grants, cancelled);
        }
        "arcade.image.palette" => return palette(manifest, &source, cancelled),
        "arcade.image.ocr" => return image_ocr(manifest, request, runtime, &source, cancelled),
        "arcade.image.background-remove" => {
            return remove_background(manifest, request, &source, grants, cancelled);
        }
        "arcade.image.upscale" => {
            return upscale_image(manifest, request, &source, grants, cancelled);
        }
        "arcade.image.redact"
            if request.options.get("action").and_then(Value::as_str) == Some("find") =>
        {
            let language = request
                .options
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("eng");
            let report = pii::suggest(
                &source,
                image_dimensions_from_file(&source)?,
                language,
                cancelled,
            )?;
            return Ok(ToolResult {
                tool_id: manifest.id.clone(),
                status: ResultStatus::Success,
                message: report["message"].as_str().map(str::to_owned),
                outputs: vec![ToolValue::text(
                    report.to_string(),
                    "structured/redaction-report",
                )],
                warnings: vec![],
                metadata: Default::default(),
            });
        }
        _ => {}
    }
    let source_format = ImageFormat::from_file(&source)?;
    let output_format = match manifest.id.as_str() {
        "arcade.image.convert" => {
            let format = request
                .options
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("png");
            ImageFormat::parse(format)?
        }
        "arcade.image.resize"
        | "arcade.image.crop"
        | "arcade.image.compress"
        | "arcade.image.redact" => source_format.writable(),
        _ => {
            return Err(format!(
                "No image executor is registered for {}",
                manifest.id
            ));
        }
    };
    let quality = request
        .options
        .get("quality")
        .and_then(Value::as_u64)
        .unwrap_or(82);
    if !(1..=100).contains(&quality) {
        return Err("Image quality must be between 1 and 100".into());
    }
    let mut needed = vec![output_format.capability()];
    if source_format != source_format.writable() {
        needed.push(source_format.capability());
    }
    match manifest.id.as_str() {
        "arcade.image.crop" => needed.extend([
            "image:operation:crop",
            "image:operation:rot",
            "image:operation:flip",
        ]),
        "arcade.image.redact" => needed.push("image:operation:draw_rect"),
        "arcade.image.metadata" => needed.push("image:operation:autorot"),
        _ => {}
    }
    let provider = image_provider(&needed)?;
    if manifest.id == "arcade.image.compress"
        && request.options.get("mode").and_then(Value::as_str) == Some("targetSize")
    {
        return target_size::compress(manifest, request, &source, &provider, grants, cancelled);
    }
    execute_with_provider(
        manifest,
        request,
        &source,
        source_format,
        output_format,
        quality as u32,
        &provider,
        grants,
        cancelled,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_with_provider(
    manifest: &ToolManifest,
    request: &ToolRequest,
    source: &Path,
    source_format: ImageFormat,
    output_format: ImageFormat,
    quality: u32,
    provider: &ProviderInfo,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let parent = source
        .parent()
        .ok_or("Selected image has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-image-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create private image workspace: {error}"))?;
    let local_input = temp
        .path()
        .join(format!("input.{}", source_format.extension()));
    link_or_copy(source, &local_input, cancelled)?;
    let local_output = temp
        .path()
        .join(format!("output.{}", output_format.extension()));
    let dimensions = image_dimensions(&provider.executable_path, &local_input, cancelled)?;
    let lossless = request
        .options
        .get("lossless")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let strip = manifest.id == "arcade.image.metadata"
        || request
            .options
            .get("stripMetadata")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if lossless
        && matches!(source_format, ImageFormat::Jpeg)
        && manifest.id == "arcade.image.compress"
    {
        return Err("JPEG cannot be recompressed losslessly. Choose a lossless format conversion or disable lossless mode.".into());
    }
    let output_argument = output_format.output_argument(&local_output, quality, strip);
    let args: Vec<OsString> = match manifest.id.as_str() {
        "arcade.image.convert" => {
            // JPEG has no alpha: composite transparent sources onto white.
            // libvips rejects flatten on images without an alpha band.
            let flatten = matches!(output_format, ImageFormat::Jpeg)
                && matches!(
                    image_bands(&provider.executable_path, &local_input, cancelled)?,
                    2 | 4
                );
            let mut args = vec![
                if flatten { "flatten" } else { "copy" }.into(),
                local_input.as_os_str().to_os_string(),
                output_argument,
            ];
            if flatten {
                args.push("--background=255 255 255".into());
            }
            args
        }
        "arcade.image.resize" => {
            let (width, height, crop) = resize_bounds(&request.options, dimensions)?;
            let mut args = vec![
                "thumbnail".into(),
                local_input.as_os_str().to_os_string(),
                output_argument,
                width.to_string().into(),
                format!("--height={height}").into(),
                "--size=both".into(),
            ];
            if crop {
                args.push("--crop=centre".into());
            }
            args
        }
        "arcade.image.compress" => {
            let mut output_argument = local_output.as_os_str().to_os_string();
            let options = match output_format {
                ImageFormat::Jpeg => format!("Q={quality}"),
                ImageFormat::Webp if lossless => "lossless".into(),
                ImageFormat::Webp => format!("Q={quality}"),
                ImageFormat::Png => "compression=9".into(),
                ImageFormat::Tiff => "compression=deflate".into(),
                _ => unreachable!("edits always save a writable format"),
            };
            output_argument.push(format!("[{options}]"));
            vec![
                "copy".into(),
                local_input.as_os_str().to_os_string(),
                output_argument,
            ]
        }
        "arcade.image.crop" => {
            let mut current = local_input.clone();
            let crop_enabled = request
                .options
                .get("cropEnabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let (x, y, width, height) = if crop_enabled {
                crop_bounds(&request.options, dimensions)?
            } else {
                (0, 0, dimensions.0, dimensions.1)
            };
            let mut stage_number = 0;
            let next_stage = |number: &mut usize| {
                *number += 1;
                temp.path().join(format!("transform-{}.v", *number))
            };
            if crop_enabled {
                let stage = next_stage(&mut stage_number);
                run_vips(
                    provider,
                    vec![
                        "crop".into(),
                        current.as_os_str().to_os_string(),
                        stage.as_os_str().to_os_string(),
                        x.to_string().into(),
                        y.to_string().into(),
                        width.to_string().into(),
                        height.to_string().into(),
                    ],
                    temp.path(),
                    cancelled,
                    "crop the image",
                )?;
                current = stage;
            }
            let rotation = request
                .options
                .get("rotation")
                .and_then(|value| {
                    value
                        .as_u64()
                        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                })
                .unwrap_or(0);
            if ![0, 90, 180, 270].contains(&rotation) {
                return Err("Rotation must be 0, 90, 180, or 270 degrees".into());
            }
            if rotation != 0 {
                let stage = next_stage(&mut stage_number);
                let direction = match rotation {
                    90 => "d90",
                    180 => "d180",
                    270 => "d270",
                    _ => unreachable!(),
                };
                run_vips(
                    provider,
                    vec![
                        "rot".into(),
                        current.as_os_str().to_os_string(),
                        stage.as_os_str().to_os_string(),
                        direction.into(),
                    ],
                    temp.path(),
                    cancelled,
                    "rotate the image",
                )?;
                current = stage;
            }
            let flip = request
                .options
                .get("flip")
                .and_then(Value::as_str)
                .unwrap_or("none");
            if !["none", "horizontal", "vertical"].contains(&flip) {
                return Err("Flip must be none, horizontal, or vertical".into());
            }
            if flip != "none" {
                let stage = next_stage(&mut stage_number);
                run_vips(
                    provider,
                    vec![
                        "flip".into(),
                        current.as_os_str().to_os_string(),
                        stage.as_os_str().to_os_string(),
                        flip.into(),
                    ],
                    temp.path(),
                    cancelled,
                    "flip the image",
                )?;
                current = stage;
            }
            if !crop_enabled && rotation == 0 && flip == "none" {
                return Err("Choose a crop, rotation, or flip operation".into());
            }
            vec![
                "copy".into(),
                current.as_os_str().to_os_string(),
                output_argument,
            ]
        }
        "arcade.image.redact" => {
            let rectangles = redaction_rectangles(&request.options, dimensions)?;
            let mask = temp.path().join("redacted.v");
            run_vips(
                provider,
                vec![
                    "copy".into(),
                    local_input.as_os_str().to_os_string(),
                    mask.as_os_str().to_os_string(),
                ],
                temp.path(),
                cancelled,
                "prepare image redaction",
            )?;
            for (x, y, width, height) in &rectangles {
                run_vips(
                    provider,
                    vec![
                        "draw_rect".into(),
                        mask.as_os_str().to_os_string(),
                        "0 0 0".into(),
                        x.to_string().into(),
                        y.to_string().into(),
                        width.to_string().into(),
                        height.to_string().into(),
                        "--fill".into(),
                    ],
                    temp.path(),
                    cancelled,
                    "redact a selected image region",
                )?;
            }
            vec!["copy".into(), mask.into_os_string(), output_argument]
        }
        _ => unreachable!(),
    };
    let output = process::run(
        &ProcessSpec {
            executable: provider.executable_path.clone(),
            args,
            current_dir: Some(temp.path().to_path_buf()),
            timeout: Duration::from_secs(2 * 3600),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() || !local_output.is_file() {
        return Err(format!(
            "libvips could not process this image: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err("Image job cancelled".into());
    }
    let output_dimensions = image_dimensions(&provider.executable_path, &local_output, cancelled)?;
    let suffix = match manifest.id.as_str() {
        "arcade.image.resize" => "resized",
        "arcade.image.crop" => "edited",
        "arcade.image.compress" => "compressed",
        "arcade.image.redact" => "redacted",
        _ => "converted",
    };
    let default_name = format!(
        "{}-{suffix}.{}",
        safe_stem(source),
        output_format.extension()
    );
    let output_name = request
        .options
        .get("outputName")
        .and_then(Value::as_str)
        .unwrap_or(&default_name);
    validate_portable_filename(output_name)?;
    if !output_name
        .to_ascii_lowercase()
        .ends_with(&format!(".{}", output_format.extension()))
    {
        return Err(format!(
            "Output filename must end in .{}",
            output_format.extension()
        ));
    }
    let final_path = publish_without_overwrite(&local_output, parent, output_name, cancelled)
        .map_err(|error| format!("Cannot save image: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![selected.as_tool_value()],
        message: Some(format!("Saved {}", selected.name)),
        warnings: vec![],
        metadata: Default::default(),
    };
    if manifest.id == "arcade.image.convert" && matches!(output_format, ImageFormat::Jpeg) {
        result.warnings.push(
            "JPEG has no transparency; transparent pixels were composited onto white.".into(),
        );
    }
    if manifest.id == "arcade.image.compress" {
        let input_bytes = fs::metadata(source)
            .map_err(|error| error.to_string())?
            .len();
        let output_bytes = selected.size;
        let reduction = if input_bytes == 0 {
            0.0
        } else {
            (input_bytes as f64 - output_bytes as f64) * 100.0 / input_bytes as f64
        };
        result.message = Some(format!(
            "Compressed image: {input_bytes} → {output_bytes} bytes ({reduction:.1}% change)"
        ));
        result
            .metadata
            .insert("inputBytes".into(), json!(input_bytes));
        result
            .metadata
            .insert("reductionPercent".into(), json!(reduction));
        if matches!(output_format, ImageFormat::Png | ImageFormat::Tiff) {
            result.warnings.push(
                "This format uses lossless encoder settings; the output may not be smaller.".into(),
            );
        }
    }
    if manifest.id == "arcade.image.redact" {
        let region_count = request
            .options
            .get("regions")
            .and_then(Value::as_array)
            .map_or(1, Vec::len);
        result.warnings.push(format!("{region_count} selected region(s) were destructively replaced with solid black pixels in the new output file."));
        result
            .metadata
            .insert("redactionRegionCount".into(), json!(region_count));
    }
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(provider.version));
    result
        .metadata
        .insert("sourceWidth".into(), json!(dimensions.0));
    result
        .metadata
        .insert("sourceHeight".into(), json!(dimensions.1));
    result
        .metadata
        .insert("outputWidth".into(), json!(output_dimensions.0));
    result
        .metadata
        .insert("outputHeight".into(), json!(output_dimensions.1));
    result
        .metadata
        .insert("outputBytes".into(), json!(selected.size));
    result
        .metadata
        .insert("outputName".into(), json!(selected.name));
    Ok(result)
}

fn image_provider(capabilities: &[&str]) -> Result<ProviderInfo, String> {
    discover_vips(None)
        .into_iter()
        .find(|provider| {
            provider.compatible
                && capabilities.iter().all(|capability| {
                    provider
                        .capabilities
                        .iter()
                        .any(|value| value == capability)
                })
        })
        .ok_or_else(|| {
            format!(
                "No compatible system libvips provider supports {}",
                capabilities.join(", ")
            )
        })
}

fn run_vips(
    provider: &ProviderInfo,
    args: Vec<OsString>,
    working_directory: &Path,
    cancelled: &AtomicBool,
    operation: &str,
) -> Result<process::ProcessOutput, String> {
    let output = process::run(
        &ProcessSpec {
            executable: provider.executable_path.clone(),
            args,
            current_dir: Some(working_directory.to_path_buf()),
            timeout: Duration::from_secs(2 * 3600),
            output_limit: 2 * 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "libvips could not {operation}: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    Ok(output)
}

fn crop_bounds(options: &Value, dimensions: (u32, u32)) -> Result<(u32, u32, u32, u32), String> {
    let integer = |key: &str, default: Option<u32>| -> Result<u32, String> {
        let Some(value) = options.get(key) else {
            return default.ok_or_else(|| format!("Enter {key}"));
        };
        let value = value
            .as_u64()
            .ok_or_else(|| format!("{key} must be a whole number"))?;
        u32::try_from(value).map_err(|_| format!("{key} is too large"))
    };
    let x = integer("x", Some(0))?;
    let y = integer("y", Some(0))?;
    let width = integer("width", None)?;
    let height = integer("height", None)?;
    if width == 0
        || height == 0
        || x >= dimensions.0
        || y >= dimensions.1
        || x.saturating_add(width) > dimensions.0
        || y.saturating_add(height) > dimensions.1
    {
        return Err(format!(
            "The rectangle must be inside the image ({}×{})",
            dimensions.0, dimensions.1
        ));
    }
    Ok((x, y, width, height))
}

fn redaction_rectangles(
    options: &Value,
    dimensions: (u32, u32),
) -> Result<Vec<(u32, u32, u32, u32)>, String> {
    let Some(value) = options.get("regions") else {
        return Ok(vec![crop_bounds(options, dimensions)?]);
    };
    let regions = value
        .as_array()
        .ok_or("Redaction regions must be an array of normalized rectangles")?;
    if regions.is_empty() || regions.len() > 1000 {
        return Err("Select between 1 and 1000 image regions to redact".into());
    }
    if ["x", "y", "width", "height"]
        .iter()
        .any(|key| options.get(key).is_some())
    {
        return Err("Use either normalized regions or the single pixel rectangle, not both".into());
    }
    regions
        .iter()
        .map(|region| {
            let number = |key: &str| -> Result<f64, String> {
                region
                    .get(key)
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| format!("Every redaction region needs a finite {key} value"))
            };
            let x = number("x")?;
            let y = number("y")?;
            let width = number("width")?;
            let height = number("height")?;
            if x < 0.0
                || y < 0.0
                || width <= 0.0
                || height <= 0.0
                || x + width > 1.0
                || y + height > 1.0
            {
                return Err(
                    "Redaction regions must be inside the image using 0..1 coordinates".into(),
                );
            }
            let x0 = (x * f64::from(dimensions.0)).floor() as u32;
            let y0 = (y * f64::from(dimensions.1)).floor() as u32;
            let x1 = ((x + width) * f64::from(dimensions.0)).ceil() as u32;
            let y1 = ((y + height) * f64::from(dimensions.1)).ceil() as u32;
            if x0 >= dimensions.0 || y0 >= dimensions.1 || x0 >= x1 || y0 >= y1 {
                return Err("A redaction region is smaller than one image pixel".into());
            }
            Ok((x0, y0, x1.min(dimensions.0) - x0, y1.min(dimensions.1) - y0))
        })
        .collect()
}

fn metadata_tool(
    manifest: &ToolManifest,
    request: &ToolRequest,
    source: &Path,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let provider = image_provider(&["image:operation:autorot"])?;
    let header = provider.executable_path.with_file_name(if cfg!(windows) {
        "vipsheader.exe"
    } else {
        "vipsheader"
    });
    let raw = process::run(
        &ProcessSpec {
            executable: header,
            args: vec!["-a".into(), source.as_os_str().to_os_string()],
            current_dir: None,
            timeout: Duration::from_secs(30),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !raw.status.success() {
        return Err(format!(
            "Could not read image metadata: {}",
            concise(&String::from_utf8_lossy(&raw.stderr))
        ));
    }
    let metadata_text = String::from_utf8_lossy(&raw.stdout).into_owned();
    let action = request
        .options
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("inspect");
    if action == "inspect" {
        let report = metadata_report(&metadata_text);
        let message = if report["hasLocation"] == true {
            "This photo contains GPS location data"
        } else {
            "Read the image's embedded details"
        };
        return Ok(ToolResult {
            tool_id: manifest.id.clone(),
            status: ResultStatus::Success,
            outputs: vec![ToolValue::text(
                report.to_string(),
                "structured/image-metadata",
            )],
            message: Some(message.into()),
            warnings: vec![],
            metadata: Default::default(),
        });
    }
    if action != "sanitize" {
        return Err("Choose Inspect or Remove metadata".into());
    }
    let format = ImageFormat::from_file(source)?;
    let parent = source
        .parent()
        .ok_or("Selected image has no parent directory")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-image-meta-")
        .tempdir_in(parent)
        .map_err(|error| error.to_string())?;
    let local = temp.path().join(format!("input.{}", format.extension()));
    link_or_copy(source, &local, cancelled)?;
    let autorot = temp.path().join("orientation-corrected.v");
    run_vips(
        &provider,
        vec![
            "autorot".into(),
            local.as_os_str().to_os_string(),
            autorot.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "apply image orientation",
    )?;
    let output_path = temp.path().join(format!("output.{}", format.extension()));
    let output_arg = format.output_argument(&output_path, 100, true);
    run_vips(
        &provider,
        vec![
            "copy".into(),
            autorot.as_os_str().to_os_string(),
            output_arg,
        ],
        temp.path(),
        cancelled,
        "remove image metadata",
    )?;
    let fallback = format!("{}-private.{}", safe_stem(source), format.extension());
    let output_name = request
        .options
        .get("outputName")
        .and_then(Value::as_str)
        .unwrap_or(&fallback);
    validate_portable_filename(output_name)?;
    if !output_name
        .to_ascii_lowercase()
        .ends_with(&format!(".{}", format.extension()))
    {
        return Err(format!(
            "Output filename must end in .{}",
            format.extension()
        ));
    }
    let final_path = publish_without_overwrite(&output_path, parent, output_name, cancelled)
        .map_err(|error| error.to_string())?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![
            selected.as_tool_value(),
            ToolValue::text(
                json!({"removedMetadata":metadata_text}).to_string(),
                "structured/image-metadata",
            ),
        ],
        message: Some(format!("Saved metadata-cleaned image {}", selected.name)),
        warnings: vec![
            "All embedded metadata was removed. Orientation was applied to pixels before metadata was stripped; JPEG output is re-encoded at quality 100.".into(),
        ],
        metadata: Default::default(),
    };
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    Ok(result)
}

/// Readable groups from `vipsheader -a` output. The file path (first line
/// and `filename`) never leaves this function.
fn metadata_report(text: &str) -> Value {
    let mut all = serde_json::Map::new();
    for line in text.lines().skip(1) {
        let Some((key, value)) = line.split_once(": ") else {
            continue;
        };
        let key = key.trim();
        if key == "filename" || key.is_empty() || value.contains("bytes of binary data") {
            continue;
        }
        // EXIF values read "Canon (Canon, ASCII, 6 components, 6 bytes)".
        let value = match value.split_once(" (") {
            Some((readable, rest)) if rest.ends_with(')') && rest.contains("components") => {
                readable
            }
            _ => value,
        };
        all.insert(key.to_owned(), json!(value.trim()));
    }
    let pick = |keys: &[(&str, &str)]| {
        let mut group = serde_json::Map::new();
        for (key, label) in keys {
            if let Some(value) = all.get(*key) {
                group.insert((*label).to_owned(), value.clone());
            }
        }
        group
    };
    let mut image = pick(&[
        ("width", "width"),
        ("height", "height"),
        ("interpretation", "colourSpace"),
        ("bands", "channels"),
        ("vips-loader", "format"),
        ("orientation", "orientation"),
    ]);
    if let Some(Value::String(loader)) = image.get_mut("format") {
        *loader = loader.trim_end_matches("load").to_uppercase();
    }
    if let Some(dpi) = all
        .get("xres")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
    {
        image.insert("dpi".into(), json!((dpi * 25.4).round()));
    }
    let camera = pick(&[
        ("exif-ifd0-Make", "make"),
        ("exif-ifd0-Model", "model"),
        ("exif-ifd2-LensModel", "lens"),
        ("exif-ifd2-DateTimeOriginal", "taken"),
        ("exif-ifd2-ExposureTime", "exposure"),
        ("exif-ifd2-FNumber", "aperture"),
        ("exif-ifd2-ISOSpeedRatings", "iso"),
        ("exif-ifd2-FocalLength", "focalLength"),
        ("exif-ifd0-Software", "software"),
    ]);
    let location = all
        .iter()
        .filter(|(key, _)| key.starts_with("exif-ifd3-GPS"))
        .map(|(key, value)| {
            (
                key.trim_start_matches("exif-ifd3-GPS").to_owned(),
                value.clone(),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "hasLocation": !location.is_empty(),
        "image": image,
        "camera": if camera.is_empty() { Value::Null } else { Value::Object(camera) },
        "location": if location.is_empty() { Value::Null } else { Value::Object(location) },
        "allFields": all,
    })
}

fn palette(
    manifest: &ToolManifest,
    source: &Path,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let provider = image_provider(&["image:resize"])?;
    let format = ImageFormat::from_file(source)?;
    let parent = source
        .parent()
        .ok_or("Selected image has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-palette-")
        .tempdir_in(parent)
        .map_err(|error| error.to_string())?;
    let local = temp.path().join(format!("input.{}", format.extension()));
    link_or_copy(source, &local, cancelled)?;
    let thumb = temp.path().join("sample.png");
    run_vips(
        &provider,
        vec![
            "thumbnail".into(),
            local.as_os_str().to_os_string(),
            thumb.as_os_str().to_os_string(),
            "64".into(),
            "--height=64".into(),
            "--size=both".into(),
        ],
        temp.path(),
        cancelled,
        "sample image colors",
    )?;
    let pixels = ::image::open(&thumb)
        .map_err(|error| format!("Could not read color sample: {error}"))?
        .to_rgb8();
    let mut bins = HashMap::<u16, (u64, [u64; 3])>::new();
    for pixel in pixels.pixels() {
        let [r, g, b] = pixel.0;
        let key = ((r as u16 >> 3) << 10) | ((g as u16 >> 3) << 5) | (b as u16 >> 3);
        let entry = bins.entry(key).or_insert((0, [0, 0, 0]));
        entry.0 += 1;
        entry.1[0] += r as u64;
        entry.1[1] += g as u64;
        entry.1[2] += b as u64;
    }
    let mut colors = bins.into_values().collect::<Vec<_>>();
    colors.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let colors = colors
        .into_iter()
        .take(8)
        .map(|(count, sums)| {
            let rgb = [
                (sums[0] / count) as u8,
                (sums[1] / count) as u8,
                (sums[2] / count) as u8,
            ];
            json!({
                "hex":format!("#{:02X}{:02X}{:02X}",rgb[0],rgb[1],rgb[2]),
                "rgb":rgb,
                "hsl":rgb_to_hsl(rgb),
                "samples":count
            })
        })
        .collect::<Vec<_>>();
    let output = json!({
        "colors":colors,
        "method":"local 5-bit RGB histogram of a 64px thumbnail"
    });
    Ok(ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![ToolValue::text(
            output.to_string(),
            "structured/color-palette",
        )],
        message: Some("Extracted dominant image colors locally".into()),
        warnings: vec![],
        metadata: Default::default(),
    })
}

fn rgb_to_hsl(rgb: [u8; 3]) -> [f64; 3] {
    let r = rgb[0] as f64 / 255.0;
    let g = rgb[1] as f64 / 255.0;
    let b = rgb[2] as f64 / 255.0;
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
    [
        (hue + 360.0).rem_euclid(360.0),
        saturation * 100.0,
        lightness * 100.0,
    ]
}

fn compare_images(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    if request.inputs.len() != 2
        || request
            .inputs
            .iter()
            .any(|input| input.kind != ValueKind::Artifact || input.mime != "file/image")
    {
        return Err("Select two images through Arcade Box".into());
    }
    let sources = request
        .inputs
        .iter()
        .map(|input| {
            grants
                .resolve(&input.value)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let provider = image_provider(&[
        "image:operation:subtract",
        "image:operation:add",
        "image:operation:abs",
        "image:operation:avg",
        "image:operation:max",
    ])?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-image-diff-")
        .tempdir_in(
            sources[0]
                .parent()
                .ok_or("Selected image has no parent folder")?,
        )
        .map_err(|error| error.to_string())?;
    let first_format = ImageFormat::from_file(&sources[0])?;
    let second_format = ImageFormat::from_file(&sources[1])?;
    let first = temp
        .path()
        .join(format!("left.{}", first_format.extension()));
    let second = temp
        .path()
        .join(format!("right.{}", second_format.extension()));
    link_or_copy(&sources[0], &first, cancelled)?;
    link_or_copy(&sources[1], &second, cancelled)?;
    let left_dimensions = image_dimensions(&provider.executable_path, &first, cancelled)?;
    let right_dimensions = image_dimensions(&provider.executable_path, &second, cancelled)?;
    if left_dimensions != right_dimensions {
        return Err("Images must have the same dimensions for pixel-difference statistics".into());
    }
    let bands = image_bands(&provider.executable_path, &first, cancelled)?;
    if bands != image_bands(&provider.executable_path, &second, cancelled)? {
        return Err("Images must have the same number of color/alpha channels for pixel-difference statistics".into());
    }
    let first_minus = temp.path().join("left-minus-right.v");
    let second_minus = temp.path().join("right-minus-left.v");
    let first_abs = temp.path().join("left-absolute.v");
    let second_abs = temp.path().join("right-absolute.v");
    let difference = temp.path().join("difference.v");
    run_vips(
        &provider,
        vec![
            "subtract".into(),
            first.as_os_str().to_os_string(),
            second.as_os_str().to_os_string(),
            first_minus.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "compare image pixels",
    )?;
    run_vips(
        &provider,
        vec![
            "subtract".into(),
            second.as_os_str().to_os_string(),
            first.as_os_str().to_os_string(),
            second_minus.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "compare image pixels",
    )?;
    run_vips(
        &provider,
        vec![
            "abs".into(),
            first_minus.as_os_str().to_os_string(),
            first_abs.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "calculate absolute image differences",
    )?;
    run_vips(
        &provider,
        vec![
            "abs".into(),
            second_minus.as_os_str().to_os_string(),
            second_abs.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "calculate absolute image differences",
    )?;
    run_vips(
        &provider,
        vec![
            "add".into(),
            first_abs.as_os_str().to_os_string(),
            second_abs.as_os_str().to_os_string(),
            difference.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "combine pixel differences",
    )?;
    let average = scalar_vips(&provider, "avg", &difference, temp.path(), cancelled)?;
    let maximum = scalar_vips(&provider, "max", &difference, temp.path(), cancelled)?;
    let result_data = json!({
        "width":left_dimensions.0,"height":left_dimensions.1,"bands":bands,
        "meanAbsoluteDifference":average,"maximumChannelDifference":maximum,
        "leftBytes":fs::metadata(&sources[0]).map_err(|error|error.to_string())?.len(),
        "rightBytes":fs::metadata(&sources[1]).map_err(|error|error.to_string())?.len(),
        "pixelValues": "libvips difference after matching dimensions and band count"
    });
    Ok(ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![ToolValue::text(result_data.to_string(), "structured/image-diff")],
        message: Some("Compared image pixels locally".into()),
        warnings: vec!["The current form returns difference statistics only; side-by-side, overlay, slider, and diff-image previews remain pending.".into()],
        metadata: Default::default(),
    })
}

fn image_bands(vips: &Path, input: &Path, cancelled: &AtomicBool) -> Result<u32, String> {
    let header = vips.with_file_name(if cfg!(windows) {
        "vipsheader.exe"
    } else {
        "vipsheader"
    });
    let output = process::run(
        &ProcessSpec {
            executable: header,
            args: vec![
                "-f".into(),
                "bands".into(),
                input.as_os_str().to_os_string(),
            ],
            current_dir: None,
            timeout: Duration::from_secs(10),
            output_limit: 64 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Could not read image channel count".into());
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| "libvips returned an invalid channel count".into())
}

fn scalar_vips(
    provider: &ProviderInfo,
    operation: &str,
    input: &Path,
    cwd: &Path,
    cancelled: &AtomicBool,
) -> Result<f64, String> {
    let output = run_vips(
        provider,
        vec![operation.into(), input.as_os_str().to_os_string()],
        cwd,
        cancelled,
        "calculate image difference statistics",
    )?;
    String::from_utf8(output.stdout)
        .map_err(|_| String::from("libvips returned invalid statistics"))?
        .trim()
        .parse::<f64>()
        .map_err(|_| "libvips returned invalid image statistics".into())
}

fn remove_background(
    manifest: &ToolManifest,
    request: &ToolRequest,
    source: &Path,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let requested_model = request
        .options
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("u2netp");
    if !["u2netp", "u2net"].contains(&requested_model) {
        return Err("Choose the U²-Net or lightweight U²-Net-P model".into());
    }
    let model = discover_background_removal_models()
        .into_iter()
        .find(|model| model.asset.model_name == requested_model)
        .ok_or_else(|| {
            format!(
                "No compatible local rembg provider with the {requested_model} model was found. Install rembg and place the model in its local cache; Arcade Box will not download it."
            )
        })?;
    let source_format = ImageFormat::from_file(source)?;
    if matches!(source_format, ImageFormat::Tiff) {
        return Err("The rembg image adapter currently accepts PNG, JPEG, and WebP inputs".into());
    }
    let parent = source
        .parent()
        .ok_or("Selected image has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-background-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private model workspace: {error}"))?;
    let input = temp
        .path()
        .join(format!("input.{}", source_format.extension()));
    let output = temp.path().join("transparent.png");
    link_or_copy(source, &input, cancelled)?;
    let alpha = request
        .options
        .get("alphaMatting")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let post_process = request
        .options
        .get("postProcessMask")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    rembg_cutout(
        &model,
        &input,
        &output,
        alpha,
        post_process,
        temp.path(),
        cancelled,
    )?;
    validate_generated_png(&output)?;
    let input_dimensions = image_dimensions_from_file(source)?;
    if image_dimensions_from_file(&output)? != input_dimensions {
        return Err("Background provider changed the input dimensions unexpectedly".into());
    }
    let output_name = image_output_name(request, source, "-background-removed", "png")?;
    publish_model_output(
        manifest,
        &model,
        &output,
        parent,
        &output_name,
        grants,
        cancelled,
        "Removed image background locally",
        vec![],
    )
}

/// Cut the subject out of `input` into a transparent PNG at `output` using a
/// pre-installed rembg model. Never downloads a model.
pub(crate) fn rembg_cutout(
    model: &crate::provider::ImageModelProvider,
    input: &Path,
    output: &Path,
    alpha_matting: bool,
    post_process: bool,
    working_directory: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let model_path = model
        .asset
        .path
        .to_str()
        .ok_or("The local model path must be representable as UTF-8 for rembg")?;
    let extras = json!({"model_path":model_path}).to_string();
    let mut args: Vec<OsString> = vec![
        "i".into(),
        "-m".into(),
        "u2net_custom".into(),
        "-x".into(),
        extras.into(),
    ];
    if alpha_matting {
        args.push("-a".into());
    }
    if post_process {
        args.push("-ppm".into());
    }
    args.extend([
        input.as_os_str().to_os_string(),
        output.as_os_str().to_os_string(),
    ]);
    let status = process::run_with_env(
        &ProcessSpec {
            executable: model.provider.executable_path.clone(),
            args,
            current_dir: Some(working_directory.to_path_buf()),
            timeout: Duration::from_secs(3 * 3600),
            output_limit: 1024 * 1024,
        },
        cancelled,
        &model.environment(),
    )
    .map_err(|error| format!("Background removal was interrupted: {error}"))?;
    if !status.status.success() {
        return Err(format!(
            "rembg could not remove the background: {}",
            concise(&String::from_utf8_lossy(&status.stderr))
        ));
    }
    Ok(())
}

fn upscale_image(
    manifest: &ToolManifest,
    request: &ToolRequest,
    source: &Path,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let model = discover_upscale_models()
        .into_iter()
        .next()
        .ok_or("No compatible Real-ESRGAN NCNN/Vulkan provider with the x4plus model was found. Install the provider and its local model package; Arcade Box will not download weights.")?;
    let source_format = ImageFormat::from_file(source)?;
    if !matches!(
        source_format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Webp
    ) {
        return Err("Real-ESRGAN currently accepts PNG, JPEG, and WebP inputs".into());
    }
    let scale = request
        .options
        .get("scale")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(2);
    if ![2, 3, 4].contains(&scale) {
        return Err("Choose a 2×, 3×, or 4× upscale".into());
    }
    let (width, height) = image_dimensions_from_file(source)?;
    let output_width = width
        .checked_mul(scale as u32)
        .ok_or("Requested image dimensions exceed the supported output limit")?;
    let output_height = height
        .checked_mul(scale as u32)
        .ok_or("Requested image dimensions exceed the supported output limit")?;
    if output_width > MAX_DIMENSION
        || output_height > MAX_DIMENSION
        || output_width as u64 * output_height as u64 > MAX_OUTPUT_PIXELS
    {
        return Err("Requested upscale exceeds Arcade Box's output dimension limit".into());
    }
    let parent = source
        .parent()
        .ok_or("Selected image has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-upscale-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private model workspace: {error}"))?;
    let input = temp
        .path()
        .join(format!("input.{}", source_format.extension()));
    let output = temp.path().join("upscaled.png");
    link_or_copy(source, &input, cancelled)?;
    let output_status = process::run(
        &ProcessSpec {
            executable: model.provider.executable_path.clone(),
            args: vec![
                "-i".into(),
                input.as_os_str().to_os_string(),
                "-o".into(),
                output.as_os_str().to_os_string(),
                "-s".into(),
                scale.to_string().into(),
                "-m".into(),
                model.asset.path.as_os_str().to_os_string(),
                "-n".into(),
                model.asset.model_name.clone().into(),
                "-f".into(),
                "png".into(),
            ],
            current_dir: Some(temp.path().to_path_buf()),
            timeout: Duration::from_secs(3 * 3600),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| format!("Image upscaling was interrupted: {error}"))?;
    if !output_status.status.success() {
        return Err(format!(
            "Real-ESRGAN could not upscale the image: {}",
            concise(&String::from_utf8_lossy(&output_status.stderr))
        ));
    }
    validate_generated_png(&output)?;
    if image_dimensions_from_file(&output)? != (output_width, output_height) {
        return Err("Real-ESRGAN produced unexpected output dimensions".into());
    }
    let output_name = image_output_name(request, source, &format!("-upscaled-x{scale}"), "png")?;
    publish_model_output(
        manifest,
        &model,
        &output,
        parent,
        &output_name,
        grants,
        cancelled,
        &format!("Upscaled image {scale}× using a local enhancement model"),
        vec![
            "The model may synthesize fine detail; inspect the result before relying on it.".into(),
        ],
    )
}

fn publish_model_output(
    manifest: &ToolManifest,
    model: &ImageModelProvider,
    staged: &Path,
    parent: &Path,
    output_name: &str,
    grants: &FileGrants,
    cancelled: &AtomicBool,
    message: &str,
    warnings: Vec<String>,
) -> Result<ToolResult, String> {
    let final_path = publish_without_overwrite(staged, parent, output_name, cancelled)
        .map_err(|error| format!("Could not save model output: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![selected.as_tool_value()],
        message: Some(format!("{message}: {}", selected.name)),
        warnings,
        metadata: Default::default(),
    };
    result
        .metadata
        .insert("modelId".into(), json!(model.asset.id));
    result
        .metadata
        .insert("modelName".into(), json!(model.asset.model_name));
    result
        .metadata
        .insert("modelVersion".into(), json!(model.asset.version));
    result
        .metadata
        .insert("modelSha256".into(), json!(model.asset.sha256));
    result
        .metadata
        .insert("modelLicense".into(), json!(model.asset.license));
    result.metadata.insert(
        "modelPath".into(),
        json!(model.asset.path.display().to_string()),
    );
    result.metadata.insert(
        "providerPath".into(),
        json!(model.provider.executable_path.display().to_string()),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(model.provider.version));
    Ok(result)
}

fn image_output_name(
    request: &ToolRequest,
    source: &Path,
    suffix: &str,
    extension: &str,
) -> Result<String, String> {
    let fallback = format!("{}{suffix}.{extension}", safe_stem(source));
    let name = match request.options.get("outputName") {
        Some(value) if !value.is_null() && !value.is_string() => {
            return Err("Output filename must be text".into());
        }
        Some(value) => value.as_str().unwrap_or(&fallback),
        None => &fallback,
    };
    validate_portable_filename(name)?;
    if !name
        .to_ascii_lowercase()
        .ends_with(&format!(".{extension}"))
    {
        return Err(format!("Output filename must end in .{extension}"));
    }
    Ok(name.to_owned())
}

fn image_dimensions_from_file(path: &Path) -> Result<(u32, u32), String> {
    ::image::ImageReader::open(path)
        .map_err(|error| format!("Could not inspect image dimensions: {error}"))?
        .with_guessed_format()
        .map_err(|error| format!("Could not detect image format: {error}"))?
        .into_dimensions()
        .map_err(|error| format!("Could not read image dimensions: {error}"))
}

fn validate_generated_png(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("Image model completed without producing an output file".into());
    }
    let kind = infer::get_from_path(path)
        .map_err(|error| error.to_string())?
        .ok_or("Image model output has no recognized file signature")?;
    if kind.mime_type() != "image/png"
        || fs::metadata(path).map_err(|error| error.to_string())?.len() == 0
    {
        return Err("Image model did not produce a valid PNG output".into());
    }
    Ok(())
}

fn image_ocr(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &crate::Arcade,
    source: &Path,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let language = request
        .options
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("eng");
    if language.is_empty()
        || language.len() > 64
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("Choose a valid OCR language code".into());
    }
    let choice = request
        .options
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    if !matches!(choice, "auto" | "lens" | "tesseract") {
        return Err("Choose a valid OCR engine".into());
    }
    let lens =
        crate::link::consumer::peer_action(runtime, arcade_link::ids::LENS, "lens.recognize");
    if choice == "lens" || choice == "auto" && !cfg!(target_os = "linux") && lens.is_some() {
        return crate::link::consumer::recognize_image(
            runtime, manifest, source, language, cancelled,
        );
    }
    let provider = crate::provider::discover_tesseract()
        .into_iter()
        .find(|provider| {
            provider.compatible
                && provider
                    .capabilities
                    .iter()
                    .any(|cap| cap == &format!("ocr:language:{language}"))
        });
    let provider = match provider {
        Some(provider) => provider,
        None if choice == "auto" && lens.is_some() => {
            return crate::link::consumer::recognize_image(
                runtime, manifest, source, language, cancelled,
            );
        }
        None => {
            return Err(format!(
                "No compatible local Tesseract provider has the {language} language pack installed"
            ));
        }
    };
    let format = ImageFormat::from_file(source)?;
    let parent = source
        .parent()
        .ok_or("Selected image has no parent directory")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-image-ocr-")
        .tempdir_in(parent)
        .map_err(|error| error.to_string())?;
    let local = temp.path().join(format!("input.{}", format.extension()));
    link_or_copy(source, &local, cancelled)?;
    let output = process::run(
        &ProcessSpec {
            executable: provider.executable_path.clone(),
            args: vec![
                local.as_os_str().to_os_string(),
                "stdout".into(),
                "-l".into(),
                language.into(),
            ],
            current_dir: Some(temp.path().to_path_buf()),
            timeout: Duration::from_secs(2 * 3600),
            output_limit: 8 * 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Tesseract could not recognize this image: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "Tesseract returned invalid UTF-8")?;
    Ok(ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![ToolValue::text(text, "text/plain")],
        message: Some("Recognized image text locally".into()),
        warnings: vec![],
        metadata: [
            ("language".into(), json!(language)),
            (
                "providerPath".into(),
                json!(provider.executable_path.display().to_string()),
            ),
            ("providerVersion".into(), json!(provider.version)),
        ]
        .into_iter()
        .collect(),
    })
}

fn link_or_copy(source: &Path, destination: &Path, cancelled: &AtomicBool) -> Result<(), String> {
    if fs::hard_link(source, destination).is_ok() {
        return Ok(());
    }
    let mut input = File::open(source).map_err(|error| error.to_string())?;
    let mut output = File::create_new(destination).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Image job cancelled".into());
        }
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn image_dimensions(
    vips: &Path,
    input: &Path,
    cancelled: &AtomicBool,
) -> Result<(u32, u32), String> {
    let header = vips.with_file_name(if cfg!(windows) {
        "vipsheader.exe"
    } else {
        "vipsheader"
    });
    let field = |name: &str| -> Result<u32, String> {
        let output = process::run(
            &ProcessSpec {
                executable: header.clone(),
                args: vec!["-f".into(), name.into(), input.as_os_str().to_os_string()],
                current_dir: None,
                timeout: Duration::from_secs(10),
                output_limit: 64 * 1024,
            },
            cancelled,
        )
        .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!("libvips could not read image {name}"));
        }
        let value = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
        let dimension: u32 = value
            .trim()
            .parse()
            .map_err(|_| format!("libvips returned an invalid image {name}"))?;
        if dimension == 0 {
            return Err(format!("Image {name} is zero"));
        }
        Ok(dimension)
    };
    Ok((field("width")?, field("height")?))
}

fn resize_bounds(options: &Value, source: (u32, u32)) -> Result<(u32, u32, bool), String> {
    let mode = options.get("mode").and_then(Value::as_str).unwrap_or("fit");
    let size = |key: &str| -> Result<Option<u32>, String> {
        let Some(value) = options.get(key) else {
            return Ok(None);
        };
        let raw = value
            .as_u64()
            .ok_or_else(|| format!("{key} must be a positive whole number"))?;
        if !(1..=MAX_DIMENSION as u64).contains(&raw) {
            return Err(format!("{key} must be between 1 and {MAX_DIMENSION}"));
        }
        Ok(Some(raw as u32))
    };
    let (width, height, crop) = match mode {
        "fit" | "fill" => {
            let width = size("width")?;
            let height = size("height")?;
            if mode == "fill" && (width.is_none() || height.is_none()) {
                return Err("Fill needs both width and height".into());
            }
            if width.is_none() && height.is_none() {
                return Err("Enter a target width or height".into());
            }
            let width =
                width.unwrap_or_else(|| scaled(source.0, height.unwrap() as f64 / source.1 as f64));
            let height = height.unwrap_or_else(|| scaled(source.1, width as f64 / source.0 as f64));
            (width, height, mode == "fill")
        }
        "longest" | "shortest" => {
            let edge = size("edge")?.ok_or("Enter an edge length")?;
            let source_edge = if mode == "longest" {
                source.0.max(source.1)
            } else {
                source.0.min(source.1)
            };
            let factor = edge as f64 / source_edge as f64;
            (scaled(source.0, factor), scaled(source.1, factor), false)
        }
        "percentage" => {
            let percentage = options
                .get("percentage")
                .and_then(Value::as_f64)
                .ok_or("Enter a percentage")?;
            if !percentage.is_finite() || !(1.0..=1000.0).contains(&percentage) {
                return Err("Percentage must be between 1 and 1000".into());
            }
            let factor = percentage / 100.0;
            (scaled(source.0, factor), scaled(source.1, factor), false)
        }
        _ => return Err(format!("Unsupported resize mode: {mode}")),
    };
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || width as u64 * height as u64 > MAX_OUTPUT_PIXELS
    {
        return Err("Requested image dimensions exceed the supported output limit".into());
    }
    Ok((width, height, crop))
}

fn scaled(dimension: u32, factor: f64) -> u32 {
    (dimension as f64 * factor)
        .round()
        .clamp(1.0, u32::MAX as f64) as u32
}

fn safe_stem(source: &Path) -> String {
    let raw = source.file_stem().unwrap_or_default().to_string_lossy();
    let stem: String = raw
        .chars()
        .take(100)
        .map(|ch| {
            if ch.is_control() || matches!(ch, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let stem = stem.trim_matches([' ', '.']);
    if stem.is_empty() {
        "image".into()
    } else {
        stem.into()
    }
}

fn concise(stderr: &str) -> String {
    stderr
        .split_whitespace()
        .take(80)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_contract::ToolRequest;

    #[test]
    fn installed_vips_converts_and_resizes_real_images() {
        let Some(provider) = discover_vips(None)
            .into_iter()
            .find(|provider| provider.compatible)
        else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let generated = temp.path().join("generated.png");
        let source = temp.path().join("source[Q=1].png");
        let output = process::run(
            &ProcessSpec {
                executable: provider.executable_path.clone(),
                args: vec![
                    "black".into(),
                    generated.as_os_str().to_os_string(),
                    "100".into(),
                    "50".into(),
                ],
                current_dir: None,
                timeout: Duration::from_secs(10),
                output_limit: 64 * 1024,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(output.status.success());
        fs::rename(generated, &source).unwrap();
        let runtime = crate::Arcade::in_memory().unwrap();
        let selected = runtime.grants().grant(&source).unwrap();
        let converted = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.image.convert".into(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"format":"webp","quality":80}),
            })
            .unwrap();
        assert_eq!(
            converted.status,
            ResultStatus::Success,
            "{:?}",
            converted.message
        );
        let converted_path = runtime
            .grants()
            .resolve(&converted.outputs[0].value)
            .unwrap();
        assert!(converted_path.is_file());
        let resized = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.image.resize".into(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"mode":"fit","width":25,"height":25}),
            })
            .unwrap();
        assert_eq!(
            resized.status,
            ResultStatus::Success,
            "{:?}",
            resized.message
        );
        assert_eq!(resized.metadata.get("outputWidth"), Some(&json!(25)));
        assert_eq!(resized.metadata.get("outputHeight"), Some(&json!(13)));
        assert_eq!(fs::metadata(source).unwrap().len(), selected.size);
    }

    #[test]
    fn installed_vips_edits_redacts_compares_and_extracts_palette() {
        let Some(provider) = discover_vips(None)
            .into_iter()
            .find(|provider| provider.compatible)
        else {
            return;
        };
        assert!(
            provider
                .capabilities
                .iter()
                .any(|cap| cap == "image:operation:draw_rect")
        );
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("red.png");
        let second = temp.path().join("blue.png");
        ::image::RgbImage::from_pixel(32, 24, ::image::Rgb([240, 0, 0]))
            .save(&source)
            .unwrap();
        ::image::RgbImage::from_pixel(32, 24, ::image::Rgb([0, 0, 240]))
            .save(&second)
            .unwrap();
        let runtime = crate::Arcade::in_memory().unwrap();
        let first_grant = runtime.grants().grant(&source).unwrap();
        let second_grant = runtime.grants().grant(&second).unwrap();
        let call = |tool_id: &str, inputs: Vec<ToolValue>, options: Value| {
            runtime.run_tool(ToolRequest {
                tool_id: tool_id.into(),
                inputs,
                options,
            })
        };

        let edited = call(
            "arcade.image.crop",
            vec![first_grant.as_tool_value()],
            json!({"cropEnabled":true,"x":4,"y":2,"width":20,"height":10,"rotation":"90","flip":"horizontal"}),
        )
        .unwrap();
        assert_eq!(edited.status, ResultStatus::Success, "{:?}", edited.message);
        assert_eq!(edited.metadata["outputWidth"], 10);
        assert_eq!(edited.metadata["outputHeight"], 20);

        let redacted = call(
            "arcade.image.redact",
            vec![first_grant.as_tool_value()],
            json!({"x":4,"y":3,"width":8,"height":6}),
        )
        .unwrap();
        assert_eq!(
            redacted.status,
            ResultStatus::Success,
            "{:?}",
            redacted.message
        );
        let redacted_path = runtime
            .grants()
            .resolve(&redacted.outputs[0].value)
            .unwrap();
        let pixels = ::image::open(redacted_path).unwrap().to_rgb8();
        assert_eq!(pixels.get_pixel(5, 4).0, [0, 0, 0]);
        assert_eq!(pixels.get_pixel(1, 1).0, [240, 0, 0]);

        let multi_redacted = call(
            "arcade.image.redact",
            vec![first_grant.as_tool_value()],
            json!({"regions":[
                {"x":0.125,"y":0.125,"width":0.25,"height":0.25},
                {"x":0.625,"y":0.5,"width":0.25,"height":0.25}
            ],"outputName":"red-multi-redacted.png"}),
        )
        .unwrap();
        assert_eq!(
            multi_redacted.status,
            ResultStatus::Success,
            "{:?}",
            multi_redacted.message
        );
        assert_eq!(multi_redacted.metadata["redactionRegionCount"], 2);
        let multi_path = runtime
            .grants()
            .resolve(&multi_redacted.outputs[0].value)
            .unwrap();
        let multi_pixels = ::image::open(multi_path).unwrap().to_rgb8();
        assert_eq!(multi_pixels.get_pixel(5, 4).0, [0, 0, 0]);
        assert_eq!(multi_pixels.get_pixel(21, 13).0, [0, 0, 0]);
        assert_eq!(multi_pixels.get_pixel(1, 1).0, [240, 0, 0]);

        let compare = call(
            "arcade.image.compare",
            vec![first_grant.as_tool_value(), second_grant.as_tool_value()],
            json!({}),
        )
        .unwrap();
        assert_eq!(
            compare.status,
            ResultStatus::Success,
            "{:?}",
            compare.message
        );
        let stats: Value = serde_json::from_str(&compare.outputs[0].value).unwrap();
        assert!(
            stats["meanAbsoluteDifference"].as_f64().unwrap() > 0.0,
            "{stats}"
        );

        let palette = call(
            "arcade.image.palette",
            vec![first_grant.as_tool_value()],
            json!({}),
        )
        .unwrap();
        assert_eq!(
            palette.status,
            ResultStatus::Success,
            "{:?}",
            palette.message
        );
        let colors: Value = serde_json::from_str(&palette.outputs[0].value).unwrap();
        assert!(!colors["colors"].as_array().unwrap().is_empty());
    }

    #[test]
    fn size_modes_bound_output() {
        assert_eq!(
            resize_bounds(&json!({"mode":"fit","width":500}), (1000, 500)).unwrap(),
            (500, 250, false)
        );
        assert_eq!(
            resize_bounds(
                &json!({"mode":"fill","width":500,"height":500}),
                (1000, 500)
            )
            .unwrap(),
            (500, 500, true)
        );
        assert!(
            resize_bounds(
                &json!({"mode":"percentage","percentage":1000}),
                (10000, 10000)
            )
            .is_err()
        );
    }

    #[test]
    fn metadata_sanitizer_removes_embedded_exif_after_applying_orientation() {
        let Some(provider) = discover_vips(None)
            .into_iter()
            .find(|provider| provider.compatible)
        else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("camera.jpg");
        ::image::RgbImage::from_pixel(16, 12, ::image::Rgb([120, 80, 40]))
            .save(&source)
            .unwrap();
        inject_exif_make(&source, *b"XYZ\0");

        let input_header = image_header(&provider, &source);
        assert!(input_header.contains("exif-ifd0-Make"), "{input_header}");
        assert!(input_header.contains("XYZ"), "{input_header}");

        let runtime = crate::Arcade::in_memory().unwrap();
        let selected = runtime.grants().grant(&source).unwrap();
        let result = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.image.metadata".into(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"action":"sanitize","outputName":"camera-clean.jpg"}),
            })
            .unwrap();
        assert_eq!(result.status, ResultStatus::Success, "{:?}", result.message);
        let output = runtime.grants().resolve(&result.outputs[0].value).unwrap();
        let output_header = image_header(&provider, &output);
        assert!(!output_header.contains("exif-ifd0-Make"), "{output_header}");
        assert!(!output_header.contains("XYZ"), "{output_header}");
        assert!(source.is_file(), "sanitizing must preserve the original");
    }

    fn inject_exif_make(path: &Path, value: [u8; 4]) {
        let original = fs::read(path).unwrap();
        assert!(original.starts_with(&[0xff, 0xd8]));
        let mut payload = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x0f\x01\x02\0\x04\0\0\0".to_vec();
        payload.extend(value);
        payload.extend([0u8; 4]);
        let length = u16::try_from(payload.len() + 2).unwrap();
        let mut tagged = Vec::with_capacity(original.len() + payload.len() + 4);
        tagged.extend_from_slice(&original[..2]);
        tagged.extend_from_slice(&[0xff, 0xe1]);
        tagged.extend_from_slice(&length.to_be_bytes());
        tagged.extend_from_slice(&payload);
        tagged.extend_from_slice(&original[2..]);
        fs::write(path, tagged).unwrap();
    }

    fn image_header(provider: &ProviderInfo, path: &Path) -> String {
        let header = provider.executable_path.with_file_name(if cfg!(windows) {
            "vipsheader.exe"
        } else {
            "vipsheader"
        });
        let output = process::run(
            &ProcessSpec {
                executable: header,
                args: vec!["-a".into(), path.as_os_str().to_os_string()],
                current_dir: None,
                timeout: Duration::from_secs(10),
                output_limit: 1024 * 1024,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}
