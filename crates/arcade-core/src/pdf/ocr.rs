//! Searchable PDF: Poppler renders each page, Tesseract writes an invisible
//! text layer, and qpdf lays that layer over the original pages. The pages
//! themselves are untouched, and pages that already have text are skipped.

use super::{
    concise, output_name, page_count, poppler_provider, qpdf_provider, run_provider, selected_pdf,
    success_file,
};
use crate::{
    artifacts::publish_without_overwrite, grants::FileGrants, provider::discover_tesseract,
    tool_kit::check_cancelled,
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use serde_json::json;
use std::{ffi::OsString, fs, sync::atomic::AtomicBool};

const MAX_PAGES: u64 = 2000;
/// Resolution pages are rendered at for recognition.
const DPI: &str = "300";

pub(super) fn searchable(
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
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+'))
    {
        return Err("Choose a valid OCR language code".into());
    }
    let tesseract = discover_tesseract()
        .into_iter()
        .find(|provider| provider.compatible)
        .ok_or("Searchable PDF needs Tesseract. Install it, or download it from Engines & Dependencies.")?;
    for code in language.split('+') {
        if !tesseract
            .capabilities
            .contains(&format!("ocr:language:{code}"))
        {
            return Err(format!("Tesseract has no `{code}` language data installed"));
        }
    }
    let render = poppler_provider("pdf.render")?;
    let text = poppler_provider("pdf.text")?;
    let qpdf = qpdf_provider("pdf:structural")?;
    let name = output_name(request, &source, "-searchable.pdf")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-ocr-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private OCR workspace: {error}"))?;
    let work = temp.path();
    let pages = page_count(&qpdf.executable_path, &source, work, cancelled)?;
    if pages == 0 || pages > MAX_PAGES {
        return Err(format!(
            "Searchable PDF handles 1 to {MAX_PAGES} pages; this PDF has {pages}"
        ));
    }

    // pdftotext ends every page with a form feed: one call finds them all.
    let existing = run_provider(
        &text.executable_path,
        vec![source.as_os_str().to_os_string(), "-".into()],
        Some(work),
        cancelled,
        "read the PDF's existing text",
        64 * 1024 * 1024,
    )?;
    let existing = String::from_utf8_lossy(&existing.stdout);
    let page_texts: Vec<&str> = existing.split('\u{c}').collect();
    let todo: Vec<u64> = (1..=pages)
        .filter(|page| {
            page_texts
                .get(*page as usize - 1)
                .is_none_or(|text| text.trim().is_empty())
        })
        .collect();
    if todo.is_empty() {
        return Err(
            "Every page already has selectable text, so there is nothing to recognize".into(),
        );
    }

    let mut list = String::new();
    for &page in &todo {
        check_cancelled(cancelled)?;
        let page_arg = OsString::from(page.to_string());
        let output = run_provider(
            &render.executable_path,
            vec![
                "-r".into(),
                DPI.into(),
                "-gray".into(),
                "-png".into(),
                "-singlefile".into(),
                "-f".into(),
                page_arg.clone(),
                "-l".into(),
                page_arg,
                source.as_os_str().to_os_string(),
                format!("page-{page}").into(),
            ],
            Some(work),
            cancelled,
            "render a PDF page for recognition",
            1024 * 1024,
        )?;
        let image = format!("page-{page}.png");
        if !output.status.success() || !work.join(&image).is_file() {
            return Err(format!(
                "Poppler could not render page {page}: {}",
                concise(&String::from_utf8_lossy(&output.stderr))
            ));
        }
        list.push_str(&image);
        list.push('\n');
    }
    fs::write(work.join("pages.txt"), list).map_err(|error| error.to_string())?;
    let output = run_provider(
        &tesseract.executable_path,
        vec![
            "pages.txt".into(),
            "layer".into(),
            "-l".into(),
            language.into(),
            "--dpi".into(),
            DPI.into(),
            "-c".into(),
            "textonly_pdf=1".into(),
            "pdf".into(),
        ],
        Some(work),
        cancelled,
        "recognize the PDF's text",
        4 * 1024 * 1024,
    )?;
    let layer = work.join("layer.pdf");
    if !output.status.success() || !layer.is_file() {
        return Err(format!(
            "Tesseract could not recognize the pages: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }

    let staged = work.join("searchable.pdf");
    let to = todo
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let output = run_provider(
        &qpdf.executable_path,
        vec![
            source.as_os_str().to_os_string(),
            "--overlay".into(),
            layer.as_os_str().to_os_string(),
            format!("--to={to}").into(),
            format!("--from=1-{}", todo.len()).into(),
            "--".into(),
            staged.as_os_str().to_os_string(),
        ],
        Some(work),
        cancelled,
        "add the text layer",
        1024 * 1024,
    )?;
    // qpdf exits 3 when it succeeded with warnings.
    if !matches!(output.status.code(), Some(0 | 3)) || !staged.is_file() {
        return Err(format!(
            "qpdf could not add the text layer: {}",
            concise(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save searchable PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(manifest, selected, "Created a searchable PDF");
    result.metadata.insert("language".into(), json!(language));
    result
        .metadata
        .insert("pagesRecognized".into(), json!(todo.len()));
    result
        .metadata
        .insert("pagesWithText".into(), json!(pages - todo.len() as u64));
    result.metadata.insert(
        "providerPath".into(),
        json!(tesseract.executable_path.display().to_string()),
    );
    Ok(result)
}
