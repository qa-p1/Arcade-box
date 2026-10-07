//! Structural PDF actions through a verified qpdf provider.

use crate::{
    artifacts::{publish_without_overwrite, validate_portable_filename},
    grants::FileGrants,
    process::{self, ProcessSpec},
    provider::{
        discover_img2pdf, discover_libreoffice, discover_ocrmypdf, discover_poppler, discover_qpdf,
    },
};
use arcade_contract::{ResultStatus, ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use printpdf::{
    BuiltinFont, Color, ExtendedGraphicsState, Mm, Op, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Pt, RawImage, RawImageData, RawImageFormat, Rgb, TextItem,
    XObjectTransform,
};
use serde_json::json;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

mod form;
mod shrink;
mod sign;

struct PdfOperationContext<'a> {
    manifest: &'a ToolManifest,
    executable: &'a Path,
    version: &'a str,
    output_directory: &'a Path,
    grants: &'a FileGrants,
    cancelled: &'a AtomicBool,
}

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    match manifest.id.as_str() {
        "arcade.pdf.merge" => merge(manifest, request, grants, cancelled),
        "arcade.pdf.split" => split(manifest, request, grants, cancelled),
        "arcade.pdf.organize" => organize(manifest, request, grants, cancelled),
        "arcade.pdf.compress"
            if request
                .options
                .get("mode")
                .and_then(serde_json::Value::as_str)
                == Some("targetSize") =>
        {
            shrink::to_size(manifest, request, grants, cancelled)
        }
        "arcade.pdf.compress" => optimize(manifest, request, grants, cancelled),
        "arcade.pdf.sign" => sign::sign(manifest, request, grants, cancelled),
        "arcade.pdf.fill" => form::fill(manifest, request, grants, cancelled),
        "arcade.pdf.images-to-pdf" => images_to_pdf(manifest, request, grants, cancelled),
        "arcade.pdf.pdf-to-images" => pdf_to_images(manifest, request, grants, cancelled),
        "arcade.pdf.ocr" => searchable_ocr(manifest, request, grants, cancelled),
        "arcade.pdf.extract" => extract_content(manifest, request, grants, cancelled),
        "arcade.pdf.watermark" => watermark(manifest, request, grants, cancelled),
        "arcade.pdf.protect" => protect(manifest, request, grants, cancelled),
        "arcade.pdf.convert" => convert_document(manifest, request, grants, cancelled),
        _ => Err(format!("No PDF executor is registered for {}", manifest.id)),
    }
}

fn merge(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    if !(2..=128).contains(&request.inputs.len()) {
        return Err("Select between 2 and 128 PDF files to merge".into());
    }
    let mut sources = Vec::with_capacity(request.inputs.len());
    for input in &request.inputs {
        if input.kind != ValueKind::Artifact || input.mime != "file/pdf" {
            return Err("Every source must be a PDF selected through Arcade Box".into());
        }
        sources.push(
            grants
                .resolve(&input.value)
                .map_err(|error| error.to_string())?,
        );
    }
    let first = &sources[0];
    let parent = first.parent().ok_or("First PDF has no parent folder")?;
    let output_name = request
        .options
        .get("outputName")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{}-merged.pdf",
                first.file_stem().unwrap_or_default().to_string_lossy()
            )
        });
    validate_portable_filename(&output_name)?;
    if !output_name.to_ascii_lowercase().ends_with(".pdf") {
        return Err("The output filename must end in .pdf".into());
    }
    let provider = qpdf_provider("pdf:merge")?;
    let operation = PdfOperationContext {
        manifest,
        executable: &provider.executable_path,
        version: &provider.version,
        output_directory: parent,
        grants,
        cancelled,
    };
    merge_with_provider(&operation, &sources, &output_name)
}

fn split(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let range = requested_page_range(request)?;
    let pages_per_file = request
        .options
        .get("pagesPerFile")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_u64()
                .filter(|count| (1..=100_000).contains(count))
                .ok_or("Pages per file must be an integer from 1 to 100000")
        })
        .transpose()?;
    let output_name = output_name(request, &source, "-pages.pdf")?;
    let provider = qpdf_provider("pdf:split")?;
    let operation = PdfOperationContext {
        manifest,
        executable: &provider.executable_path,
        version: &provider.version,
        output_directory: parent,
        grants,
        cancelled,
    };
    split_with_provider(&operation, &source, &output_name, &range, pages_per_file)
}

fn optimize(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let output_name = output_name(request, &source, "-optimized.pdf")?;
    let (mode, quality) = pdf_optimization_settings(&request.options)?;
    let provider = qpdf_provider("pdf:structural")?;
    if quality.is_some()
        && !provider
            .capabilities
            .iter()
            .any(|capability| capability == "pdf:optimize-images")
    {
        return Err("Image recompression requires a compatible qpdf 12.1+ provider with JPEG optimization support. Lossless structural optimization remains available.".into());
    }
    let operation = PdfOperationContext {
        manifest,
        executable: &provider.executable_path,
        version: &provider.version,
        output_directory: parent,
        grants,
        cancelled,
    };
    optimize_with_provider(&operation, &source, &output_name, mode, quality)
}

fn pdf_optimization_settings(
    options: &serde_json::Value,
) -> Result<(&'static str, Option<u32>), String> {
    let mode = options
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("lossless");
    match mode {
        "lossless" => Ok(("lossless", None)),
        "balanced" => Ok(("balanced", Some(82))),
        "small" => Ok(("small", Some(60))),
        "custom" => {
            let quality = options
                .get("quality")
                .and_then(serde_json::Value::as_u64)
                .ok_or("Enter a JPEG quality from 30 to 95")?;
            if !(30..=95).contains(&quality) {
                return Err("JPEG quality must be from 30 to 95".into());
            }
            Ok(("custom", Some(quality as u32)))
        }
        _ => Err("Choose lossless, balanced, small, or custom optimization".into()),
    }
}

fn organize(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let range = request
        .options
        .get("pageOrder")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("1-z");
    validate_qpdf_range(range)?;
    let rotation = integer_option(request.options.get("rotation")).unwrap_or(0);
    if ![0, 90, 180, 270].contains(&rotation) {
        return Err("Rotation must be 0, 90, 180, or 270 degrees".into());
    }
    if request
        .options
        .get("pageRotations")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err("Per-page rotations must be entered as text".into());
    }
    let page_rotations_text = request
        .options
        .get("pageRotations")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if rotation != 0 && !page_rotations_text.is_empty() {
        return Err("Choose either whole-document rotation or per-page rotations, not both".into());
    }
    let output_name = output_name(request, &source, "-organized.pdf")?;
    let provider = qpdf_provider("pdf:split")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private PDF workspace: {error}"))?;
    let selected = temp.path().join("selected.pdf");
    let mut range_arg = OsString::from("--range=");
    range_arg.push(range);
    run_qpdf(
        &provider.executable_path,
        vec![
            source.as_os_str().to_os_string(),
            "--pages".into(),
            source.as_os_str().to_os_string(),
            range_arg,
            "--".into(),
            selected.as_os_str().to_os_string(),
        ],
        temp.path(),
        cancelled,
        "reorder or remove PDF pages",
    )?;
    let page_rotations = if page_rotations_text.is_empty() {
        Vec::new()
    } else {
        let selected_page_count = qpdf_page_count(&provider.executable_path, &selected, cancelled)?;
        parse_pdf_page_rotations(page_rotations_text, selected_page_count)?
    };
    let staged = if rotation == 0 && page_rotations.is_empty() {
        selected
    } else {
        let rotated = temp.path().join("rotated.pdf");
        let mut rotate_args = Vec::new();
        if rotation != 0 {
            let mut rotate_arg = OsString::from("--rotate=+");
            rotate_arg.push(rotation.to_string());
            rotate_args.push(rotate_arg);
        }
        for rule in &page_rotations {
            let mut rotate_arg = OsString::from("--rotate=+");
            rotate_arg.push(rule.angle.to_string());
            rotate_arg.push(":");
            rotate_arg.push(&rule.pages);
            rotate_args.push(rotate_arg);
        }
        let mut args = vec![selected.as_os_str().to_os_string()];
        args.extend(rotate_args);
        args.extend(["--".into(), rotated.as_os_str().to_os_string()]);
        run_qpdf(
            &provider.executable_path,
            args,
            temp.path(),
            cancelled,
            "rotate selected PDF pages",
        )?;
        rotated
    };
    let final_path = publish_without_overwrite(&staged, parent, &output_name, cancelled)
        .map_err(|error| format!("Could not save organized PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(manifest, selected, "Reordered and saved PDF pages");
    result.warnings.push("Page reordering, duplication, deletion, whole-document rotation, and per-page rotation are supported. The current form does not provide page thumbnail previews or multi-select controls yet.".into());
    result.metadata.insert("pageOrder".into(), json!(range));
    result.metadata.insert("rotation".into(), json!(rotation));
    result.metadata.insert(
        "pageRotations".into(),
        json!(
            page_rotations
                .iter()
                .map(|rule| json!({"pages": rule.pages, "rotation": rule.angle}))
                .collect::<Vec<_>>()
        ),
    );
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    Ok(result)
}

fn images_to_pdf(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    if !(1..=128).contains(&request.inputs.len()) {
        return Err("Select between 1 and 128 images".into());
    }
    let mut sources = Vec::new();
    for input in &request.inputs {
        if input.kind != ValueKind::Artifact || input.mime != "file/image" {
            return Err("Select image files through Arcade Box".into());
        }
        sources.push(
            grants
                .resolve(&input.value)
                .map_err(|error| error.to_string())?,
        );
    }
    let first = &sources[0];
    let parent = first
        .parent()
        .ok_or("Selected image has no parent folder")?;
    let name = request
        .options
        .get("outputName")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}-images.pdf", safe_stem(first)));
    validate_pdf_output_name(&name)?;
    let page_size_name = request
        .options
        .get("pageSize")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("original");
    let page_size = match page_size_name {
        "original" => None,
        "A4" => Some("A4"),
        "letter" => Some("letter"),
        _ => return Err("Page size must be original, A4, or letter".into()),
    };
    let fit = request
        .options
        .get("fit")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("into");
    if !["into", "shrink", "fill", "exact", "enlarge"].contains(&fit) {
        return Err("Unsupported image placement mode".into());
    }
    if page_size.is_none() && fit != "into" {
        return Err("Image fit mode applies only when A4 or Letter page size is selected".into());
    }
    let orientation = request
        .options
        .get("orientation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("auto");
    if !["auto", "portrait", "landscape"].contains(&orientation) {
        return Err("Orientation must be auto, portrait, or landscape".into());
    }
    if page_size.is_none() && orientation != "auto" {
        return Err(
            "Orientation control applies only when A4 or Letter page size is selected".into(),
        );
    }
    let margin_mm = request
        .options
        .get("marginMm")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if margin_mm > 100 {
        return Err("Margins must be between 0 and 100 millimeters".into());
    }
    let dpi = request
        .options
        .get("dpi")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_u64()
                .filter(|dpi| (72..=600).contains(dpi))
                .ok_or("DPI must be an integer from 72 to 600")
        })
        .transpose()?;
    if page_size.is_some() && dpi.is_some() {
        return Err("Select Original page size to set a custom image DPI".into());
    }
    if page_size.is_none() && margin_mm > 0 && dpi.is_none() {
        return Err(
            "Original-sized pages need an explicit DPI before margins can be calculated. Choose A4 or Letter, or set an image DPI.".into(),
        );
    }
    let provider = discover_img2pdf()
        .into_iter()
        .find(|provider| {
            provider.compatible && provider.capabilities.iter().any(|cap| cap == "pdf:create")
        })
        .ok_or(
            "Images to PDF needs img2pdf (the `img2pdf` command). Install it, then try again.",
        )?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private PDF workspace: {error}"))?;
    let staged = temp.path().join("images.pdf");
    let mut args: Vec<OsString> = vec!["--output".into(), staged.as_os_str().to_os_string()];
    if let Some(page_size) = page_size {
        args.push("--pagesize".into());
        let page_size = match orientation {
            "auto" => page_size.to_owned(),
            "portrait" => page_size.to_owned(),
            "landscape" => format!("{page_size}^T"),
            _ => page_size.to_owned(),
        };
        args.push(page_size.into());
        if orientation == "auto" {
            args.push("--auto-orient".into());
        }
        args.push("--fit".into());
        args.push(fit.into());
    }
    if margin_mm > 0 {
        args.push("--border".into());
        args.push(format!("{margin_mm}mm").into());
    }
    if let Some(dpi) = dpi {
        args.push("--imgsize".into());
        args.push(format!("{dpi}dpi").into());
    }
    args.push("--".into());
    args.extend(sources.iter().map(|path| path.as_os_str().to_os_string()));
    let output = run_provider(
        &provider.executable_path,
        args,
        Some(temp.path()),
        cancelled,
        "create a PDF from images",
        1024 * 1024,
    )?;
    if !output.status.success() {
        let detail = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let detail = concise(&String::from_utf8_lossy(detail));
        return Err(if detail.is_empty() {
            format!(
                "img2pdf could not create a PDF (exit status {})",
                output
                    .status
                    .code()
                    .map_or_else(|| "unknown".to_owned(), |code| code.to_string())
            )
        } else {
            format!("img2pdf could not create a PDF: {detail}")
        });
    }
    let mut signature = [0; 5];
    File::open(&staged)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(|error| format!("img2pdf did not create a readable PDF: {error}"))?;
    if &signature != b"%PDF-" {
        return Err("img2pdf reported success but did not produce a valid PDF".into());
    }
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save image PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(manifest, selected, "Created PDF from selected images");
    result
        .metadata
        .insert("imageCount".into(), json!(sources.len()));
    result
        .metadata
        .insert("pageSize".into(), json!(page_size_name));
    result.metadata.insert("fit".into(), json!(fit));
    result
        .metadata
        .insert("orientation".into(), json!(orientation));
    result.metadata.insert("marginMm".into(), json!(margin_mm));
    if let Some(dpi) = dpi {
        result.metadata.insert("dpi".into(), json!(dpi));
    }
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    result.warnings.push("Per-image crop/fill previews and DPI overrides on fixed-size pages are not available. Review page sizes before printing.".into());
    Ok(result)
}

fn pdf_to_images(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let provider = poppler_provider("pdf.render")?;
    let dpi = request
        .options
        .get("dpi")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(150);
    if !(36..=600).contains(&dpi) {
        return Err("DPI must be from 36 to 600".into());
    }
    let format = request
        .options
        .get("format")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("png");
    let (switch, extension) = match format {
        "png" => ("-png", "png"),
        "jpeg" | "jpg" => ("-jpeg", "jpg"),
        _ => return Err("PDF pages can currently be rendered as PNG or JPEG".into()),
    };
    let page_count = pdf_page_count(&source, cancelled)?;
    if page_count == 0 || page_count > 5_000 {
        return Err("PDF must contain between 1 and 5000 pages for image rendering".into());
    }
    let first = request
        .options
        .get("pageFrom")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let last = request
        .options
        .get("pageTo")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(page_count);
    if first == 0 || first > last || last > page_count || last - first + 1 > 500 {
        return Err(format!(
            "Choose a page range within 1–{page_count} containing at most 500 pages"
        ));
    }
    let requested_name = request
        .options
        .get("outputName")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}-page.{extension}", safe_stem(&source)));
    validate_portable_filename(&requested_name)?;
    let requested_path = Path::new(&requested_name);
    let requested_extension = requested_path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    if requested_extension
        .as_deref()
        .is_some_and(|value| !["png", "jpg", "jpeg"].contains(&value))
    {
        return Err("Filename pattern may use only a PNG or JPEG extension".into());
    }
    let stem = requested_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if stem.is_empty() {
        return Err("Filename pattern needs a non-empty name".into());
    }
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private render workspace: {error}"))?;
    let prefix = temp.path().join("page");
    let output = run_provider(
        &provider.executable_path,
        vec![
            "-f".into(),
            first.to_string().into(),
            "-l".into(),
            last.to_string().into(),
            "-r".into(),
            dpi.to_string().into(),
            switch.into(),
            source.as_os_str().to_os_string(),
            prefix.as_os_str().to_os_string(),
        ],
        Some(temp.path()),
        cancelled,
        "render PDF pages to images",
        2 * 1024 * 1024,
    )?;
    if output.status.code().is_some_and(|code| code != 0) {
        return Err(format!(
            "Poppler could not render this PDF: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    let mut rendered = fs::read_dir(temp.path())
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        })
        .collect::<Vec<_>>();
    rendered.sort_by_key(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.rsplit('-').next())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(u64::MAX)
    });
    if rendered.len() != (last - first + 1) as usize {
        return Err(format!(
            "Poppler produced {} images; expected {} pages",
            rendered.len(),
            last - first + 1
        ));
    }
    let width = last.to_string().len();
    let mut published = Vec::new();
    let mut selected_files = Vec::new();
    for (offset, image) in rendered.iter().enumerate() {
        let page = first + offset as u64;
        let filename = format!("{stem}-{page:0width$}.{extension}");
        let path =
            publish_without_overwrite(image, parent, &filename, cancelled).map_err(|error| {
                format!(
                    "Could not save rendered page image: {error}{}",
                    preserved_outputs_note(&published)
                )
            })?;
        published.push(path.clone());
        selected_files.push(
            grants
                .grant(&path)
                .map_err(|error| format!("Could not grant a rendered image: {error}"))?,
        );
    }
    let mut result = ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: selected_files
            .iter()
            .map(|file| file.as_tool_value())
            .collect(),
        message: Some(format!(
            "Rendered {} PDF pages at {dpi} DPI",
            selected_files.len()
        )),
        warnings: vec![],
        metadata: Default::default(),
    };
    result.metadata.insert("pageFrom".into(), json!(first));
    result.metadata.insert("pageTo".into(), json!(last));
    result.metadata.insert("dpi".into(), json!(dpi));
    result.metadata.insert("format".into(), json!(format));
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    Ok(result)
}

fn searchable_ocr(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let language = request
        .options
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("eng");
    if language.is_empty()
        || language.len() > 64
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-'))
    {
        return Err("Choose a valid OCR language code".into());
    }
    let provider = discover_ocrmypdf().into_iter().find(|provider| provider.compatible)
        .ok_or("Searchable PDF needs OCRmyPDF (the `ocrmypdf` command) and Tesseract language data. Install them, then try again.")?;
    let name = output_name(request, &source, "-searchable.pdf")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-ocr-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private OCR workspace: {error}"))?;
    let staged = temp.path().join("searchable.pdf");
    run_provider(
        &provider.executable_path,
        vec![
            "--skip-text".into(),
            "-l".into(),
            language.into(),
            source.as_os_str().to_os_string(),
            staged.as_os_str().to_os_string(),
        ],
        Some(temp.path()),
        cancelled,
        "add a searchable OCR layer",
        2 * 1024 * 1024,
    )?;
    if !staged.is_file() {
        return Err("OCRmyPDF finished without creating the searchable PDF".into());
    }
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save searchable PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(manifest, selected, "Created a searchable PDF");
    result.metadata.insert("language".into(), json!(language));
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    Ok(result)
}

fn extract_content(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let text_provider = poppler_provider("pdf.text")?;
    let info_provider = poppler_provider("pdf.info")?;
    let images_provider = poppler_provider("pdf.images")?;
    let attachments_provider = poppler_provider("pdf.attachments")?;
    let text = run_provider(
        &text_provider.executable_path,
        vec![
            "-layout".into(),
            source.as_os_str().to_os_string(),
            "-".into(),
        ],
        source.parent(),
        cancelled,
        "extract PDF text",
        8 * 1024 * 1024,
    )?;
    let info = run_provider(
        &info_provider.executable_path,
        vec![source.as_os_str().to_os_string()],
        source.parent(),
        cancelled,
        "read PDF metadata",
        1024 * 1024,
    )?;
    let images = run_provider(
        &images_provider.executable_path,
        vec!["-list".into(), source.as_os_str().to_os_string()],
        source.parent(),
        cancelled,
        "inspect embedded PDF images",
        2 * 1024 * 1024,
    )?;
    let attachments = run_provider(
        &attachments_provider.executable_path,
        vec!["-list".into(), source.as_os_str().to_os_string()],
        source.parent(),
        cancelled,
        "inspect PDF attachments",
        2 * 1024 * 1024,
    )?;
    for output in [&text, &info, &images, &attachments] {
        if output.status.code().is_some_and(|code| code != 0) {
            return Err(format!(
                "A Poppler reader could not inspect the PDF: {}",
                concise(&String::from_utf8_lossy(&output.stderr))
            ));
        }
    }
    let include_images = request
        .options
        .get("includeImages")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let include_attachments = request
        .options
        .get("includeAttachments")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mut extracted_files = Vec::<ToolValue>::new();
    let mut published_files = Vec::new();
    let mut extracted_count = 0usize;
    let mut extracted_attachment_count = 0usize;
    if include_images {
        const MAX_EMBEDDED_IMAGES: usize = 500;
        const MAX_EMBEDDED_IMAGE_PIXELS: u64 = 300_000_000;
        const MAX_PUBLISHED_IMAGE_BYTES: u64 = 1024 * 1024 * 1024;
        let image_listing = String::from_utf8_lossy(&images.stdout);
        let (image_count, total_pixels) = pdf_image_listing_stats(&image_listing)?;
        if image_count > MAX_EMBEDDED_IMAGES || total_pixels > MAX_EMBEDDED_IMAGE_PIXELS {
            return Err(format!(
                "PDF has too many embedded images to extract safely ({} images, {} pixels)",
                image_count, total_pixels
            ));
        }
        if image_count > 0 {
            let temp = tempfile::Builder::new()
                .prefix(".arcade-pdf-images-")
                .tempdir_in(parent)
                .map_err(|error| {
                    format!("Cannot create a private image extraction workspace: {error}")
                })?;
            let prefix = temp.path().join("embedded");
            let extraction = run_provider(
                &images_provider.executable_path,
                vec![
                    "-all".into(),
                    source.as_os_str().to_os_string(),
                    prefix.as_os_str().to_os_string(),
                ],
                Some(temp.path()),
                cancelled,
                "extract embedded PDF images",
                2 * 1024 * 1024,
            )?;
            if !extraction.status.success() {
                return Err(format!(
                    "Poppler could not extract embedded PDF images: {}",
                    concise(&String::from_utf8_lossy(&extraction.stderr))
                ));
            }
            let mut images_to_publish = fs::read_dir(temp.path())
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("embedded-"))
                })
                .collect::<Vec<_>>();
            images_to_publish.sort();
            if images_to_publish.len() > MAX_EMBEDDED_IMAGES {
                return Err("PDF image extraction exceeded the 500-file safety limit".into());
            }
            if image_count > 0 && images_to_publish.is_empty() {
                return Err(
                    "Poppler listed embedded images but produced no extractable image files".into(),
                );
            }
            let mut total_output_bytes = 0u64;
            for (index, staged_image) in images_to_publish.iter().enumerate() {
                let metadata = fs::metadata(staged_image).map_err(|error| error.to_string())?;
                total_output_bytes = total_output_bytes
                    .checked_add(metadata.len())
                    .ok_or("Extracted image size total overflowed")?;
                if metadata.len() > 256 * 1024 * 1024
                    || total_output_bytes > MAX_PUBLISHED_IMAGE_BYTES
                {
                    return Err(
                        "Extracted PDF images exceed the 1 GiB publication safety limit".into(),
                    );
                }
                let extension = safe_image_extension(staged_image);
                let output_name =
                    format!("{}-image-{:03}.{extension}", safe_stem(&source), index + 1);
                let published =
                    publish_without_overwrite(staged_image, parent, &output_name, cancelled)
                        .map_err(|error| {
                            format!(
                                "Could not save extracted image: {error}{}",
                                preserved_outputs_note(&published_files)
                            )
                        })?;
                published_files.push(published.clone());
                let selected = grants.grant(&published).map_err(|error| {
                    format!(
                        "Could not grant extracted image output: {error}{}",
                        preserved_outputs_note(&published_files)
                    )
                })?;
                extracted_files.push(selected.as_tool_value());
            }
            extracted_count = extracted_files.len();
        }
    }
    let attachment_listing = String::from_utf8_lossy(&attachments.stdout);
    let attachment_entries = parse_pdf_attachment_listing(&attachment_listing)?;
    if include_attachments && !attachment_entries.is_empty() {
        const MAX_ATTACHMENTS: usize = 100;
        const MAX_ATTACHMENT_BYTES: u64 = 128 * 1024 * 1024;
        const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;
        if attachment_entries.len() > MAX_ATTACHMENTS {
            return Err(
                "PDF has more than 100 attachments; attachment extraction was stopped".into(),
            );
        }
        let temp = tempfile::Builder::new()
            .prefix(".arcade-pdf-attachments-")
            .tempdir_in(parent)
            .map_err(|error| {
                format!("Cannot create a private attachment extraction workspace: {error}")
            })?;
        let mut total_attachment_bytes = 0u64;
        for attachment in &attachment_entries {
            if cancelled.load(Ordering::Relaxed) {
                return Err("PDF attachment extraction cancelled".into());
            }
            let staged_attachment = temp
                .path()
                .join(format!("attachment-{:03}.bin", attachment.index));
            let extraction = run_provider_with_file_limit(
                &attachments_provider.executable_path,
                vec![
                    "-save".into(),
                    attachment.index.to_string().into(),
                    "-o".into(),
                    staged_attachment.as_os_str().to_os_string(),
                    source.as_os_str().to_os_string(),
                ],
                Some(temp.path()),
                &staged_attachment,
                MAX_ATTACHMENT_BYTES,
                cancelled,
                "extract a PDF attachment",
            )?;
            if !extraction.status.success() {
                return Err(format!(
                    "Poppler could not extract PDF attachment {}: {}{}",
                    attachment.index,
                    concise(&String::from_utf8_lossy(&extraction.stderr)),
                    preserved_outputs_note(&published_files)
                ));
            }
            let metadata = fs::symlink_metadata(&staged_attachment)
                .map_err(|error| format!("PDF attachment output is missing: {error}"))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("Poppler produced a non-regular attachment output".into());
            }
            if metadata.len() > MAX_ATTACHMENT_BYTES {
                return Err("PDF attachment exceeds the 128 MiB per-file limit".into());
            }
            total_attachment_bytes = total_attachment_bytes
                .checked_add(metadata.len())
                .ok_or("PDF attachment size total overflowed")?;
            if total_attachment_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
                return Err("PDF attachments exceed the 512 MiB total publication limit".into());
            }
            let output_name = format!(
                "{}-attachment-{:03}.bin",
                safe_stem(&source),
                attachment.index
            );
            let published =
                publish_without_overwrite(&staged_attachment, parent, &output_name, cancelled)
                    .map_err(|error| {
                        format!(
                            "Could not save PDF attachment: {error}{}",
                            preserved_outputs_note(&published_files)
                        )
                    })?;
            published_files.push(published.clone());
            let selected = grants.grant(&published).map_err(|error| {
                format!(
                    "Could not grant PDF attachment output: {error}{}",
                    preserved_outputs_note(&published_files)
                )
            })?;
            let mut value = selected.as_tool_value();
            value.mime = "file/octet-stream".into();
            extracted_files.push(value);
            extracted_attachment_count += 1;
        }
    }
    let extraction = json!({
        "text": String::from_utf8_lossy(&text.stdout),
        "metadata": String::from_utf8_lossy(&info.stdout),
        "images": String::from_utf8_lossy(&images.stdout),
        "attachments": attachment_entries.iter().map(|attachment| json!({ "index": attachment.index, "name": attachment.name })).collect::<Vec<_>>(),
    });
    let mut outputs = vec![ToolValue::text(
        extraction.to_string(),
        "structured/pdf-extraction",
    )];
    outputs.extend(extracted_files);
    let mut result = ToolResult {
        tool_id: manifest.id.clone(), status: ResultStatus::Success,
        outputs,
        message: Some(format!("Extracted PDF text and inspected metadata; saved {extracted_count} embedded image(s) and {extracted_attachment_count} attachment(s)")),
        warnings: vec!["Table structure is not reconstructed; extracted tables are represented only by reading-order text.".into()],
        metadata: Default::default(),
    };
    result.metadata.insert(
        "providerPath".into(),
        json!(text_provider.executable_path.display().to_string()),
    );
    result.metadata.insert(
        "textCharacters".into(),
        json!(String::from_utf8_lossy(&text.stdout).chars().count()),
    );
    result
        .metadata
        .insert("extractedImageCount".into(), json!(extracted_count));
    result
        .metadata
        .insert("attachmentCount".into(), json!(attachment_entries.len()));
    result.metadata.insert(
        "extractedAttachmentCount".into(),
        json!(extracted_attachment_count),
    );
    Ok(result)
}

fn pdf_image_listing_stats(listing: &str) -> Result<(usize, u64), String> {
    let mut count = 0usize;
    let mut pixels = 0u64;
    for line in listing.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 5
            || fields[0].parse::<u64>().is_err()
            || fields[1].parse::<u64>().is_err()
        {
            continue;
        }
        if !matches!(fields[2], "image" | "smask" | "mask" | "stencil") {
            continue;
        }
        let width = fields[3]
            .parse::<u64>()
            .map_err(|_| "Poppler reported an invalid embedded image width")?;
        let height = fields[4]
            .parse::<u64>()
            .map_err(|_| "Poppler reported an invalid embedded image height")?;
        if width == 0 || height == 0 {
            return Err("Poppler reported an empty embedded image".into());
        }
        pixels = pixels
            .checked_add(
                width
                    .checked_mul(height)
                    .ok_or("PDF image dimensions overflow")?,
            )
            .ok_or("PDF image pixel total overflow")?;
        count += 1;
    }
    Ok((count, pixels))
}

struct PdfAttachmentEntry {
    index: usize,
    name: String,
}

fn parse_pdf_attachment_listing(listing: &str) -> Result<Vec<PdfAttachmentEntry>, String> {
    const MAX_ATTACHMENTS: usize = 100;
    let mut entries = Vec::new();
    for line in listing.lines() {
        let Some((index, name)) = line.trim().split_once(':') else {
            continue;
        };
        let Ok(index) = index.trim().parse::<usize>() else {
            continue;
        };
        if index == 0 || index > MAX_ATTACHMENTS {
            return Err("PDF attachment listing exceeds the 100-file extraction limit".into());
        }
        if entries
            .iter()
            .any(|entry: &PdfAttachmentEntry| entry.index == index)
        {
            return Err("Poppler returned duplicate PDF attachment indexes".into());
        }
        entries.push(PdfAttachmentEntry {
            index,
            name: sanitize_embedded_filename(name),
        });
    }
    entries.sort_by_key(|entry| entry.index);
    if entries
        .iter()
        .enumerate()
        .any(|(position, entry)| entry.index != position + 1)
    {
        return Err("Poppler returned an incomplete PDF attachment listing".into());
    }
    let reported_count = listing.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let count = fields.next()?.parse::<usize>().ok()?;
        let label = fields.next()?.to_ascii_lowercase();
        label.starts_with("embedded").then_some(count)
    });
    if reported_count.is_some_and(|count| count != entries.len()) {
        return Err("Poppler returned an inconsistent PDF attachment listing".into());
    }
    Ok(entries)
}

fn sanitize_embedded_filename(raw: &str) -> String {
    let raw = raw.trim().trim_matches('"');
    let leaf = raw.replace('\\', "/");
    let leaf = leaf.rsplit('/').next().unwrap_or_default();
    let mut sanitized = leaf
        .chars()
        .take(120)
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_owned();
    if sanitized.is_empty() || validate_portable_filename(&sanitized).is_err() {
        sanitized = "attachment.bin".into();
    }
    sanitized
}

fn run_provider_with_file_limit(
    executable: &Path,
    args: Vec<OsString>,
    working_directory: Option<&Path>,
    output_path: &Path,
    max_file_bytes: u64,
    cancelled: &AtomicBool,
    operation: &str,
) -> Result<process::ProcessOutput, String> {
    let provider_cancelled = AtomicBool::new(false);
    let file_limit_hit = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let provider_cancelled_ref = &provider_cancelled;
        let runner = scope.spawn(move || {
            run_provider(
                executable,
                args,
                working_directory,
                provider_cancelled_ref,
                operation,
                64 * 1024,
            )
        });
        loop {
            if cancelled.load(Ordering::Relaxed) {
                provider_cancelled.store(true, Ordering::Release);
            }
            if fs::symlink_metadata(output_path)
                .is_ok_and(|metadata| metadata.len() > max_file_bytes)
            {
                file_limit_hit.store(true, Ordering::Release);
                provider_cancelled.store(true, Ordering::Release);
            }
            if runner.is_finished() {
                let result = runner
                    .join()
                    .map_err(|_| "PDF attachment provider worker failed".to_owned())?;
                if file_limit_hit.load(Ordering::Acquire) {
                    let _ = fs::remove_file(output_path);
                    return Err(format!(
                        "PDF attachment exceeded the {} MiB per-file limit",
                        max_file_bytes / (1024 * 1024)
                    ));
                }
                if cancelled.load(Ordering::Acquire) {
                    let _ = fs::remove_file(output_path);
                    return Err("PDF attachment extraction cancelled".into());
                }
                let result = result?;
                if fs::symlink_metadata(output_path)
                    .is_ok_and(|metadata| metadata.len() > max_file_bytes)
                {
                    let _ = fs::remove_file(output_path);
                    return Err(format!(
                        "PDF attachment exceeded the {} MiB per-file limit",
                        max_file_bytes / (1024 * 1024)
                    ));
                }
                return Ok(result);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })
}

fn safe_image_extension(path: &Path) -> &'static str {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return "bin";
    };
    match extension.to_ascii_lowercase().as_str() {
        "png" => "png",
        "jpg" | "jpeg" => "jpg",
        "tif" | "tiff" => "tiff",
        "jp2" | "jpx" => "jp2",
        "jb2" | "jbig2" => "jb2",
        "ccitt" => "ccitt",
        _ => "bin",
    }
}

fn watermark(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    const MAX_PAGES: u64 = 1000;
    const MAX_PAGE_POINTS: f64 = 20_000.0;
    const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
    const MAX_IMAGE_PIXELS: u64 = 40_000_000;

    if request.inputs.is_empty() || request.inputs.len() > 2 {
        return Err("Select one source PDF, followed by an optional watermark image".into());
    }
    if request
        .inputs
        .iter()
        .any(|input| input.kind != ValueKind::Artifact)
    {
        return Err("Watermark inputs must be files selected through Arcade Box".into());
    }
    let source = grants
        .resolve(&request.inputs[0].value)
        .map_err(|error| error.to_string())?;
    if request.inputs[0].mime != "file/pdf" {
        return Err("The first selected file must be the source PDF".into());
    }
    let mode = request
        .options
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("text");
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let name = output_name(request, &source, "-watermarked.pdf")?;
    let provider = qpdf_provider("pdf:structural")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private PDF workspace: {error}"))?;
    let staged = temp.path().join("watermarked.pdf");
    let page_count = pdf_page_count(&source, cancelled)?;
    if page_count == 0 || page_count > MAX_PAGES {
        return Err(format!(
            "Watermarking supports 1 to {MAX_PAGES} pages per job"
        ));
    }
    let page_count = page_count as u32;
    let page_sizes = pdf_page_sizes(&source, page_count, cancelled)?;
    if page_sizes.iter().any(|(width, height)| {
        !width.is_finite()
            || !height.is_finite()
            || *width <= 0.0
            || *height <= 0.0
            || *width > MAX_PAGE_POINTS
            || *height > MAX_PAGE_POINTS
    }) {
        return Err("The PDF contains a page with dimensions outside the supported range".into());
    }
    let selected_pages = watermark_page_selection(request, page_count)?;
    let opacity = watermark_opacity(request)?;
    let position = request
        .options
        .get("position")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("center");
    let overlay_pdf = temp.path().join("watermark-overlay.pdf");

    match mode {
        "text" => {
            if request.inputs.len() != 1 {
                return Err(
                    "Text watermarks use the source PDF only. Remove the second file.".into(),
                );
            }
            let text = request
                .options
                .get("text")
                .and_then(serde_json::Value::as_str)
                .ok_or("Enter watermark text")?;
            if text.is_empty() || text.chars().count() > 200 || text.contains(['\r', '\n', '\0']) {
                return Err("Watermark text must contain 1 to 200 characters on one line".into());
            }
            if !text.is_ascii() || text.chars().any(char::is_control) {
                return Err(
                    "The built-in Helvetica watermark currently supports printable ASCII text only"
                        .into(),
                );
            }
            let font_size = request
                .options
                .get("fontSize")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(36.0);
            if !font_size.is_finite() || !(6.0..=120.0).contains(&font_size) {
                return Err("Font size must be between 6 and 120 points".into());
            }
            let mut document = PdfDocument::new("Arcade Box PDF watermark");
            let gs = document.add_graphics_state(
                ExtendedGraphicsState::default().with_current_fill_alpha(opacity as f32),
            );
            let mut pages = Vec::with_capacity(page_count as usize);
            for (index, (width, height)) in page_sizes.iter().copied().enumerate() {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("PDF watermarking cancelled".into());
                }
                let mut ops = Vec::new();
                if selected_pages[index] {
                    let display = text
                        .replace("{page}", &(index + 1).to_string())
                        .replace("{pages}", &page_count.to_string());
                    let (x, y) = watermark_text_position(
                        &display,
                        font_size as f32,
                        width as f32,
                        height as f32,
                        position,
                    )?;
                    ops.extend([
                        Op::SaveGraphicsState,
                        Op::LoadGraphicsState { gs: gs.clone() },
                        Op::StartTextSection,
                        Op::SetTextCursor {
                            pos: Point { x: Pt(x), y: Pt(y) },
                        },
                        Op::SetFont {
                            font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                            size: Pt(font_size as f32),
                        },
                        Op::SetFillColor {
                            col: Color::Rgb(Rgb {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                icc_profile: None,
                            }),
                        },
                        Op::ShowText {
                            items: vec![TextItem::Text(display)],
                        },
                        Op::EndTextSection,
                        Op::RestoreGraphicsState,
                    ]);
                }
                pages.push(PdfPage::new(
                    Mm(points_to_mm(width)),
                    Mm(points_to_mm(height)),
                    ops,
                ));
            }
            document.with_pages(pages);
            let mut warnings = Vec::new();
            let bytes = document.save(&PdfSaveOptions::default(), &mut warnings);
            write_private(&overlay_pdf, &bytes)?;
        }
        "image" => {
            let [_, image_input] = request.inputs.as_slice() else {
                return Err(
                    "Image watermarks need the source PDF first, then one image file".into(),
                );
            };
            if !image_input.mime.starts_with("file/image") {
                return Err("The second selected file must be an image watermark".into());
            }
            let image_path = grants
                .resolve(&image_input.value)
                .map_err(|error| error.to_string())?;
            let metadata = fs::metadata(&image_path)
                .map_err(|error| format!("Cannot inspect watermark image: {error}"))?;
            if metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
                return Err(format!(
                    "Watermark image must be no larger than {} MiB",
                    MAX_IMAGE_BYTES / (1024 * 1024)
                ));
            }
            let (pixel_width, pixel_height) = ::image::image_dimensions(&image_path)
                .map_err(|error| format!("Cannot read watermark image dimensions: {error}"))?;
            let pixels = u64::from(pixel_width) * u64::from(pixel_height);
            if pixel_width == 0 || pixel_height == 0 || pixels > MAX_IMAGE_PIXELS {
                return Err("Watermark image exceeds the 40 million pixel safety limit".into());
            }
            let decoded = ::image::open(&image_path)
                .map_err(|error| format!("Cannot decode watermark image: {error}"))?
                .into_rgba8();
            let image = RawImage {
                pixels: RawImageData::U8(decoded.into_raw()),
                width: pixel_width as usize,
                height: pixel_height as usize,
                data_format: RawImageFormat::RGBA8,
                tag: Vec::new(),
            };
            let mut document = PdfDocument::new("Arcade Box PDF image watermark");
            let image_id = document.add_image(&image);
            let gs = document.add_graphics_state(
                ExtendedGraphicsState::default().with_current_fill_alpha(opacity as f32),
            );
            let mut pages = Vec::with_capacity(page_count as usize);
            for (index, (width, height)) in page_sizes.iter().copied().enumerate() {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("PDF watermarking cancelled".into());
                }
                let mut ops = Vec::new();
                if selected_pages[index] {
                    let (x, y, draw_width, draw_height) = watermark_image_position(
                        pixel_width,
                        pixel_height,
                        width as f32,
                        height as f32,
                        position,
                    )?;
                    ops.extend([
                        Op::SaveGraphicsState,
                        Op::LoadGraphicsState { gs: gs.clone() },
                        Op::UseXobject {
                            id: image_id.clone(),
                            transform: XObjectTransform {
                                translate_x: Some(Pt(x)),
                                translate_y: Some(Pt(y)),
                                scale_x: Some(draw_width / pixel_width as f32),
                                scale_y: Some(draw_height / pixel_height as f32),
                                dpi: Some(72.0),
                                ..Default::default()
                            },
                        },
                        Op::RestoreGraphicsState,
                    ]);
                }
                pages.push(PdfPage::new(
                    Mm(points_to_mm(width)),
                    Mm(points_to_mm(height)),
                    ops,
                ));
            }
            document.with_pages(pages);
            let mut warnings = Vec::new();
            let bytes = document.save(&PdfSaveOptions::default(), &mut warnings);
            write_private(&overlay_pdf, &bytes)?;
        }
        "pdf-overlay" => {
            let [_, overlay_input] = request.inputs.as_slice() else {
                return Err(
                    "PDF overlay mode needs the source PDF first, then one overlay PDF".into(),
                );
            };
            if overlay_input.mime != "file/pdf" {
                return Err("The second selected file must be an overlay PDF".into());
            }
            let overlay = grants
                .resolve(&overlay_input.value)
                .map_err(|error| error.to_string())?;
            let range = pages_to_qpdf_range(&selected_pages);
            run_qpdf(
                &provider.executable_path,
                vec![
                    source.as_os_str().to_os_string(),
                    "--overlay".into(),
                    overlay.as_os_str().to_os_string(),
                    format!("--to={range}").into(),
                    "--from=".into(),
                    "--repeat=1".into(),
                    "--".into(),
                    staged.as_os_str().to_os_string(),
                ],
                temp.path(),
                cancelled,
                "stamp the PDF with the selected overlay",
            )?;
        }
        _ => return Err("Choose text, image, or PDF overlay watermark mode".into()),
    }
    if mode != "pdf-overlay" {
        run_qpdf(
            &provider.executable_path,
            vec![
                source.as_os_str().to_os_string(),
                "--overlay".into(),
                overlay_pdf.as_os_str().to_os_string(),
                "--".into(),
                staged.as_os_str().to_os_string(),
            ],
            temp.path(),
            cancelled,
            "overlay the configured watermark on the PDF",
        )?;
    }
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save stamped PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(manifest, selected, "Applied the configured PDF watermark");
    if mode == "pdf-overlay" {
        result.warnings.push("PDF overlay mode uses the source overlay page's existing artwork and appearance. Use image or text mode to control opacity and position.".into());
    }
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    result.metadata.insert("watermarkMode".into(), json!(mode));
    result
        .metadata
        .insert("pageCount".into(), json!(page_count));
    result.metadata.insert(
        "pagesMarked".into(),
        json!(selected_pages.iter().filter(|marked| **marked).count()),
    );
    if mode != "pdf-overlay" {
        result.metadata.insert("opacity".into(), json!(opacity));
        result.metadata.insert("position".into(), json!(position));
    }
    Ok(result)
}

fn points_to_mm(points: f64) -> f32 {
    (points * 25.4 / 72.0) as f32
}

fn watermark_opacity(request: &ToolRequest) -> Result<f64, String> {
    let percent = request
        .options
        .get("opacity")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(45.0);
    if !percent.is_finite() || !(1.0..=100.0).contains(&percent) {
        return Err("Opacity must be between 1% and 100%".into());
    }
    Ok(percent / 100.0)
}

fn watermark_page_selection(request: &ToolRequest, page_count: u32) -> Result<Vec<bool>, String> {
    let requested = request
        .options
        .get("pageRanges")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if requested.is_empty() || requested.eq_ignore_ascii_case("all") {
        return Ok(vec![true; page_count as usize]);
    }
    if requested.len() > 4096 {
        return Err("Page ranges must be no longer than 4096 characters".into());
    }
    let mut selected = vec![false; page_count as usize];
    for part in requested.split(',').map(str::trim) {
        if part.is_empty() {
            return Err("Use page ranges such as 1-3,5".into());
        }
        let mut bounds = part.split('-');
        let start = bounds
            .next()
            .unwrap_or_default()
            .trim()
            .parse::<u32>()
            .map_err(|_| "Use numeric page ranges such as 1-3,5")?;
        let end_text = bounds.next();
        if bounds.next().is_some() || start == 0 {
            return Err("Use numeric page ranges such as 1-3,5".into());
        }
        let end = match end_text {
            Some(text) if !text.trim().is_empty() => text
                .trim()
                .parse::<u32>()
                .map_err(|_| "Use numeric page ranges such as 1-3,5")?,
            Some(_) => page_count,
            None => start,
        };
        if end < start || end > page_count {
            return Err(format!(
                "Page range {part} must stay within pages 1 to {page_count}"
            ));
        }
        for page in start..=end {
            selected[(page - 1) as usize] = true;
        }
    }
    if !selected.iter().any(|marked| *marked) {
        return Err("Select at least one page to watermark".into());
    }
    Ok(selected)
}

fn pages_to_qpdf_range(selected: &[bool]) -> String {
    let mut ranges = Vec::new();
    let mut start = None;
    let mut last = 0usize;
    for (index, marked) in selected.iter().copied().enumerate() {
        let page = index + 1;
        if marked {
            if start.is_none() {
                start = Some(page);
            }
            last = page;
        } else if let Some(first) = start.take() {
            ranges.push(if first == last {
                first.to_string()
            } else {
                format!("{first}-{last}")
            });
        }
    }
    if let Some(first) = start {
        ranges.push(if first == last {
            first.to_string()
        } else {
            format!("{first}-{last}")
        });
    }
    ranges.join(",")
}

fn watermark_text_position(
    text: &str,
    font_size: f32,
    page_width: f32,
    page_height: f32,
    position: &str,
) -> Result<(f32, f32), String> {
    let width = (text.chars().count() as f32 * font_size * 0.56).min(page_width);
    let margin = 24.0f32.min((page_width.min(page_height) / 8.0).max(0.0));
    let x = match position {
        "top-left" | "bottom-left" => margin,
        "top-center" | "bottom-center" | "center" => ((page_width - width) / 2.0).max(0.0),
        "top-right" | "bottom-right" => (page_width - width - margin).max(0.0),
        _ => return Err("Choose a valid watermark position".into()),
    };
    let y = match position {
        "top-left" | "top-center" | "top-right" => (page_height - margin - font_size).max(0.0),
        "bottom-left" | "bottom-center" | "bottom-right" => margin,
        "center" => ((page_height - font_size) / 2.0).max(0.0),
        _ => return Err("Choose a valid watermark position".into()),
    };
    Ok((x, y))
}

fn watermark_image_position(
    image_width: u32,
    image_height: u32,
    page_width: f32,
    page_height: f32,
    position: &str,
) -> Result<(f32, f32, f32, f32), String> {
    if image_width == 0 || image_height == 0 || page_width <= 0.0 || page_height <= 0.0 {
        return Err("Image and page dimensions must be non-zero".into());
    }
    let margin = 24.0f32.min((page_width.min(page_height) / 8.0).max(0.0));
    let max_width = (page_width * 0.35).min(page_width - 2.0 * margin).max(1.0);
    let max_height = (page_height * 0.2).min(page_height - 2.0 * margin).max(1.0);
    let scale = (max_width / image_width as f32).min(max_height / image_height as f32);
    let draw_width = image_width as f32 * scale;
    let draw_height = image_height as f32 * scale;
    let x = match position {
        "top-left" | "bottom-left" => margin,
        "top-center" | "bottom-center" | "center" => ((page_width - draw_width) / 2.0).max(0.0),
        "top-right" | "bottom-right" => (page_width - draw_width - margin).max(0.0),
        _ => return Err("Choose a valid watermark position".into()),
    };
    let y = match position {
        "top-left" | "top-center" | "top-right" => (page_height - draw_height - margin).max(0.0),
        "bottom-left" | "bottom-center" | "bottom-right" => margin,
        "center" => ((page_height - draw_height) / 2.0).max(0.0),
        _ => return Err("Choose a valid watermark position".into()),
    };
    Ok((x, y, draw_width, draw_height))
}

fn pdf_page_sizes(
    source: &Path,
    page_count: u32,
    cancelled: &AtomicBool,
) -> Result<Vec<(f64, f64)>, String> {
    let provider = poppler_provider("pdf.info")?;
    let output = process::run(
        &ProcessSpec {
            executable: provider.executable_path,
            args: vec![
                "-f".into(),
                "1".into(),
                "-l".into(),
                page_count.to_string().into(),
                "-box".into(),
                source.as_os_str().to_os_string(),
            ],
            current_dir: None,
            timeout: Duration::from_secs(30),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| format!("Could not inspect PDF page sizes: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Poppler could not inspect PDF page sizes: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    let mut sizes = vec![None; page_count as usize];
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(rest) = line.strip_prefix("Page ") else {
            continue;
        };
        let Some((page_text, values)) = rest.split_once(" size:") else {
            continue;
        };
        let Ok(page) = page_text.trim().parse::<usize>() else {
            continue;
        };
        let Some((width, height)) = values
            .trim()
            .split_once(" x ")
            .and_then(|(width, rest)| rest.split_once(" pts").map(|(height, _)| (width, height)))
        else {
            continue;
        };
        let (Ok(width), Ok(height)) = (width.trim().parse::<f64>(), height.trim().parse::<f64>())
        else {
            continue;
        };
        if (1..=page_count as usize).contains(&page) {
            sizes[page - 1] = Some((width, height));
        }
    }
    sizes
        .into_iter()
        .enumerate()
        .map(|(index, size)| {
            size.ok_or_else(|| {
                format!(
                    "Poppler did not report dimensions for PDF page {}",
                    index + 1
                )
            })
        })
        .collect()
}

fn protect(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let action = request
        .options
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("protect");
    let name = output_name(
        request,
        &source,
        if action == "unlock" {
            "-unlocked.pdf"
        } else {
            "-protected.pdf"
        },
    )?;
    let provider = qpdf_provider("pdf:structural")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-secure-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private PDF workspace: {error}"))?;
    let staged = temp.path().join("secured.pdf");
    let mut args = vec![source.as_os_str().to_os_string()];
    match action {
        "protect" => {
            let user_password = required_password(request, "userPassword")?;
            // Without an owner password, a random one still enforces the
            // permissions below; nobody needs to remember it.
            let generated;
            let owner_password = match request
                .options
                .get("ownerPassword")
                .and_then(serde_json::Value::as_str)
            {
                Some(value) if !value.is_empty() => required_password(request, "ownerPassword")?,
                _ => {
                    use rand::{Rng, distr::Alphanumeric};
                    generated = rand::rng()
                        .sample_iter(&Alphanumeric)
                        .take(40)
                        .map(char::from)
                        .collect::<String>();
                    generated.as_str()
                }
            };
            if user_password == owner_password {
                return Err(
                    "Use different user and owner passwords for safer PDF encryption".into(),
                );
            }
            args.extend([
                "--encrypt".into(),
                format!("--user-password={user_password}").into(),
                format!("--owner-password={owner_password}").into(),
                "--bits=256".into(),
            ]);
            let print = request
                .options
                .get("printing")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("full");
            let modify = request
                .options
                .get("modification")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("all");
            if !["none", "low", "full"].contains(&print)
                || !["none", "assembly", "form", "annotate", "all"].contains(&modify)
            {
                return Err("Choose a supported print and modification permission".into());
            }
            args.push(format!("--print={print}").into());
            args.push(format!("--modify={modify}").into());
            args.extend(["--extract=y".into(), "--".into()]);
        }
        "unlock" => {
            let password = required_password(request, "password")?;
            let password_file = temp.path().join("input-password");
            write_private(&password_file, password.as_bytes())?;
            args.push(format!("--password-file={}", password_file.display()).into());
            args.push("--decrypt".into());
            args.push("--".into());
        }
        _ => return Err("Choose Protect or Unlock".into()),
    }
    args.extend([staged.as_os_str().to_os_string()]);
    let args_path = temp.path().join("qpdf-arguments");
    let mut bytes = Vec::new();
    for argument in &args {
        let value = argument
            .to_str()
            .ok_or("PDF arguments must be valid UTF-8")?;
        if value.contains(['\r', '\n', '\0']) {
            return Err("PDF arguments may not contain line breaks".into());
        }
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(b'\n');
    }
    write_private(&args_path, &bytes)?;
    let output = run_qpdf(
        &provider.executable_path,
        vec![OsString::from(format!("@{}", args_path.display()))],
        temp.path(),
        cancelled,
        "protect or unlock the PDF",
    )?;
    let exit = output.exit;
    if exit == -1 || !staged.is_file() {
        return Err("qpdf did not create the requested PDF output".into());
    }
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save protected PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(
        manifest,
        selected,
        if action == "unlock" {
            "Removed PDF encryption using the supplied valid password"
        } else {
            "Encrypted PDF with 256-bit AES"
        },
    );
    result.warnings.push("PDF permission restrictions depend on reader support and are not a substitute for file-level access control.".into());
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    Ok(result)
}

fn convert_document(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let [input] = request.inputs.as_slice() else {
        return Err("Select one document through Arcade Box".into());
    };
    if input.kind != ValueKind::Artifact || input.mime != "file/document" {
        return Err("Select a supported document file through Arcade Box".into());
    }
    let source = grants
        .resolve(&input.value)
        .map_err(|error| error.to_string())?;
    let parent = source
        .parent()
        .ok_or("Selected document has no parent folder")?;
    let provider = discover_libreoffice()
        .into_iter()
        .find(|provider| provider.compatible)
        .ok_or("Converting documents to PDF needs LibreOffice (the `soffice` command). Install it, then try again.")?;
    let name = output_name(request, &source, ".pdf")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-office-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private conversion workspace: {error}"))?;
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .ok_or("The selected document has no extension")?;
    let staged_input = temp.path().join(format!("input.{extension}"));
    link_or_copy_pdf(source.as_path(), &staged_input, cancelled)?;
    let profile_url = url::Url::from_directory_path(temp.path().join("profile"))
        .map_err(|_| "Cannot make a private LibreOffice profile")?;
    let args = vec![
        "--headless".into(),
        format!("-env:UserInstallation={profile_url}").into(),
        "--convert-to".into(),
        "pdf".into(),
        "--outdir".into(),
        temp.path().as_os_str().to_os_string(),
        staged_input.as_os_str().to_os_string(),
    ];
    run_provider(
        &provider.executable_path,
        args,
        Some(temp.path()),
        cancelled,
        "convert the document to PDF",
        2 * 1024 * 1024,
    )?;
    let staged = temp.path().join(format!("input.pdf"));
    if !staged.is_file() {
        return Err("LibreOffice finished without creating a PDF".into());
    }
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save converted PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(
        manifest,
        selected,
        "Converted the document using system LibreOffice",
    );
    result.warnings.push("Formatting fidelity depends on LibreOffice and the source document. Review the output before relying on complex layout conversion.".into());
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(provider.version));
    Ok(result)
}

fn selected_pdf(request: &ToolRequest, grants: &FileGrants) -> Result<PathBuf, String> {
    let [input] = request.inputs.as_slice() else {
        return Err("Select one PDF through Arcade Box".into());
    };
    if input.kind != ValueKind::Artifact || input.mime != "file/pdf" {
        return Err("The source must be a PDF selected through Arcade Box".into());
    }
    grants
        .resolve(&input.value)
        .map_err(|error| error.to_string())
}

fn success_file(
    manifest: &ToolManifest,
    selected: crate::grants::SelectedFile,
    message: &str,
) -> ToolResult {
    ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![selected.as_tool_value()],
        message: Some(format!("{message}: {}", selected.name)),
        warnings: vec![],
        metadata: Default::default(),
    }
}

fn validate_pdf_output_name(name: &str) -> Result<(), String> {
    validate_portable_filename(name)?;
    if !name.to_ascii_lowercase().ends_with(".pdf") {
        return Err("Output filename must end in .pdf".into());
    }
    Ok(())
}

fn validate_qpdf_range(range: &str) -> Result<(), String> {
    if range.is_empty()
        || range.len() > 512
        || !range
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'-' | b':'))
    {
        return Err("Enter a page selection such as 1-3,5 or r3-r1".into());
    }
    Ok(())
}

struct PageRotationRule {
    angle: u16,
    pages: String,
}

fn parse_pdf_page_rotations(
    specification: &str,
    page_count: u64,
) -> Result<Vec<PageRotationRule>, String> {
    const MAX_RULES: usize = 100;
    const MAX_SELECTED_PAGES: usize = 10_000;
    if specification.len() > 2048 {
        return Err("Per-page rotation instructions are too long".into());
    }
    let groups = specification.split(';').collect::<Vec<_>>();
    if groups.len() > MAX_RULES {
        return Err("Use no more than 100 per-page rotation rules".into());
    }
    let mut used_pages = BTreeSet::new();
    let mut rules = Vec::with_capacity(groups.len());
    for group in groups {
        let (page_spec, angle_spec) = group
            .trim()
            .split_once('=')
            .ok_or("Use per-page rotation rules such as 1-2=90;4=180")?;
        let page_spec = page_spec.trim();
        let angle = angle_spec
            .trim()
            .parse::<u16>()
            .map_err(|_| "Page rotation must be 90, 180, or 270 degrees")?;
        if ![90, 180, 270].contains(&angle) {
            return Err("Page rotation must be 90, 180, or 270 degrees".into());
        }
        if page_spec.is_empty()
            || !page_spec
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b',' | b'-'))
        {
            return Err("Per-page rules accept numbered pages and ranges only".into());
        }
        let mut normalized_ranges = Vec::new();
        for term in page_spec.split(',') {
            let (start, end) = match term.split_once('-') {
                Some((start, end)) if !end.contains('-') => (
                    start.parse::<u64>().map_err(|_| "Invalid page number")?,
                    end.parse::<u64>().map_err(|_| "Invalid page number")?,
                ),
                Some(_) => return Err("Use numbered page ranges such as 1-3".into()),
                None => {
                    let page = term.parse::<u64>().map_err(|_| "Invalid page number")?;
                    (page, page)
                }
            };
            if start == 0 || start > end || end > page_count {
                return Err(format!(
                    "Per-page rotation range {term} is outside the selected output pages (1-{page_count})"
                ));
            }
            for page in start..=end {
                if !used_pages.insert(page) {
                    return Err(format!(
                        "Output page {page} has more than one rotation rule"
                    ));
                }
                if used_pages.len() > MAX_SELECTED_PAGES {
                    return Err("Per-page rotation applies to more than 10000 output pages".into());
                }
            }
            if start == end {
                normalized_ranges.push(start.to_string());
            } else {
                normalized_ranges.push(format!("{start}-{end}"));
            }
        }
        rules.push(PageRotationRule {
            angle,
            pages: normalized_ranges.join(","),
        });
    }
    Ok(rules)
}

fn qpdf_page_count(
    executable: &Path,
    source: &Path,
    cancelled: &AtomicBool,
) -> Result<u64, String> {
    let output = process::run(
        &ProcessSpec {
            executable: executable.to_path_buf(),
            args: vec!["--show-npages".into(), source.as_os_str().to_os_string()],
            current_dir: source.parent().map(Path::to_path_buf),
            timeout: Duration::from_secs(30),
            output_limit: 64 * 1024,
        },
        cancelled,
    )
    .map_err(|error| format!("Could not count selected PDF pages: {error}"))?;
    if !matches!(output.status.code(), Some(0 | 3)) {
        return Err(format!(
            "qpdf could not count selected pages: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    let count = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .map_err(|_| "qpdf returned an invalid selected-page count")?;
    if count == 0 || count > 100_000 {
        return Err("The selected PDF page count is outside the supported range".into());
    }
    Ok(count)
}

fn poppler_provider(capability: &str) -> Result<crate::provider::ProviderInfo, String> {
    discover_poppler()
        .into_iter()
        .find(|provider| provider.compatible && provider.capability == capability)
        .ok_or_else(|| format!("No compatible Poppler provider for {capability} was found"))
}

fn run_provider(
    executable: &Path,
    args: Vec<OsString>,
    working_directory: Option<&Path>,
    cancelled: &AtomicBool,
    operation: &str,
    output_limit: usize,
) -> Result<process::ProcessOutput, String> {
    let current_dir = working_directory.map(Path::to_path_buf);
    process::run(
        &ProcessSpec {
            executable: executable.to_path_buf(),
            args,
            current_dir,
            timeout: Duration::from_secs(2 * 3600),
            output_limit,
        },
        cancelled,
    )
    .map_err(|error| format!("Could not {operation}: {error}"))
}

fn pdf_page_count(source: &Path, cancelled: &AtomicBool) -> Result<u64, String> {
    let provider = poppler_provider("pdf.info")?;
    let output = process::run(
        &ProcessSpec {
            executable: provider.executable_path,
            args: vec![source.as_os_str().to_os_string()],
            current_dir: None,
            timeout: Duration::from_secs(30),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Poppler could not inspect this PDF: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            line.strip_prefix("Pages:")
                .and_then(|value| value.trim().parse::<u64>().ok())
        })
        .ok_or_else(|| "Poppler did not report a valid page count".into())
}

fn required_password<'a>(request: &'a ToolRequest, key: &str) -> Result<&'a str, String> {
    let value = request
        .options
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("Enter {key}"))?;
    if value.is_empty() || value.len() > 4096 || value.contains(['\r', '\n', '\0']) {
        return Err("Password must contain 1 to 4096 characters and no line breaks".into());
    }
    Ok(value)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())
}

fn link_or_copy_pdf(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if fs::hard_link(source, destination).is_ok() {
        return Ok(());
    }
    let mut input = File::open(source).map_err(|error| error.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("PDF job cancelled".into());
        }
        let count =
            std::io::Read::read(&mut input, &mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
    }
    output.flush().map_err(|error| error.to_string())
}

fn qpdf_provider(capability: &str) -> Result<crate::provider::ProviderInfo, String> {
    discover_qpdf(None)
        .into_iter()
        .find(|provider| {
            provider.compatible
                && provider
                    .capabilities
                    .iter()
                    .any(|item| item == capability)
        })
        .ok_or_else(|| {
            "No compatible qpdf provider was found. Install qpdf or configure a PDF provider in Engines & Dependencies.".into()
        })
}

fn integer_option(value: Option<&serde_json::Value>) -> Option<i64> {
    value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })
}

fn output_name(request: &ToolRequest, source: &Path, suffix: &str) -> Result<String, String> {
    if request
        .options
        .get("outputName")
        .is_some_and(|value| !value.is_null() && !value.is_string())
    {
        return Err("Output filename must be text".into());
    }
    let explicit_name = request
        .options
        .get("outputName")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let name = explicit_name.unwrap_or_else(|| format!("{}{suffix}", safe_stem(source)));
    validate_portable_filename(&name)?;
    if !name.to_ascii_lowercase().ends_with(".pdf") {
        return Err("The output filename must end in .pdf".into());
    }
    Ok(name)
}

fn safe_stem(source: &Path) -> String {
    let raw = source.file_stem().unwrap_or_default().to_string_lossy();
    let mut stem = String::new();
    for ch in raw.chars() {
        let safe = if ch.is_control()
            || matches!(ch, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
        {
            '_'
        } else {
            ch
        };
        if stem.len() + safe.len_utf8() > 100 {
            break;
        }
        stem.push(safe);
    }
    let stem = stem.trim_matches([' ', '.']);
    if stem.is_empty() {
        "document".into()
    } else {
        stem.into()
    }
}

fn requested_page_range(request: &ToolRequest) -> Result<String, String> {
    let Some(value) = request.options.get("range") else {
        return Ok("1-z".into());
    };
    if value.is_null() {
        return Ok("1-z".into());
    }
    let Some(range) = value.as_str() else {
        return Err("Page range must be text, for example 1-3,5 or r3-r1".into());
    };
    if range.is_empty()
        || range.len() > 512
        || !range
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'-' | b':'))
    {
        return Err("Page range must use qpdf syntax, for example 1-3,5 or r3-r1".into());
    }
    Ok(range.to_owned())
}

fn split_with_provider(
    operation: &PdfOperationContext<'_>,
    source: &Path,
    output_name: &str,
    range: &str,
    pages_per_file: Option<u64>,
) -> Result<ToolResult, String> {
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(operation.output_directory)
        .map_err(|error| format!("Cannot create a private output folder: {error}"))?;
    let selected = temp.path().join("selected.pdf");
    let mut file_argument = OsString::from("--file=");
    file_argument.push(source.as_os_str());
    let mut selection_args = vec![
        source.as_os_str().to_os_string(),
        "--pages".into(),
        file_argument,
    ];
    let mut range_argument = OsString::from("--range=");
    range_argument.push(range);
    selection_args.push(range_argument);
    selection_args.push("--".into());
    selection_args.push(selected.as_os_str().to_os_string());
    let selection = run_qpdf(
        operation.executable,
        selection_args,
        temp.path(),
        operation.cancelled,
        "extract the requested PDF pages",
    )?;
    if !selected.is_file() {
        return Err("qpdf finished without producing the selected pages".into());
    }
    let mut qpdf_warnings = Vec::new();
    if selection.exit == 3 {
        qpdf_warnings.push(format!(
            "qpdf recovered from input warnings: {}",
            concise(&String::from_utf8_lossy(&selection.stderr))
        ));
    }

    let selected_pages = page_count(
        operation.executable,
        &selected,
        temp.path(),
        operation.cancelled,
    )?;
    if selected_pages == 0 {
        return Err("The selected page range did not contain any pages".into());
    }
    let group_size = pages_per_file.unwrap_or(selected_pages).max(1);
    let group_count = selected_pages.div_ceil(group_size);
    if group_count > 10_000 {
        return Err(
            "This split would create more than 10000 files; increase pages per file".into(),
        );
    }

    let staged_files = if group_count == 1 {
        vec![selected]
    } else {
        let qpdf_base = temp.path().join("split.pdf");
        let mut split_flag = OsString::from("--split-pages=");
        split_flag.push(group_size.to_string());
        let split_output = run_qpdf(
            operation.executable,
            vec![
                split_flag,
                selected.as_os_str().to_os_string(),
                qpdf_base.as_os_str().to_os_string(),
            ],
            temp.path(),
            operation.cancelled,
            "split the selected PDF pages",
        )?;
        if split_output.exit == 3 {
            qpdf_warnings.push(format!(
                "qpdf reported warnings while splitting pages: {}",
                concise(&String::from_utf8_lossy(&split_output.stderr))
            ));
        }
        let entries = fs::read_dir(temp.path())
            .map_err(|error| format!("Cannot read staged PDF outputs: {error}"))?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| format!("Cannot inspect staged PDF output: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut paths = entries
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("split-"))
                    && path.extension().is_some_and(|extension| extension == "pdf")
            })
            .collect::<Vec<_>>();
        paths.sort();
        if paths.len() != group_count as usize || paths.iter().any(|path| !path.is_file()) {
            return Err(format!(
                "qpdf produced {} split files; expected {group_count}",
                paths.len()
            ));
        }
        paths
    };

    if operation.cancelled.load(Ordering::Relaxed) {
        return Err("PDF page extraction cancelled".into());
    }
    let output_stem = Path::new(output_name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let width = selected_pages.to_string().len();
    let mut published = Vec::with_capacity(staged_files.len());
    for (index, staged) in staged_files.iter().enumerate() {
        let name = if group_count == 1 {
            output_name.to_owned()
        } else {
            let first_page = index as u64 * group_size + 1;
            let last_page = (first_page + group_size - 1).min(selected_pages);
            format!("{output_stem}-{first_page:0width$}-{last_page:0width$}.pdf")
        };
        match publish_without_overwrite(
            staged,
            operation.output_directory,
            &name,
            operation.cancelled,
        ) {
            Ok(path) => published.push(path),
            Err(error) => {
                return Err(format!(
                    "Could not save extracted PDF pages: {error}{}",
                    preserved_outputs_note(&published)
                ));
            }
        }
    }
    let selected_outputs = match grant_outputs(operation.grants, &published) {
        Ok(outputs) => outputs,
        Err(error) => {
            return Err(format!(
                "Could not grant access to extracted PDF files: {error}{}",
                preserved_outputs_note(&published)
            ));
        }
    };
    let output_bytes = selected_outputs.iter().map(|file| file.size).sum::<u64>();
    let mut result = ToolResult {
        tool_id: operation.manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: selected_outputs
            .iter()
            .map(|file| file.as_tool_value())
            .collect(),
        message: Some(if group_count == 1 {
            format!(
                "Extracted {selected_pages} pages into {}",
                selected_outputs[0].name
            )
        } else {
            format!(
                "Split {selected_pages} pages into {} PDF files",
                selected_outputs.len()
            )
        }),
        warnings: qpdf_warnings,
        metadata: Default::default(),
    };
    if group_count > 1 {
        result.warnings.push("qpdf's split-pages mode does not preserve document-level outlines, threads, and similar features in each part.".into());
    }
    result.metadata.insert(
        "providerPath".into(),
        json!(operation.executable.display().to_string()),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(operation.version));
    result.metadata.insert("pageRange".into(), json!(range));
    result
        .metadata
        .insert("selectedPages".into(), json!(selected_pages));
    result
        .metadata
        .insert("pagesPerFile".into(), json!(pages_per_file));
    result
        .metadata
        .insert("outputBytes".into(), json!(output_bytes));
    result.metadata.insert(
        "outputNames".into(),
        json!(
            selected_outputs
                .iter()
                .map(|file| &file.name)
                .collect::<Vec<_>>()
        ),
    );
    Ok(result)
}

fn optimize_with_provider(
    operation: &PdfOperationContext<'_>,
    source: &Path,
    output_name: &str,
    mode: &str,
    jpeg_quality: Option<u32>,
) -> Result<ToolResult, String> {
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(operation.output_directory)
        .map_err(|error| format!("Cannot create a private output folder: {error}"))?;
    let staged = temp.path().join("optimized.pdf");
    let mut args: Vec<OsString> = vec![
        "--object-streams=generate".into(),
        "--recompress-flate".into(),
        "--compression-level=9".into(),
    ];
    if let Some(quality) = jpeg_quality {
        args.push("--optimize-images".into());
        args.push(format!("--jpeg-quality={quality}").into());
    }
    args.push(source.as_os_str().to_os_string());
    args.push(staged.as_os_str().to_os_string());
    let output = run_qpdf(
        operation.executable,
        args,
        temp.path(),
        operation.cancelled,
        "optimize the PDF structure",
    )?;
    if !staged.is_file() {
        return Err("qpdf finished without producing an optimized PDF".into());
    }
    if operation.cancelled.load(Ordering::Relaxed) {
        return Err("PDF optimization cancelled".into());
    }
    let input_bytes = fs::metadata(source)
        .map_err(|error| format!("Cannot inspect the selected PDF: {error}"))?
        .len();
    let staged_bytes = fs::metadata(&staged)
        .map_err(|error| format!("Cannot inspect the optimized PDF: {error}"))?
        .len();
    let final_path = publish_without_overwrite(
        &staged,
        operation.output_directory,
        output_name,
        operation.cancelled,
    )
    .map_err(|error| format!("Could not save optimized PDF: {error}"))?;
    let selected = match operation.grants.grant(&final_path) {
        Ok(selected) => selected,
        Err(error) => {
            return Err(format!(
                "Could not grant access to the optimized PDF: {error}. The complete file remains saved as {}.",
                final_path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    };
    let reduction_percent = if input_bytes == 0 {
        0.0
    } else {
        (input_bytes as f64 - staged_bytes as f64) * 100.0 / input_bytes as f64
    };
    let quality_description = jpeg_quality
        .map(|quality| format!(" at JPEG quality {quality}"))
        .unwrap_or_default();
    let mode_description = if jpeg_quality.is_some() {
        format!("Image-recompression optimization{quality_description}")
    } else {
        "Lossless structural optimization".into()
    };
    let mut result = ToolResult {
        tool_id: operation.manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![selected.as_tool_value()],
        message: Some(if staged_bytes < input_bytes {
            format!(
                "{mode_description} reduced the PDF from {} to {}",
                format_bytes(input_bytes),
                format_bytes(staged_bytes)
            )
        } else if staged_bytes == input_bytes {
            format!(
                "{mode_description} kept the PDF at {}",
                format_bytes(input_bytes)
            )
        } else {
            format!(
                "{mode_description} rewrote the PDF; output grew from {} to {}",
                format_bytes(input_bytes),
                format_bytes(staged_bytes)
            )
        }),
        warnings: if jpeg_quality.is_some() {
            vec!["JPEG recompression can reduce image quality and may introduce artifacts. qpdf does not downsample image resolution; review the output before replacing or sharing it.".into()]
        } else {
            vec!["This mode changes PDF structure only. It does not downsample or recompress image pixels, so it may not reduce image-heavy PDFs.".into()]
        },
        metadata: Default::default(),
    };
    result.warnings.push(
        "Object streams require PDF 1.5 or newer; very old PDF viewers may not support the generated file."
            .into(),
    );
    if output.exit == 3 {
        result.warnings.push(format!(
            "qpdf recovered from input warnings: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    result.metadata.insert(
        "providerPath".into(),
        json!(operation.executable.display().to_string()),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(operation.version));
    result
        .metadata
        .insert("optimizationMode".into(), json!(mode));
    if let Some(quality) = jpeg_quality {
        result.metadata.insert("jpegQuality".into(), json!(quality));
    }
    result
        .metadata
        .insert("inputBytes".into(), json!(input_bytes));
    result
        .metadata
        .insert("outputBytes".into(), json!(selected.size));
    result
        .metadata
        .insert("reductionPercent".into(), json!(reduction_percent));
    result
        .metadata
        .insert("outputName".into(), json!(selected.name));
    Ok(result)
}

struct QpdfOutput {
    exit: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_qpdf(
    executable: &Path,
    args: Vec<OsString>,
    working_directory: &Path,
    cancelled: &AtomicBool,
    operation: &str,
) -> Result<QpdfOutput, String> {
    let output = process::run(
        &ProcessSpec {
            executable: executable.to_path_buf(),
            args,
            current_dir: Some(working_directory.to_path_buf()),
            timeout: Duration::from_secs(2 * 3600),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    let exit = output.status.code().unwrap_or(-1);
    if !matches!(exit, 0 | 3) {
        return Err(format!(
            "qpdf could not {operation} (exit {exit}): {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    Ok(QpdfOutput {
        exit,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn page_count(
    executable: &Path,
    source: &Path,
    working_directory: &Path,
    cancelled: &AtomicBool,
) -> Result<u64, String> {
    let output = run_qpdf(
        executable,
        vec!["--show-npages".into(), source.as_os_str().to_os_string()],
        working_directory,
        cancelled,
        "count selected PDF pages",
    )?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .map_err(|_| "qpdf returned an invalid page count".into())
}

fn grant_outputs(
    grants: &FileGrants,
    paths: &[PathBuf],
) -> Result<Vec<crate::grants::SelectedFile>, String> {
    paths
        .iter()
        .map(|path| grants.grant(path).map_err(|error| error.to_string()))
        .collect()
}

fn preserved_outputs_note(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let names = paths
        .iter()
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy())
        .collect::<Vec<_>>()
        .join(", ");
    format!(" Complete output files already published were left in place: {names}.")
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let kib = bytes as f64 / KIB;
    if kib >= KIB {
        format!("{:.1} MiB", kib / KIB)
    } else {
        format!("{kib:.1} KiB")
    }
}

fn merge_with_provider(
    operation: &PdfOperationContext<'_>,
    sources: &[PathBuf],
    output_name: &str,
) -> Result<ToolResult, String> {
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(operation.output_directory)
        .map_err(|error| format!("Cannot create a private output folder: {error}"))?;
    let staged = temp.path().join("merged.pdf");
    // qpdf's primary input supplies document-level metadata and bookmarks.
    // Later PDFs supply pages; qpdf does not merge all outlines from them.
    let mut args: Vec<OsString> = vec![
        sources[0].as_os_str().to_os_string(),
        "--pages".into(),
        ".".into(),
    ];
    for source in &sources[1..] {
        let mut argument = OsString::from("--file=");
        argument.push(source.as_os_str());
        args.push(argument);
    }
    args.push("--".into());
    args.push(staged.as_os_str().to_os_string());
    let output = process::run(
        &ProcessSpec {
            executable: operation.executable.to_path_buf(),
            args,
            current_dir: Some(temp.path().to_path_buf()),
            timeout: Duration::from_secs(2 * 3600),
            output_limit: 1024 * 1024,
        },
        operation.cancelled,
    )
    .map_err(|error| error.to_string())?;
    let exit = output.status.code().unwrap_or(-1);
    if !matches!(exit, 0 | 3) || !staged.is_file() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "qpdf could not merge these PDFs (exit {exit}): {}",
            concise(&detail)
        ));
    }
    if operation.cancelled.load(Ordering::Relaxed) {
        return Err("PDF merge cancelled".into());
    }
    let final_path = publish_without_overwrite(
        &staged,
        operation.output_directory,
        output_name,
        operation.cancelled,
    )
    .map_err(|error| format!("Could not save merged PDF: {error}"))?;
    let selected = operation
        .grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = ToolResult {
        tool_id: operation.manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![selected.as_tool_value()],
        message: Some(format!(
            "Merged {} PDFs into {}",
            sources.len(),
            selected.name
        )),
        warnings: vec![],
        metadata: Default::default(),
    };
    if exit == 3 {
        result.warnings.push(format!(
            "qpdf recovered from input warnings: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    result.warnings.push("Bookmarks and document metadata from the first PDF are retained where qpdf supports them; bookmarks from later PDFs may not carry over.".into());
    result.metadata.insert(
        "providerPath".into(),
        json!(operation.executable.display().to_string()),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(operation.version));
    result
        .metadata
        .insert("outputName".into(), json!(selected.name));
    result
        .metadata
        .insert("outputBytes".into(), json!(selected.size));
    Ok(result)
}

fn concise(detail: &str) -> String {
    let condensed = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    condensed.chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};

    #[test]
    fn pdf_optimization_modes_set_explicit_quality_and_reject_bad_values() {
        assert_eq!(
            pdf_optimization_settings(&json!({})).unwrap(),
            ("lossless", None)
        );
        assert_eq!(
            pdf_optimization_settings(&json!({"mode":"balanced"})).unwrap(),
            ("balanced", Some(82))
        );
        assert_eq!(
            pdf_optimization_settings(&json!({"mode":"small"})).unwrap(),
            ("small", Some(60))
        );
        assert_eq!(
            pdf_optimization_settings(&json!({"mode":"custom","quality":42})).unwrap(),
            ("custom", Some(42))
        );
        assert!(pdf_optimization_settings(&json!({"mode":"custom","quality":29})).is_err());
        assert!(pdf_optimization_settings(&json!({"mode":"custom","quality":"82"})).is_err());
    }

    #[test]
    fn pdf_attachment_listing_sanitizes_names_and_requires_complete_indexes() {
        let entries = parse_pdf_attachment_listing(
            "2 embedded files\n1: ../../report.txt\n2: C:\\private\\item?.json\n",
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 1);
        assert_eq!(entries[0].name, "report.txt");
        assert_eq!(entries[1].name, "item_.json");
        assert!(parse_pdf_attachment_listing("2 embedded files\n1: only-one.bin\n").is_err());
        assert!(parse_pdf_attachment_listing("1 embedded file\n101: too-many.bin\n").is_err());
    }

    #[test]
    fn organizer_per_page_rotation_rules_validate_output_page_ranges() {
        let rules = parse_pdf_page_rotations("1-2=90;4,6-7=180", 8).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].pages, "1-2");
        assert_eq!(rules[0].angle, 90);
        assert_eq!(rules[1].pages, "4,6-7");
        assert!(parse_pdf_page_rotations("1-2=90;2=180", 8).is_err());
        assert!(parse_pdf_page_rotations("1=45", 8).is_err());
        assert!(parse_pdf_page_rotations("0=90", 8).is_err());
        assert!(parse_pdf_page_rotations("9=90", 8).is_err());
    }

    fn write_one_page_pdf(path: &Path) {
        write_pdf_with_pages(path, 1);
    }

    fn write_pdf_with_attachment(path: &Path, embedded_name: &str, payload: &[u8]) {
        let name = embedded_name
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let embedded_stream = format!(
            "<< /Type /EmbeddedFile /Length {} >>\nstream\n{}\nendstream",
            payload.len(),
            String::from_utf8_lossy(payload)
        );
        let objects = vec![
            format!(
                "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [({name}) 4 0 R] >> >> >>"
            ),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 7 0 R >>".into(),
            format!(
                "<< /Type /Filespec /F ({name}) /UF ({name}) /EF << /F 5 0 R >> >>"
            ),
            embedded_stream,
            "<< /Type /Filespec /F (unused) >>".into(),
            "<< /Length 0 >>\nstream\n\nendstream".into(),
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut offsets = vec![0u64];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len() as u64);
            bytes
                .extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }
        let xref_offset = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        fs::write(path, bytes).unwrap();
    }

    fn write_pdf_with_pages(path: &Path, page_count: usize) {
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        let pages_id = 3;
        let content_id = pages_id + page_count;
        let kids = (pages_id..content_id)
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        let bodies = std::iter::once("<< /Type /Catalog /Pages 2 0 R >>".to_string())
            .chain(std::iter::once(format!(
                "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
            )))
            .chain((pages_id..content_id).map(|_| {
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents {content_id} 0 R >>"
                )
            }))
            .chain(std::iter::once(
                "<< /Length 0 >>\nstream\n\nendstream".to_string(),
            ))
            .collect::<Vec<_>>();
        for (index, body) in bodies.iter().enumerate() {
            offsets.push(bytes.len());
            write!(&mut bytes, "{} 0 obj\n{}\nendobj\n", index + 1, body).unwrap();
        }
        let xref = bytes.len();
        write!(
            &mut bytes,
            "xref\n0 {}\n0000000000 65535 f \n",
            bodies.len() + 1
        )
        .unwrap();
        for offset in offsets.into_iter().skip(1) {
            writeln!(&mut bytes, "{offset:010} 00000 n ").unwrap();
        }
        write!(
            &mut bytes,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            bodies.len() + 1
        )
        .unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn write_pdf_with_rgb_image(path: &Path) {
        let contents = b"q 100 0 0 100 30 40 cm /Im0 Do Q\n";
        let bodies: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
            [
                format!("<< /Length {} >>\nstream\n", contents.len()).into_bytes(),
                contents.to_vec(),
                b"endstream".to_vec(),
            ]
            .concat(),
            [
                b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 3 >>\nstream\n".to_vec(),
                vec![255, 0, 0],
                b"\nendstream".to_vec(),
            ]
            .concat(),
        ];
        let mut bytes = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0u64];
        for (index, body) in bodies.iter().enumerate() {
            offsets.push(bytes.len() as u64);
            bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref = bytes.len();
        bytes.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
        );
        for offset in offsets.into_iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                bodies.len() + 1
            )
            .as_bytes(),
        );
        fs::write(path, bytes).unwrap();
    }

    fn installed_qpdf(capability: &str) -> Option<crate::provider::ProviderInfo> {
        discover_qpdf(None).into_iter().find(|provider| {
            provider.compatible && provider.capabilities.iter().any(|item| item == capability)
        })
    }

    #[test]
    fn images_to_pdf_checks_provider_status_and_publishes_a_real_pdf() {
        let provider_available = discover_img2pdf().into_iter().any(|provider| {
            provider.compatible && provider.capabilities.iter().any(|cap| cap == "pdf:create")
        });
        if !provider_available {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.png");
        ::image::RgbImage::from_pixel(32, 24, ::image::Rgb([20, 80, 180]))
            .save(&source)
            .unwrap();
        let runtime = crate::Arcade::in_memory().unwrap();
        let grants = runtime.grants();
        let selected = grants.grant(&source).unwrap();
        let manifest = runtime
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.pdf.images-to-pdf")
            .unwrap();
        let cancelled = AtomicBool::new(false);
        let result = execute(
            &manifest,
            &ToolRequest {
                tool_id: manifest.id.clone(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"pageSize":"A4","fit":"into"}),
            },
            grants,
            &cancelled,
        )
        .unwrap();
        assert_eq!(result.status, ResultStatus::Success, "{:?}", result.message);
        let output = grants.resolve(&result.outputs[0].value).unwrap();
        let mut signature = [0; 5];
        File::open(&output)
            .unwrap()
            .read_exact(&mut signature)
            .unwrap();
        assert_eq!(&signature, b"%PDF-");
        assert_eq!(result.metadata["imageCount"], 1);

        let unsupported = temp.path().join("unsupported.png");
        ::image::ImageBuffer::<::image::Rgba<u16>, Vec<u16>>::from_pixel(
            32,
            24,
            ::image::Rgba([50_000, 1_000, 2_000, 30_000]),
        )
        .save(&unsupported)
        .unwrap();
        let unsupported_grant = grants.grant(&unsupported).unwrap();
        let error = execute(
            &manifest,
            &ToolRequest {
                tool_id: manifest.id.clone(),
                inputs: vec![unsupported_grant.as_tool_value()],
                options: json!({"pageSize":"A4","fit":"into"}),
            },
            grants,
            &cancelled,
        )
        .unwrap_err();
        assert!(error.contains("img2pdf could not create a PDF"), "{error}");
        assert!(!temp.path().join("unsupported-images.pdf").exists());
    }

    #[test]
    fn installed_poppler_extracts_metadata_and_renders_pdf_pages() {
        let Some(render_provider) = discover_poppler()
            .into_iter()
            .find(|provider| provider.compatible && provider.capability == "pdf.render")
        else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.pdf");
        write_pdf_with_pages(&source, 2);
        let runtime = crate::Arcade::in_memory().unwrap();
        let grants = runtime.grants();
        let selected = grants.grant(&source).unwrap();
        let tools = runtime.list_tools();
        let render_manifest = tools
            .iter()
            .find(|tool| tool.id == "arcade.pdf.pdf-to-images")
            .unwrap();
        let render_result = execute(
            render_manifest,
            &ToolRequest {
                tool_id: render_manifest.id.clone(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"format":"png","dpi":72}),
            },
            grants,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            render_result.status,
            ResultStatus::Success,
            "{:?}",
            render_result.message
        );
        assert_eq!(render_result.outputs.len(), 2);
        for output in &render_result.outputs {
            let path = grants.resolve(&output.value).unwrap();
            assert!(path.is_file());
            assert!(path.extension().is_some_and(|extension| extension == "png"));
        }

        let extract_manifest = tools
            .iter()
            .find(|tool| tool.id == "arcade.pdf.extract")
            .unwrap();
        let extracted = execute(
            extract_manifest,
            &ToolRequest {
                tool_id: extract_manifest.id.clone(),
                inputs: vec![selected.as_tool_value()],
                options: json!({}),
            },
            grants,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            extracted.status,
            ResultStatus::Success,
            "{:?}",
            extracted.message
        );
        assert_eq!(extracted.outputs[0].mime, "structured/pdf-extraction");
        assert!(render_provider.executable_path.is_file());
    }

    #[test]
    fn installed_poppler_extracts_embedded_images_as_granted_files() {
        let Some(images_provider) = discover_poppler()
            .into_iter()
            .find(|provider| provider.compatible && provider.capability == "pdf.images")
        else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("image-source.pdf");
        write_pdf_with_rgb_image(&source);
        let runtime = crate::Arcade::in_memory().unwrap();
        let grants = runtime.grants();
        let selected = grants.grant(&source).unwrap();
        let manifest = runtime
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.pdf.extract")
            .unwrap();
        let result = execute(
            &manifest,
            &ToolRequest {
                tool_id: manifest.id.clone(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"includeImages": true}),
            },
            grants,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.status, ResultStatus::Success, "{:?}", result.message);
        assert_eq!(result.metadata["extractedImageCount"], 1);
        assert_eq!(result.outputs[0].mime, "structured/pdf-extraction");
        assert_eq!(result.outputs.len(), 2);
        assert!(matches!(
            result.outputs[1].mime.as_str(),
            "file/image" | "file/octet-stream"
        ));
        let extracted = grants.resolve(&result.outputs[1].value).unwrap();
        assert!(extracted.is_file());
        assert!(images_provider.executable_path.is_file());
    }

    #[test]
    fn installed_poppler_extracts_attachments_to_generated_non_overwriting_files() {
        if ["pdf.text", "pdf.info", "pdf.images", "pdf.attachments"]
            .iter()
            .any(|capability| poppler_provider(capability).is_err())
        {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.pdf");
        write_pdf_with_attachment(&source, "../../secret.txt", b"private payload\n");
        let existing = temp.path().join("source-attachment-001.bin");
        fs::write(&existing, b"keep existing file").unwrap();
        let runtime = crate::Arcade::in_memory().unwrap();
        let grants = runtime.grants();
        let selected = grants.grant(&source).unwrap();
        let manifest = runtime
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.pdf.extract")
            .unwrap();
        let result = execute(
            &manifest,
            &ToolRequest {
                tool_id: manifest.id.clone(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"includeImages": false, "includeAttachments": true}),
            },
            grants,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result.status, ResultStatus::Success, "{:?}", result.message);
        assert_eq!(result.metadata["attachmentCount"], 1);
        assert_eq!(result.metadata["extractedAttachmentCount"], 1);
        assert_eq!(result.outputs.len(), 2);
        assert_eq!(result.outputs[1].mime, "file/octet-stream");
        assert_eq!(fs::read(&existing).unwrap(), b"keep existing file");
        let extracted = grants.resolve(&result.outputs[1].value).unwrap();
        assert_eq!(
            extracted.file_name().unwrap().to_string_lossy(),
            "source-attachment-001-2.bin"
        );
        assert_eq!(fs::read(extracted).unwrap(), b"private payload\n");
        let summary: serde_json::Value = serde_json::from_str(&result.outputs[0].value).unwrap();
        assert_eq!(summary["attachments"][0]["name"], "secret.txt");
    }

    fn assert_page_count(provider: &crate::provider::ProviderInfo, path: &Path, expected: u64) {
        let pages = process::run(
            &ProcessSpec {
                executable: provider.executable_path.clone(),
                args: vec!["--show-npages".into(), path.as_os_str().to_os_string()],
                current_dir: None,
                timeout: Duration::from_secs(5),
                output_limit: 1024,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(pages.status.success());
        assert_eq!(
            String::from_utf8_lossy(&pages.stdout).trim(),
            expected.to_string()
        );
    }

    #[test]
    fn installed_qpdf_merges_selected_pages_without_replacing_sources() {
        let Some(provider) = installed_qpdf("pdf:merge") else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.pdf");
        let second = temp.path().join("second.pdf");
        write_one_page_pdf(&first);
        write_one_page_pdf(&second);
        let runtime = crate::Arcade::in_memory().unwrap();
        let grants = runtime.grants();
        let first_grant = grants.grant(&first).unwrap();
        let second_grant = grants.grant(&second).unwrap();
        let manifest = runtime
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.pdf.merge")
            .unwrap();
        let cancelled = AtomicBool::new(false);
        let operation = PdfOperationContext {
            manifest: &manifest,
            executable: &provider.executable_path,
            version: &provider.version,
            output_directory: temp.path(),
            grants,
            cancelled: &cancelled,
        };
        let result =
            merge_with_provider(&operation, &[first.clone(), second.clone()], "combined.pdf")
                .unwrap();
        let output = grants.resolve(&result.outputs[0].value).unwrap();
        assert_eq!(output.file_name().unwrap(), "combined.pdf");
        assert!(first.is_file() && second.is_file());
        assert_eq!(first_grant.mime, "file/pdf");
        assert_eq!(second_grant.mime, "file/pdf");
        assert_page_count(&provider, &output, 2);
    }

    #[test]
    fn installed_qpdf_extracts_ranges_and_splits_every_n_pages_safely() {
        let Some(provider) = installed_qpdf("pdf:split") else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.pdf");
        write_pdf_with_pages(&source, 8);
        fs::write(temp.path().join("part-1-2.pdf"), b"keep existing file").unwrap();
        let runtime = crate::Arcade::in_memory().unwrap();
        let grants = runtime.grants();
        let manifest = runtime
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.pdf.split")
            .unwrap();
        let active = AtomicBool::new(false);
        let operation = PdfOperationContext {
            manifest: &manifest,
            executable: &provider.executable_path,
            version: &provider.version,
            output_directory: temp.path(),
            grants,
            cancelled: &active,
        };
        let result = split_with_provider(&operation, &source, "part.pdf", "2-7", Some(2)).unwrap();
        assert_eq!(result.outputs.len(), 3);
        assert_eq!(result.metadata["selectedPages"], 6);
        assert_eq!(
            fs::read(temp.path().join("part-1-2.pdf")).unwrap(),
            b"keep existing file"
        );
        let names = result.metadata["outputNames"].as_array().unwrap();
        assert_eq!(names[0], "part-1-2-2.pdf");
        assert_eq!(names[1], "part-3-4.pdf");
        assert_eq!(names[2], "part-5-6.pdf");
        for (output, expected_pages) in result.outputs.iter().zip([2, 2, 2]) {
            let path = grants.resolve(&output.value).unwrap();
            assert_page_count(&provider, &path, expected_pages);
        }
        let extracted =
            split_with_provider(&operation, &source, "range.pdf", "5,2-3", None).unwrap();
        assert_eq!(extracted.outputs.len(), 1);
        assert_eq!(extracted.metadata["selectedPages"], 3);
        assert_page_count(
            &provider,
            &grants.resolve(&extracted.outputs[0].value).unwrap(),
            3,
        );
        assert!(source.is_file());
        let cancelled = AtomicBool::new(true);
        let cancelled_operation = PdfOperationContext {
            manifest: &manifest,
            executable: &provider.executable_path,
            version: &provider.version,
            output_directory: temp.path(),
            grants,
            cancelled: &cancelled,
        };
        let cancellation_result =
            split_with_provider(&cancelled_operation, &source, "cancelled.pdf", "1-2", None);
        assert!(cancellation_result.is_err());
        assert!(!temp.path().join("cancelled.pdf").exists());
    }

    #[test]
    fn installed_qpdf_lossless_optimize_preserves_pages_and_source() {
        let Some(provider) = installed_qpdf("pdf:structural") else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.pdf");
        write_pdf_with_pages(&source, 4);
        let original = fs::read(&source).unwrap();
        let runtime = crate::Arcade::in_memory().unwrap();
        let grants = runtime.grants();
        let manifest = runtime
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.pdf.compress")
            .unwrap();
        let cancelled = AtomicBool::new(false);
        let operation = PdfOperationContext {
            manifest: &manifest,
            executable: &provider.executable_path,
            version: &provider.version,
            output_directory: temp.path(),
            grants,
            cancelled: &cancelled,
        };
        let result =
            optimize_with_provider(&operation, &source, "optimized.pdf", "lossless", None).unwrap();
        let output = grants.resolve(&result.outputs[0].value).unwrap();
        assert_eq!(result.metadata["optimizationMode"], "lossless");
        assert_ne!(output, source);
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_page_count(&provider, &output, 4);
    }

    #[test]
    fn qpdf_page_ranges_are_bounded_and_cannot_contain_extra_arguments() {
        let request = ToolRequest {
            tool_id: "arcade.pdf.split".into(),
            inputs: vec![],
            options: json!({ "range": "r3-r1,1-20:even" }),
        };
        assert_eq!(requested_page_range(&request).unwrap(), "r3-r1,1-20:even");
        let invalid = ToolRequest {
            options: json!({ "range": "1-2 --replace-input" }),
            ..request
        };
        assert!(requested_page_range(&invalid).is_err());
    }

    #[test]
    fn organizer_rotation_accepts_the_select_control_string() {
        assert_eq!(integer_option(Some(&json!(90))), Some(90));
        assert_eq!(integer_option(Some(&json!("270"))), Some(270));
        assert_eq!(integer_option(Some(&json!("rotate"))), None);
    }

    #[test]
    fn default_output_stems_are_sanitized_but_explicit_names_are_strict() {
        let source = Path::new("report:notes.pdf");
        let request = ToolRequest {
            tool_id: "arcade.pdf.compress".into(),
            inputs: vec![],
            options: json!({}),
        };
        assert_eq!(
            output_name(&request, source, "-optimized.pdf").unwrap(),
            "report_notes-optimized.pdf"
        );
        let unicode_name = format!("{}.pdf", "🙂".repeat(80));
        let unicode_output =
            output_name(&request, Path::new(&unicode_name), "-optimized.pdf").unwrap();
        assert!(unicode_output.len() <= 120);
        let explicit = ToolRequest {
            options: json!({ "outputName": "report:notes.pdf" }),
            ..request
        };
        assert!(output_name(&explicit, source, "-optimized.pdf").is_err());
    }
}
