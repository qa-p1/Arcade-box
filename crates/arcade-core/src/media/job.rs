//! Shared execution context for media tools: the job handed to executors,
//! batch processing for single-file transforms, and staged publication.

use super::{
    MediaInput, attach_provider, base_result, publish_staged, requested_output_name, run_ffmpeg,
};
use crate::{grants::FileGrants, provider::ProviderInfo};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Most files a batch-capable tool accepts in one run.
pub(super) const MAX_BATCH: usize = 50;

pub(super) type Progress = Arc<dyn Fn(f64) + Send + Sync>;

/// Everything an executor needs besides its input paths.
pub(super) struct Job<'a> {
    pub(super) manifest: &'a ToolManifest,
    pub(super) provider: &'a ProviderInfo,
    pub(super) request: &'a ToolRequest,
    pub(super) grants: &'a FileGrants,
    pub(super) cancelled: &'a AtomicBool,
    pub(super) progress: Progress,
}

impl Job<'_> {
    pub(super) fn with_progress(&self, progress: Progress) -> Job<'_> {
        Job {
            manifest: self.manifest,
            provider: self.provider,
            request: self.request,
            grants: self.grants,
            cancelled: self.cancelled,
            progress,
        }
    }
}

/// Run a single-file tool over each selected file. One failure does not stop
/// the rest; it is reported beside the files that succeeded.
pub(super) fn batch(
    job: &Job,
    paths: &[PathBuf],
    single: fn(&Job, &[PathBuf]) -> Result<ToolResult, String>,
) -> Result<ToolResult, String> {
    // A fixed output name can't apply to many files; each gets its own name.
    let mut request = job.request.clone();
    if let Some(options) = request.options.as_object_mut() {
        options.remove("outputName");
    }
    let total = paths.len();
    let mut outputs = Vec::new();
    let mut names = Vec::new();
    let mut sizes = Vec::new();
    let mut warnings = Vec::new();
    let mut failures = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if job.cancelled.load(Ordering::Relaxed) {
            return Err(format!(
                "Cancelled after {} of {total} files; finished outputs were kept",
                outputs.len()
            ));
        }
        let progress = job.progress.clone();
        let item = Job {
            request: &request,
            ..job.with_progress(Arc::new(move |fraction| {
                progress((index as f64 + fraction) / total as f64)
            }))
        };
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        match single(&item, std::slice::from_ref(path)) {
            Ok(result) => {
                outputs.extend(result.outputs);
                names.push(
                    result
                        .metadata
                        .get("outputName")
                        .cloned()
                        .unwrap_or(json!(name)),
                );
                sizes.push(
                    result
                        .metadata
                        .get("outputBytes")
                        .cloned()
                        .unwrap_or(Value::Null),
                );
                warnings.extend(
                    result
                        .warnings
                        .into_iter()
                        .map(|warning| format!("{name}: {warning}")),
                );
            }
            Err(error) if job.cancelled.load(Ordering::Relaxed) => {
                return Err(format!(
                    "{error} (after {} of {total} files)",
                    outputs.len()
                ));
            }
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    if outputs.is_empty() {
        return Err(format!(
            "None of the {total} files could be processed. {}",
            failures.first().cloned().unwrap_or_default()
        ));
    }
    let message = if failures.is_empty() {
        format!("Processed all {total} files")
    } else {
        format!("Processed {} of {total} files", total - failures.len())
    };
    let mut result = base_result(job.manifest, outputs, &message);
    attach_provider(&mut result, job.provider);
    result.warnings = failures.into_iter().chain(warnings).collect();
    result
        .metadata
        .insert("outputNames".into(), Value::Array(names));
    result
        .metadata
        .insert("outputBytes".into(), Value::Array(sizes));
    Ok(result)
}

/// Run FFmpeg into a staged file, then publish it beside `source` without
/// replacing anything.
#[allow(clippy::too_many_arguments)]
pub(super) fn produce(
    job: &Job,
    source: &Path,
    stage: &Path,
    inputs: &[MediaInput],
    mut args: Vec<OsString>,
    extension: &str,
    suffix: &str,
    duration: Option<f64>,
    expect_mime: Option<&str>,
) -> Result<ToolResult, String> {
    let staged = stage.join(format!("output.{extension}"));
    args.extend(["-n".into(), staged.as_os_str().to_os_string()]);
    run_ffmpeg(
        &job.provider.executable_path,
        inputs,
        args,
        stage,
        duration,
        job.cancelled,
        job.progress.clone(),
        "error",
    )?;
    let name = requested_output_name(job.request, source, suffix, extension)?;
    publish_named(job, source, &staged, &name, expect_mime)
}

pub(super) fn publish_named(
    job: &Job,
    source: &Path,
    staged: &Path,
    name: &str,
    expect_mime: Option<&str>,
) -> Result<ToolResult, String> {
    let parent = source
        .parent()
        .ok_or("Source path has no parent directory")?;
    let (final_path, selected) = publish_staged(staged, parent, name, job.grants, job.cancelled)?;
    let output = selected.as_tool_value();
    if let Some(mime) = expect_mime
        && output.mime != mime
    {
        return Err(format!(
            "The saved output was identified as {}, but this action produces {mime}",
            output.mime
        ));
    }
    let file_name = final_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut result = base_result(job.manifest, vec![output], &format!("Saved {file_name}"));
    attach_provider(&mut result, job.provider);
    result
        .metadata
        .insert("outputName".into(), json!(file_name));
    result
        .metadata
        .insert("outputBytes".into(), json!(selected.size));
    Ok(result)
}

pub(super) fn output_name(result: &ToolResult) -> String {
    result
        .metadata
        .get("outputName")
        .and_then(Value::as_str)
        .unwrap_or("the result")
        .to_owned()
}

/// `1:02.5`-style time for messages.
pub(super) fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    let rest = format!("{rest:04.1}");
    if minutes >= 60.0 {
        format!(
            "{}:{:02}:{rest}",
            (minutes / 60.0).floor(),
            (minutes % 60.0) as u64
        )
    } else {
        format!("{minutes}:{rest}")
    }
}

pub(super) fn megabytes(bytes: u64) -> String {
    let value = bytes as f64 / (1024.0 * 1024.0);
    if value >= 100.0 {
        format!("{value:.0} MB")
    } else {
        format!("{value:.1} MB")
    }
}
