//! Compress an image to fit under a size such as "100 KB" for upload forms.
//! Searches JPEG/WebP quality first and only reduces the dimensions when even
//! low quality is too large.

use super::{ImageFormat, image_bands, image_dimensions, link_or_copy, run_vips, safe_stem};
use crate::{
    artifacts::publish_without_overwrite,
    grants::FileGrants,
    provider::ProviderInfo,
    tool_kit::{check_cancelled, number_in, output_name, success},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use std::{fs, path::Path, sync::atomic::AtomicBool};

const MIN_QUALITY: u32 = 20;
const MAX_QUALITY: u32 = 92;

pub(super) fn compress(
    manifest: &ToolManifest,
    request: &ToolRequest,
    source: &Path,
    provider: &ProviderInfo,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let target_kb = number_in(
        request,
        "targetKb",
        "Target size",
        Some(100.0),
        5.0..=50_000.0,
    )?;
    let target = (target_kb * 1024.0) as u64;
    let source_format = ImageFormat::from_file(source)?;
    // Only JPEG and WebP have a quality dial; everything else becomes JPEG.
    let format = if source_format == ImageFormat::Webp {
        ImageFormat::Webp
    } else {
        ImageFormat::Jpeg
    };
    let parent = source
        .parent()
        .ok_or("Selected image has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-image-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create private image workspace: {error}"))?;
    let input = temp
        .path()
        .join(format!("input.{}", source_format.extension()));
    link_or_copy(source, &input, cancelled)?;
    let original_size = fs::metadata(source)
        .map_err(|error| error.to_string())?
        .len();
    // Flatten transparency onto white for JPEG.
    let mut working = input.clone();
    if format == ImageFormat::Jpeg
        && matches!(
            image_bands(&provider.executable_path, &input, cancelled)?,
            2 | 4
        )
    {
        let flat = temp.path().join("flat.v");
        run_vips(
            provider,
            vec![
                "flatten".into(),
                input.into_os_string(),
                flat.as_os_str().into(),
                "--background=255 255 255".into(),
            ],
            temp.path(),
            cancelled,
            "flatten the image",
        )?;
        working = flat;
    }
    let (width, height) = image_dimensions(&provider.executable_path, &working, cancelled)?;
    let output = temp.path().join(format!("output.{}", format.extension()));
    let encode = |from: &Path, quality: u32| -> Result<u64, String> {
        check_cancelled(cancelled)?;
        let mut argument = output.as_os_str().to_os_string();
        argument.push(format!("[Q={quality},strip]"));
        run_vips(
            provider,
            vec!["copy".into(), from.as_os_str().into(), argument],
            temp.path(),
            cancelled,
            "encode the image",
        )?;
        fs::metadata(&output)
            .map(|meta| meta.len())
            .map_err(|error| error.to_string())
    };
    let mut scale = 1.0f64;
    let mut current = working.clone();
    let mut chosen = None;
    for _ in 0..6 {
        // Binary search for the highest quality that fits.
        let (mut low, mut high, mut best) = (MIN_QUALITY, MAX_QUALITY, None);
        while low <= high {
            let quality = (low + high) / 2;
            if encode(&current, quality)? <= target {
                best = Some(quality);
                low = quality + 1;
            } else {
                high = quality.saturating_sub(1);
            }
        }
        if let Some(quality) = best {
            let bytes = encode(&current, quality)?;
            chosen = Some((quality, bytes));
            break;
        }
        // Even minimum quality is too big: shrink the dimensions and retry.
        let smallest = encode(&current, MIN_QUALITY)?;
        scale *= ((target as f64 / smallest as f64).sqrt() * 0.92).clamp(0.3, 0.95);
        if (width as f64 * scale) < 64.0 || (height as f64 * scale) < 64.0 {
            break;
        }
        let resized = temp.path().join("resized.v");
        run_vips(
            provider,
            vec![
                "resize".into(),
                working.as_os_str().into(),
                resized.as_os_str().into(),
                format!("{scale:.4}").into(),
            ],
            temp.path(),
            cancelled,
            "resize the image",
        )?;
        current = resized;
    }
    let (quality, bytes) =
        chosen.ok_or_else(|| format!("Could not get this image under {target_kb} KB"))?;
    let (out_width, out_height) = image_dimensions(&provider.executable_path, &output, cancelled)?;
    let default_name = format!(
        "{}-{}kb.{}",
        safe_stem(source),
        target_kb.round(),
        format.extension()
    );
    let name = output_name(request, &default_name)?;
    let final_path = publish_without_overwrite(&output, parent, &name, cancelled)
        .map_err(|error| format!("Cannot save image: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut warnings = Vec::new();
    if source_format != format {
        warnings.push(format!(
            "Saved as {} so the size could be controlled.",
            format.extension().to_uppercase()
        ));
    }
    if scale < 1.0 {
        warnings.push(format!(
            "Reduced from {width}×{height} to {out_width}×{out_height} to fit."
        ));
    }
    if bytes >= original_size {
        warnings.push("The original was already under this size.".into());
    }
    let mut result = success(
        manifest,
        vec![selected.as_tool_value()],
        Some(format!(
            "Saved {} — {:.0} KB at quality {quality}",
            selected.name,
            bytes as f64 / 1024.0
        )),
        warnings,
    );
    result.metadata.insert("outputBytes".into(), bytes.into());
    Ok(result)
}
