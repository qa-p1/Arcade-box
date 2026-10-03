//! Shrink a PDF under a target size (for upload limits) with Ghostscript,
//! stepping image resolution down until the file fits. Text and vector
//! content stay sharp; only images are resampled.

use super::{format_bytes, link_or_copy_pdf, output_name, selected_pdf, success_file};
use crate::{
    artifacts::publish_without_overwrite,
    grants::FileGrants,
    process::{self, ProcessSpec},
    provider::find_system_executable,
    tool_kit::{check_cancelled, number_in},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use std::{ffi::OsString, fs, path::Path, sync::atomic::AtomicBool, time::Duration};

/// Image resolutions (DPI) tried in order, sharpest first.
const LADDER: [u32; 7] = [200, 150, 120, 96, 72, 60, 48];

pub(super) fn to_size(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let target_mb = number_in(request, "targetMb", "Target size", Some(5.0), 0.05..=2000.0)?;
    let target = (target_mb * 1024.0 * 1024.0) as u64;
    let original = fs::metadata(&source)
        .map_err(|error| error.to_string())?
        .len();
    let gs = find_system_executable("gs")
        .ok_or("Compressing to a size needs Ghostscript (the `gs` command)")?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private PDF workspace: {error}"))?;
    // Fixed relative names: Ghostscript treats `%` in output paths as a format.
    link_or_copy_pdf(&source, &temp.path().join("in.pdf"), cancelled)?;
    let mut best: Option<(u32, u64)> = None;
    for dpi in LADDER {
        check_cancelled(cancelled)?;
        let output = format!("out-{dpi}.pdf");
        run_gs(&gs, temp.path(), dpi, &output, cancelled)?;
        let size = fs::metadata(temp.path().join(&output))
            .map_err(|error| error.to_string())?
            .len();
        if best.is_none_or(|(_, smallest)| size < smallest) {
            best = Some((dpi, size));
        }
        if size <= target {
            break;
        }
    }
    let (dpi, size) = best.ok_or("Ghostscript produced no output")?;
    if size > target {
        return Err(format!(
            "The smallest this PDF gets is {} (images at {dpi} DPI). Text and fonts can't shrink further; try a larger target.",
            format_bytes(size)
        ));
    }
    if size >= original {
        return Err(format!(
            "This PDF is already {} — under the target",
            format_bytes(original)
        ));
    }
    let name = output_name(request, &source, "-compressed.pdf")?;
    let final_path = publish_without_overwrite(
        &temp.path().join(format!("out-{dpi}.pdf")),
        parent,
        &name,
        cancelled,
    )
    .map_err(|error| format!("Could not save the compressed PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(
        manifest,
        selected,
        &format!(
            "{} → {} (images at {dpi} DPI)",
            format_bytes(original),
            format_bytes(size)
        ),
    );
    result.metadata.insert("outputBytes".into(), size.into());
    Ok(result)
}

fn run_gs(
    gs: &Path,
    dir: &Path,
    dpi: u32,
    output: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mono = (dpi * 2).max(300);
    let args: Vec<OsString> = [
        "-dSAFER".to_owned(),
        "-dBATCH".into(),
        "-dNOPAUSE".into(),
        "-dQUIET".into(),
        "-sDEVICE=pdfwrite".into(),
        "-dCompatibilityLevel=1.6".into(),
        "-dPDFSETTINGS=/ebook".into(),
        "-dDetectDuplicateImages=true".into(),
        "-dCompressFonts=true".into(),
        "-dSubsetFonts=true".into(),
        "-dDownsampleColorImages=true".into(),
        "-dDownsampleGrayImages=true".into(),
        "-dDownsampleMonoImages=true".into(),
        "-dColorImageDownsampleType=/Bicubic".into(),
        "-dGrayImageDownsampleType=/Bicubic".into(),
        format!("-dColorImageResolution={dpi}"),
        format!("-dGrayImageResolution={dpi}"),
        format!("-dMonoImageResolution={mono}"),
        "-dColorImageDownsampleThreshold=1.0".into(),
        "-dGrayImageDownsampleThreshold=1.0".into(),
        format!("-sOutputFile={output}"),
        "in.pdf".into(),
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let result = process::run(
        &ProcessSpec {
            executable: gs.to_path_buf(),
            args,
            current_dir: Some(dir.to_path_buf()),
            timeout: Duration::from_secs(60 * 60),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| format!("Ghostscript could not run: {error}"))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!(
            "Ghostscript could not rewrite this PDF: {}",
            stderr
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("unknown error")
        ));
    }
    Ok(())
}
