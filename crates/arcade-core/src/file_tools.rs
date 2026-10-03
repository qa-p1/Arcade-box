//! Local file operations. File data is streamed through scoped grants;
//! generated files are published from private staging without replace.

use crate::{
    Arcade,
    artifacts::validate_portable_filename,
    tool_kit::{check_cancelled, json_result, option_bool, option_str, success},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use cap_std::fs::Dir;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

const MAX_FOLDER_ENTRIES: usize = 100_000;
const COPY_BUFFER: usize = 1024 * 1024;

#[derive(Clone)]
struct ScannedEntry {
    root: usize,
    relative: PathBuf,
    display: String,
    size: u64,
    directory: bool,
}

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    match manifest.id.as_str() {
        "arcade.files.rename" => rename_files(manifest, request, runtime, cancelled),
        "arcade.files.duplicates" => duplicate_finder(manifest, request, runtime, cancelled),
        "arcade.files.compare" => compare_files(manifest, request, runtime, cancelled),
        "arcade.files.sizes" => folder_sizes(manifest, request, runtime, cancelled),
        _ => Err(format!(
            "No file executor is registered for {}",
            manifest.id
        )),
    }
}

fn rename_files(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let files = selected_files(request)?;
    if files.is_empty() || files.len() > 5000 {
        return Err("Select between 1 and 5,000 files".into());
    }
    let apply = option_bool(request, "apply", false);
    let mode = option_str(request, "mode", "replace");
    let target = option_str(request, "target", "portable");
    let find = option_str(request, "find", "");
    let replacement = option_str(request, "replacement", "");
    let prefix = option_str(request, "prefix", "");
    let suffix = option_str(request, "suffix", "");
    let pattern = if mode == "regex" && !find.is_empty() {
        Some(regex::Regex::new(find).map_err(|error| format!("Rename pattern: {error}"))?)
    } else {
        None
    };
    let number_width = files.len().to_string().len().max(2);
    let mut plans = Vec::with_capacity(files.len());
    let mut names = std::collections::HashSet::new();
    for (index, input) in files.iter().enumerate() {
        check_cancelled(cancelled)?;
        let path = runtime
            .grants()
            .resolve(&input.value)
            .map_err(|error| error.to_string())?;
        let original = path
            .file_name()
            .and_then(|part| part.to_str())
            .ok_or("A selected file has an unsupported filename")?;
        let desired = if mode == "safe" {
            sanitize_filename(original, target)
        } else {
            transform_filename(
                original,
                mode,
                find,
                replacement,
                prefix,
                suffix,
                (index + 1, number_width),
                pattern.as_ref(),
            )?
        };
        let sanitized = sanitize_filename(&desired, "portable");
        validate_portable_filename(&sanitized)?;
        let unique = unique_output_name(&sanitized, &mut names);
        plans.push((input, original.to_owned(), unique));
    }
    let plan_value = plans
        .iter()
        .map(|(_, original, renamed)| json!({"original":original,"renamed":renamed}))
        .collect::<Vec<_>>();
    if !apply {
        let changed = plans
            .iter()
            .filter(|(_, original, renamed)| original != renamed)
            .count();
        let summary = format!(
            "{changed} of {} names change. Turn on Create renamed copies to apply.",
            plans.len()
        );
        return Ok(json_result(
            manifest,
            json!({"preview":plan_value,"applied":false,"message":summary}),
            "structured/rename-plan",
        ));
    }
    let stage = private_stage(runtime)?;
    let mut output = Vec::with_capacity(plans.len() + 1);
    for (index, (input, _original, renamed)) in plans.iter().enumerate() {
        check_cancelled(cancelled)?;
        let staged = stage.path().join(format!("copy-{index:05}"));
        let mut source = runtime
            .grants()
            .open_scoped(&input.value)
            .map_err(|error| error.to_string())?;
        let mut target_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|error| error.to_string())?;
        copy_checked(&mut source, &mut target_file, cancelled)?;
        target_file.sync_all().map_err(|error| error.to_string())?;
        drop(target_file);
        output.push(publish(runtime, request, cancelled, &staged, renamed)?);
    }
    output.push(ToolValue::text(
        json!({"preview":plan_value,"applied":true}).to_string(),
        "structured/rename-plan",
    ));
    Ok(success(
        manifest,
        output,
        Some("Created renamed copies; the selected originals were preserved".into()),
        vec![],
    ))
}

fn transform_filename(
    name: &str,
    mode: &str,
    find: &str,
    replacement: &str,
    prefix: &str,
    suffix: &str,
    (number, width): (usize, usize),
    pattern: Option<&regex::Regex>,
) -> Result<String, String> {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|part| part.to_str())
        .unwrap_or(name);
    let extension = path.extension().and_then(|part| part.to_str());
    let changed = match mode {
        "replace" => {
            if find.is_empty() {
                stem.to_owned()
            } else {
                stem.replace(find, replacement)
            }
        }
        "regex" => pattern
            .ok_or("Enter a valid rename pattern")?
            .replace_all(stem, replacement)
            .into_owned(),
        "number" => format!("{prefix}{number:0width$}"),
        "prefix" => format!("{prefix}{stem}"),
        "suffix" => format!("{stem}{suffix}"),
        "lowercase" => stem.to_lowercase(),
        "uppercase" => stem.to_uppercase(),
        _ => return Err(format!("Unknown rename rule `{mode}`")),
    };
    Ok(match extension.filter(|extension| !extension.is_empty()) {
        Some(extension) => format!("{changed}.{extension}"),
        None => changed,
    })
}

fn sanitize_filename(name: &str, target: &str) -> String {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|part| part.to_str())
        .unwrap_or(name);
    let extension = path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("");
    let forbidden = |ch: char| match target {
        "windows" | "portable" => {
            ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        }
        "macos" => ch.is_control() || matches!(ch, ':' | '/'),
        "linux" => ch == '/' || ch == '\0',
        _ => true,
    };
    let clean = |value: &str| {
        value
            .chars()
            .map(|ch| if forbidden(ch) { '_' } else { ch })
            .collect::<String>()
    };
    let mut stem = clean(stem).trim().trim_end_matches(['.', ' ']).to_owned();
    let extension = clean(extension)
        .trim()
        .trim_end_matches(['.', ' '])
        .to_owned();
    if stem.is_empty() {
        stem = "untitled".into();
    }
    if matches!(target, "windows" | "portable") && reserved_windows_name(&stem) {
        stem.push('_');
    }
    truncate_filename(&mut stem, 180);
    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

fn reserved_windows_name(stem: &str) -> bool {
    matches!(
        stem.trim_end_matches(['.', ' '])
            .to_ascii_uppercase()
            .as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn truncate_filename(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

fn unique_output_name(name: &str, used: &mut std::collections::HashSet<String>) -> String {
    if used.insert(name.to_ascii_lowercase()) {
        return name.to_owned();
    }
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().into_owned());
    for index in 2..=10_000 {
        let candidate = if let Some(extension) = &extension {
            format!("{stem}-{index}.{extension}")
        } else {
            format!("{stem}-{index}")
        };
        if used.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
    }
    format!("{name}-copy")
}

fn duplicate_finder(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let roots = selected_folders(request, runtime)?;
    if roots.is_empty() {
        return Err("Select at least one folder to scan".into());
    }
    let mut entries = Vec::new();
    for (index, directory) in roots.iter().enumerate() {
        scan_directory(
            directory,
            index,
            Path::new(""),
            0,
            &mut entries,
            false,
            cancelled,
        )?;
        if entries.len() > MAX_FOLDER_ENTRIES {
            return Err(format!(
                "Folder scan exceeded the {MAX_FOLDER_ENTRIES} entry limit"
            ));
        }
    }
    let mut by_size: HashMap<u64, Vec<ScannedEntry>> = HashMap::new();
    for entry in entries.into_iter().filter(|entry| !entry.directory) {
        by_size.entry(entry.size).or_default().push(entry);
    }
    let mut by_hash: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut hashed_files = 0usize;
    for candidates in by_size.into_values().filter(|group| group.len() > 1) {
        for entry in candidates {
            check_cancelled(cancelled)?;
            let mut file = roots[entry.root]
                .open(&entry.relative)
                .map_err(|error| format!("Read {}: {error}", entry.display))?;
            let digest = hash_reader(&mut file, cancelled)?;
            by_hash
                .entry(format!("{}:{digest}", entry.size))
                .or_default()
                .push(json!({"path":entry.display,"size":entry.size,"blake3":digest}));
            hashed_files += 1;
        }
    }
    let groups = by_hash
        .into_values()
        .filter(|group| group.len() > 1)
        .collect::<Vec<_>>();
    Ok(json_result(
        manifest,
        json!({"groups":groups,"duplicateGroups":groups.len(),"filesFullyHashed":hashed_files}),
        "structured/duplicate-groups",
    ))
}

fn compare_files(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    if request.inputs.len() < 2 {
        return Err("Select at least two files or folders to compare".into());
    }
    if request
        .inputs
        .iter()
        .all(|input| input.mime == "folder/reference")
    {
        let roots = selected_folders(request, runtime)?;
        return compare_directories(manifest, roots, cancelled);
    }
    if request
        .inputs
        .iter()
        .any(|input| input.mime == "folder/reference")
    {
        return Err("Compare files with files, or folders with folders".into());
    }
    let files = selected_files(request)?;
    let mut results = Vec::new();
    for input in files {
        check_cancelled(cancelled)?;
        let path = runtime
            .grants()
            .resolve(&input.value)
            .map_err(|error| error.to_string())?;
        let mut file = runtime
            .grants()
            .open_scoped(&input.value)
            .map_err(|error| error.to_string())?;
        let size = file.metadata().map_err(|error| error.to_string())?.len();
        let digest = hash_reader(&mut file, cancelled)?;
        results.push(json!({"name":path.file_name().map(|value| value.to_string_lossy()),"size":size,"blake3":digest}));
    }
    let identical = results.first().is_some_and(|first| {
        results
            .iter()
            .all(|value| value["size"] == first["size"] && value["blake3"] == first["blake3"])
    });
    Ok(json_result(
        manifest,
        json!({"mode":"files","identical":identical,"items":results}),
        "structured/file-diff",
    ))
}

fn compare_directories(
    manifest: &ToolManifest,
    roots: Vec<Dir>,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    if roots.len() < 2 {
        return Err("Select at least two folders".into());
    }
    let mut all = Vec::new();
    let mut maps = Vec::new();
    for index in 0..roots.len() {
        let mut entries = Vec::new();
        scan_directory(
            &roots[index],
            index,
            Path::new(""),
            0,
            &mut entries,
            false,
            cancelled,
        )?;
        if entries.len() > MAX_FOLDER_ENTRIES {
            return Err("Folder scan exceeded the 100,000 entry limit".into());
        }
        let mut map = BTreeMap::new();
        for entry in entries.into_iter().filter(|entry| !entry.directory) {
            check_cancelled(cancelled)?;
            let mut file = roots[index]
                .open(&entry.relative)
                .map_err(|error| error.to_string())?;
            let digest = hash_reader(&mut file, cancelled)?;
            map.insert(entry.display.clone(), (entry.size, digest));
        }
        maps.push(map);
    }
    let base = &maps[0];
    let names = maps
        .iter()
        .flat_map(|map| map.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    for name in names {
        let values = maps
            .iter()
            .map(|map| {
                map.get(&name)
                    .map(|(size, hash)| json!({"size":size,"blake3":hash}))
            })
            .collect::<Vec<_>>();
        let identical = values.iter().all(|value| value == &values[0]);
        if !identical {
            let status = if base.contains_key(&name) {
                "changed"
            } else {
                "only-some"
            };
            all.push(json!({"path":name,"status":status,"items":values}));
        }
    }
    Ok(json_result(
        manifest,
        json!({"mode":"folders","differentCount":all.len(),"differences":all}),
        "structured/file-diff",
    ))
}

fn folder_sizes(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let roots = selected_folders(request, runtime)?;
    let mut entries = Vec::new();
    for (index, root) in roots.iter().enumerate() {
        scan_directory(root, index, Path::new(""), 0, &mut entries, true, cancelled)?;
    }
    if entries.len() > MAX_FOLDER_ENTRIES {
        return Err(format!(
            "Folder scan exceeded the {MAX_FOLDER_ENTRIES} entry limit"
        ));
    }
    let mut dirs: BTreeMap<String, u64> = BTreeMap::new();
    let mut extensions: BTreeMap<String, u64> = BTreeMap::new();
    let mut files = Vec::new();
    for entry in entries.iter().filter(|entry| !entry.directory) {
        check_cancelled(cancelled)?;
        files.push(json!({"path":entry.display,"size":entry.size}));
        let extension = Path::new(&entry.display)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("[none]")
            .to_ascii_lowercase();
        *extensions.entry(extension).or_default() += entry.size;
        let mut parent = entry.relative.parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            let key = path.to_string_lossy().into_owned();
            *dirs.entry(key).or_default() += entry.size;
            parent = path.parent();
        }
    }
    files.sort_by(|a, b| b["size"].as_u64().cmp(&a["size"].as_u64()));
    files.truncate(100);
    let mut directories = dirs
        .into_iter()
        .map(|(path, size)| json!({"path":path,"size":size}))
        .collect::<Vec<_>>();
    directories.sort_by(|a, b| b["size"].as_u64().cmp(&a["size"].as_u64()));
    directories.truncate(100);
    let extension_sizes = extensions
        .into_iter()
        .map(|(extension, size)| json!({"extension":extension,"size":size}))
        .collect::<Vec<_>>();
    let total = files_total(&entries);
    Ok(json_result(
        manifest,
        json!({"totalBytes":total,"fileCount":entries.iter().filter(|entry| !entry.directory).count(),"largestFiles":files,"directories":directories,"extensions":extension_sizes}),
        "structured/folder-sizes",
    ))
}

fn files_total(entries: &[ScannedEntry]) -> u64 {
    entries
        .iter()
        .filter(|entry| !entry.directory)
        .map(|entry| entry.size)
        .sum()
}

fn scan_directory(
    root: &Dir,
    root_index: usize,
    relative: &Path,
    depth: usize,
    output: &mut Vec<ScannedEntry>,
    include_dirs: bool,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if depth > 64 {
        return Ok(());
    }
    // `root` is already the current capability directory at each recursion
    // level. `relative` is only a display/open path retained for later reads.
    let directory = root.try_clone().map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    for entry in directory.entries().map_err(|error| error.to_string())? {
        check_cancelled(cancelled)?;
        if entries.len() >= MAX_FOLDER_ENTRIES {
            return Err(format!(
                "Folder scan exceeded the {MAX_FOLDER_ENTRIES} entry limit"
            ));
        }
        entries.push(entry.map_err(|error| error.to_string())?);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        check_cancelled(cancelled)?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let child_relative = relative.join(&name);
        let display = child_relative.to_string_lossy().replace('\\', "/");
        if file_type.is_dir() {
            if include_dirs {
                output.push(ScannedEntry {
                    root: root_index,
                    relative: child_relative.clone(),
                    display: display.clone(),
                    size: 0,
                    directory: true,
                });
            }
            if output.len() >= MAX_FOLDER_ENTRIES {
                return Err(format!(
                    "Folder scan exceeded the {MAX_FOLDER_ENTRIES} entry limit"
                ));
            }
            let child = entry
                .open_dir()
                .map_err(|error| format!("Open folder {display}: {error}"))?;
            scan_directory(
                &child,
                root_index,
                &child_relative,
                depth + 1,
                output,
                include_dirs,
                cancelled,
            )?;
        } else if file_type.is_file() {
            let metadata = entry.metadata().map_err(|error| error.to_string())?;
            output.push(ScannedEntry {
                root: root_index,
                relative: child_relative,
                display,
                size: metadata.len(),
                directory: false,
            });
            if output.len() >= MAX_FOLDER_ENTRIES {
                return Err(format!(
                    "Folder scan exceeded the {MAX_FOLDER_ENTRIES} entry limit"
                ));
            }
        }
    }
    Ok(())
}

fn selected_files<'a>(request: &'a ToolRequest) -> Result<Vec<&'a ToolValue>, String> {
    let files = request
        .inputs
        .iter()
        .filter(|input| input.kind == ValueKind::Artifact && input.mime != "folder/reference")
        .collect::<Vec<_>>();
    if files.len() != request.inputs.len() {
        return Err("This operation accepts selected files only".into());
    }
    Ok(files)
}

fn selected_folders(request: &ToolRequest, runtime: &Arcade) -> Result<Vec<Dir>, String> {
    request
        .inputs
        .iter()
        .map(|input| {
            if input.kind != ValueKind::Artifact || input.mime != "folder/reference" {
                return Err("Select folders only".into());
            }
            runtime
                .grants()
                .open_input_directory(&input.value)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn private_stage(runtime: &Arcade) -> Result<tempfile::TempDir, String> {
    tempfile::tempdir_in(runtime.artifact_staging_root())
        .map_err(|error| format!("Create private staging folder: {error}"))
}

fn publish(
    runtime: &Arcade,
    request: &ToolRequest,
    cancelled: &AtomicBool,
    path: &Path,
    name: &str,
) -> Result<ToolValue, String> {
    let directory = request
        .options
        .get("destinationGrant")
        .and_then(Value::as_str);
    runtime
        .publish_staged_output(directory, path, name, cancelled)
        .map(|file| file.as_tool_value())
        .map_err(|error| error.to_string())
}

fn copy_checked(
    source: &mut dyn Read,
    destination: &mut dyn Write,
    cancelled: &AtomicBool,
) -> Result<u64, String> {
    let mut buffer = vec![0; COPY_BUFFER];
    let mut total = 0u64;
    loop {
        check_cancelled(cancelled)?;
        let count = source
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        destination
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
        total = total.saturating_add(count as u64);
    }
    Ok(total)
}

fn hash_reader(reader: &mut dyn Read, cancelled: &AtomicBool) -> Result<String, String> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0; COPY_BUFFER];
    loop {
        check_cancelled(cancelled)?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Arcade;
    use arcade_contract::{ResultStatus, ToolRequest};
    use std::fs;

    fn run(runtime: &Arcade, id: &str, inputs: Vec<ToolValue>, options: Value) -> ToolResult {
        runtime
            .run_tool(ToolRequest {
                tool_id: id.into(),
                inputs,
                options,
            })
            .unwrap()
    }

    #[test]
    fn duplicate_finder_stays_within_granted_root() {
        let runtime = Arcade::in_memory().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("scan");
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("nested/item.dat"), b"same bytes").unwrap();
        fs::write(root.join("copy.dat"), b"same bytes").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path(), root.join("escape")).unwrap();
        let selected = runtime.grant_input_directory(&root).unwrap();
        let folder = ToolValue {
            kind: ValueKind::Artifact,
            value: selected.token,
            mime: "folder/reference".into(),
        };

        let duplicates = run(
            &runtime,
            "arcade.files.duplicates",
            vec![folder.clone()],
            json!({}),
        );
        assert_eq!(duplicates.status, ResultStatus::Success);
        let groups: Value = serde_json::from_str(&duplicates.outputs[0].value).unwrap();
        assert_eq!(groups["duplicateGroups"], 1);
    }

    #[test]
    fn rename_compare_and_size_tools_return_reviewable_results() {
        let runtime = Arcade::in_memory().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let first_path = temp.path().join("report-final.txt");
        let second_path = temp.path().join("report copy.txt");
        fs::write(&first_path, b"identical content").unwrap();
        fs::write(&second_path, b"identical content").unwrap();
        let first = runtime.grants().grant(&first_path).unwrap();
        let second = runtime.grants().grant(&second_path).unwrap();

        let comparison = run(
            &runtime,
            "arcade.files.compare",
            vec![first.as_tool_value(), second.as_tool_value()],
            json!({}),
        );
        let compared: Value = serde_json::from_str(&comparison.outputs[0].value).unwrap();
        assert_eq!(compared["identical"], true);

        let preview = run(
            &runtime,
            "arcade.files.rename",
            vec![first.as_tool_value()],
            json!({"mode":"replace","find":"final","replacement":"ready"}),
        );
        let preview_value: Value = serde_json::from_str(&preview.outputs[0].value).unwrap();
        assert_eq!(preview_value["preview"][0]["renamed"], "report-ready.txt");

        let applied = run(
            &runtime,
            "arcade.files.rename",
            vec![first.as_tool_value()],
            json!({"mode":"replace","find":"final","replacement":"ready","apply":true}),
        );
        assert!(
            applied
                .outputs
                .iter()
                .any(|output| output.kind == ValueKind::Artifact)
        );
        assert_eq!(fs::read(&first_path).unwrap(), b"identical content");

        let sanitized = run(
            &runtime,
            "arcade.files.rename",
            vec![second.as_tool_value()],
            json!({"mode":"safe","target":"windows"}),
        );
        let sanitized_value: Value = serde_json::from_str(&sanitized.outputs[0].value).unwrap();
        assert_eq!(sanitized_value["preview"][0]["renamed"], "report copy.txt");

        let folder = runtime.grant_input_directory(temp.path()).unwrap();
        let size = run(
            &runtime,
            "arcade.files.sizes",
            vec![ToolValue {
                kind: ValueKind::Artifact,
                value: folder.token,
                mime: "folder/reference".into(),
            }],
            json!({}),
        );
        let size_value: Value = serde_json::from_str(&size.outputs[0].value).unwrap();
        assert_eq!(size_value["fileCount"], 2);
        assert_eq!(size_value["totalBytes"], 34);
    }
}
