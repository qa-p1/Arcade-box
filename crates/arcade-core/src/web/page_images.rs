//! Download every image a webpage references: `<img>` (largest `srcset`
//! candidate), `<picture>` sources, social preview images, and icons.
//! Static HTML only; images added later by JavaScript are not seen.

use super::{fetch_page, parse_tag, stage_dir, url_input};
use crate::{
    Arcade, network,
    process::{self, ProcessSpec},
    tool_kit::{check_cancelled, number_in, option_bool, publish_file, success},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use serde_json::json;
use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicBool},
    thread,
    time::Duration,
};

const PAGE_LIMIT: usize = 5 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 40 * 1024 * 1024;
const WORKERS: usize = 6;

pub(super) fn download(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<ToolResult, String> {
    let url = url_input(request)?;
    let limit =
        number_in(request, "limit", "Maximum images", Some(100.0), 1.0..=500.0)?.round() as usize;
    let skip_small = option_bool(request, "skipSmall", true);
    let stage = stage_dir(runtime.artifact_staging_root())?;
    let page = fetch_page(&url, PAGE_LIMIT, stage.path(), cancelled)?;
    let html = String::from_utf8_lossy(&page.body);
    let mut urls = image_urls(&html, &page.url);
    let found = urls.len();
    urls.truncate(limit);
    if urls.is_empty() {
        return Err("No images were found in this page's HTML. Images loaded later by JavaScript can't be seen.".into());
    }
    progress(0.05);
    let curl = network::curl_provider()?;
    let queue = Mutex::new(urls.iter().enumerate().collect::<Vec<_>>());
    let done = Mutex::new(Vec::new());
    let total = urls.len();
    thread::scope(|scope| {
        for _ in 0..WORKERS.min(total) {
            scope.spawn(|| {
                loop {
                    if check_cancelled(cancelled).is_err() {
                        return;
                    }
                    let Some((index, url)) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop()
                    else {
                        return;
                    };
                    let staged = stage.path().join(format!("image-{index:04}"));
                    let fetched =
                        fetch_image(&curl.executable_path, url, &staged, stage.path(), cancelled);
                    let mut done = done.lock().unwrap_or_else(|e| e.into_inner());
                    done.push((index, url.clone(), staged, fetched));
                    progress(0.05 + 0.85 * done.len() as f64 / total as f64);
                }
            });
        }
    });
    check_cancelled(cancelled)?;
    let mut results = done.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort_by_key(|(index, ..)| *index);
    let mut outputs = Vec::new();
    let mut report = Vec::new();
    let mut names = HashSet::new();
    let (mut failed, mut skipped) = (0, 0);
    for (_, url, staged, fetched) in results {
        let extension = match fetched {
            Ok(extension) => extension,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        let bytes = fs::metadata(&staged).map(|meta| meta.len()).unwrap_or(0);
        if skip_small && bytes < 4 * 1024 && extension != "svg" {
            skipped += 1;
            continue;
        }
        let name = unique_name(&file_name(&url, extension), &mut names);
        let value = publish_file(request, runtime, &staged, &name, cancelled)?;
        report.push(json!({"file": name, "bytes": bytes, "url": network::redact_url(&url)}));
        outputs.push(value);
    }
    if outputs.is_empty() {
        return Err(format!(
            "None of the {total} images could be saved ({failed} failed, {skipped} were tiny)"
        ));
    }
    let mut warnings = Vec::new();
    if found > total {
        warnings.push(format!(
            "The page lists {found} images; only the first {total} were downloaded."
        ));
    }
    if failed > 0 {
        warnings.push(format!(
            "{failed} image{} could not be downloaded.",
            if failed == 1 { "" } else { "s" }
        ));
    }
    if skipped > 0 {
        warnings.push(format!(
            "Skipped {skipped} tiny image{} (icons, spacers, trackers).",
            if skipped == 1 { "" } else { "s" }
        ));
    }
    let saved = outputs.len();
    outputs.push(ToolValue::text(
        json!({"images": report}).to_string(),
        "structured/web-images",
    ));
    progress(1.0);
    Ok(success(
        manifest,
        outputs,
        Some(format!(
            "Saved {saved} image{}",
            if saved == 1 { "" } else { "s" }
        )),
        warnings,
    ))
}

/// Download one image; returns its extension from the file's signature.
fn fetch_image(
    curl: &Path,
    url: &str,
    staged: &Path,
    dir: &Path,
    cancelled: &AtomicBool,
) -> Result<&'static str, String> {
    let mut args = network::curl_common_args();
    args.extend([
        "--location".into(),
        "--max-redirs".into(),
        "5".into(),
        "--fail".into(),
        "--max-filesize".into(),
        MAX_IMAGE_BYTES.to_string().into(),
        "--output".into(),
        staged.as_os_str().to_os_string(),
        "--".into(),
        OsString::from(url),
    ]);
    let output = process::run(
        &ProcessSpec {
            executable: curl.to_path_buf(),
            args,
            current_dir: Some(dir.to_path_buf()),
            timeout: Duration::from_secs(90),
            output_limit: 64 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("download failed".into());
    }
    let mut header = [0u8; 512];
    let count = fs::File::open(staged)
        .and_then(|mut file| std::io::Read::read(&mut file, &mut header))
        .map_err(|error| error.to_string())?;
    let header = &header[..count];
    if let Some(kind) = infer::get(header).filter(|kind| kind.mime_type().starts_with("image/")) {
        return Ok(match kind.mime_type() {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/avif" => "avif",
            "image/bmp" => "bmp",
            "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
            "image/heif" => "heic",
            "image/tiff" => "tiff",
            _ => "img",
        });
    }
    let text = String::from_utf8_lossy(header);
    if text.trim_start().starts_with("<svg") || (text.contains("<?xml") && text.contains("<svg")) {
        return Ok("svg");
    }
    Err("not an image".into())
}

/// Image URLs in document order, de-duplicated, without `data:` URIs.
fn image_urls(html: &str, base: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |raw: &str| {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with("data:") {
            return;
        }
        if let Some(url) = network::resolve_url(base, raw)
            && seen.insert(url.clone())
        {
            urls.push(url);
        }
    };
    let mut index = 0;
    while let Some(open) = html[index..].find('<').map(|offset| index + offset) {
        let Some(close) = html[open..].find('>').map(|offset| open + offset) else {
            break;
        };
        index = close + 1;
        let Some((tag, _, attributes)) = parse_tag(&html[open + 1..close]) else {
            continue;
        };
        match tag.as_str() {
            "img" | "source" => {
                // The largest srcset candidate, else src (or a lazy-loading attribute).
                if let Some(best) = attributes
                    .get("srcset")
                    .or_else(|| attributes.get("data-srcset"))
                    .and_then(|set| largest_candidate(set))
                {
                    push(&best);
                } else if let Some(src) = attributes
                    .get("data-src")
                    .or_else(|| attributes.get("data-lazy-src"))
                    .or_else(|| attributes.get("src"))
                {
                    push(src);
                }
            }
            "meta" => {
                let property = attributes
                    .get("property")
                    .or_else(|| attributes.get("name"))
                    .map(|value| value.to_ascii_lowercase());
                if matches!(
                    property.as_deref(),
                    Some("og:image" | "og:image:url" | "twitter:image")
                ) && let Some(content) = attributes.get("content")
                {
                    push(content);
                }
            }
            "link" => {
                let rel = attributes
                    .get("rel")
                    .map(|value| value.to_ascii_lowercase())
                    .unwrap_or_default();
                if (rel.contains("icon") || rel.contains("image_src"))
                    && let Some(href) = attributes.get("href")
                {
                    push(href);
                }
            }
            _ => {}
        }
    }
    urls
}

/// The URL with the largest `w` or `x` descriptor in a `srcset`.
fn largest_candidate(srcset: &str) -> Option<String> {
    srcset
        .split(',')
        .filter_map(|candidate| {
            let mut parts = candidate.split_whitespace();
            let url = parts.next()?;
            let size = parts
                .next()
                .and_then(|descriptor| descriptor.trim_end_matches(['w', 'x']).parse::<f64>().ok())
                .unwrap_or(1.0);
            Some((size, url.to_owned()))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, url)| url)
}

fn file_name(url: &str, extension: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    let last = path.rsplit('/').next().unwrap_or_default();
    let stem = last.rsplit_once('.').map_or(last, |(stem, _)| stem);
    let stem = stem
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .take(80)
        .collect::<String>();
    let stem = stem.trim_matches('_');
    format!(
        "{}.{extension}",
        if stem.is_empty() { "image" } else { stem }
    )
}

fn unique_name(name: &str, used: &mut HashSet<String>) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut candidate = name.to_owned();
    let mut number = 2;
    while !used.insert(candidate.to_lowercase()) {
        candidate = format!("{stem}-{number}.{extension}");
        number += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_images_with_best_srcset_and_metadata() {
        let html = r#"<html><head><meta property="og:image" content="/social.png"><link rel="icon" href="/favicon.ico"></head>
            <body><img src="a.jpg"><img srcset="small.jpg 320w, big.jpg 1280w" src="small.jpg">
            <img data-src="lazy.webp" src="data:image/gif;base64,AAAA"><img src="a.jpg"></body></html>"#;
        let urls = image_urls(html, "https://example.com/blog/post");
        assert_eq!(
            urls,
            vec![
                "https://example.com/social.png",
                "https://example.com/favicon.ico",
                "https://example.com/blog/a.jpg",
                "https://example.com/blog/big.jpg",
                "https://example.com/blog/lazy.webp",
            ]
        );
        assert_eq!(
            file_name("https://x.test/i/My Photo!.jpeg?w=2", "jpg"),
            "My_Photo.jpg"
        );
        let mut used = HashSet::new();
        assert_eq!(unique_name("a.jpg", &mut used), "a.jpg");
        assert_eq!(unique_name("A.jpg", &mut used), "A-2.jpg");
    }
}
