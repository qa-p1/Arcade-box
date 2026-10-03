//! Text and structured-data tools that operate on the shared tool contracts.

use crate::{
    Arcade,
    artifacts::validate_portable_filename,
    tool_kit::{check_cancelled, input_text, option_bool, option_str, single_text, text_result},
};
use arcade_contract::{ResultStatus, ToolManifest, ToolRequest, ToolResult};
use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, html};
use serde_json::{Map, Value, json};
use similar::TextDiff;
use std::{collections::HashSet, sync::atomic::AtomicBool};
use unicode_segmentation::UnicodeSegmentation;

mod clean;
mod lorem;
mod structured;

const MAX_TEXT_INPUT: usize = 16 * 1024 * 1024;
const MAX_CSV_INPUT: usize = 64 * 1024 * 1024;
const MAX_CSV_ROWS: usize = 1_000_000;

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    match manifest.id.as_str() {
        "arcade.text.diff" => text_diff(manifest, request, runtime, cancelled),
        "arcade.text.case" => {
            let input = single_text(request, runtime, MAX_TEXT_INPUT, cancelled)?;
            let output = clean::change_case(&input, option_str(request, "mode", "upper"))?;
            Ok(text_result(manifest, output, "text/plain"))
        }
        "arcade.text.clean" => clean_text(manifest, request, runtime, cancelled),
        "arcade.text.structured" => structured_data(manifest, request, runtime, cancelled),
        "arcade.text.csv" => csv_workbench(manifest, request, runtime, cancelled),
        "arcade.text.markdown" => markdown_export(manifest, request, runtime, cancelled),
        "arcade.text.statistics" => text_statistics(manifest, request, runtime, cancelled),
        "arcade.text.lorem" => Ok(text_result(
            manifest,
            lorem::generate(request)?,
            "text/plain",
        )),
        _ => Err(format!(
            "No text executor is registered for {}",
            manifest.id
        )),
    }
}

fn clean_text(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let input = single_text(request, runtime, MAX_TEXT_INPUT, cancelled)?;
    let options = clean::CleanOptions {
        trim: option_bool(request, "trim", true),
        collapse: option_bool(request, "collapse", false),
        remove_blank: option_bool(request, "removeBlank", false),
        unique: option_bool(request, "unique", false),
        ignore_case: option_bool(request, "ignoreCase", false),
        sort: option_str(request, "sort", "keep"),
        tabs: option_str(request, "tabs", "keep"),
        crlf: option_str(request, "lineEnding", "lf") == "crlf",
    };
    let before = input.lines().count();
    let output = clean::clean(&input, &options)?;
    let after = if output.is_empty() {
        0
    } else {
        output.lines().count()
    };
    let mut result = text_result(manifest, output, "text/plain");
    result.message = Some(match before - after.min(before) {
        0 => plural(after, "line"),
        removed => format!("{} ({removed} removed)", plural(after, "line")),
    });
    Ok(result)
}

fn text_diff(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let values = request
        .inputs
        .iter()
        .map(|input| input_text(input, runtime, MAX_TEXT_INPUT, cancelled))
        .collect::<Result<Vec<_>, _>>()?;
    let operation = option_str(request, "operation", "diff");
    if operation == "merge" {
        let (base, left, right) = match values.as_slice() {
            [base, left, right] => (base.as_str(), left.as_str(), right.as_str()),
            [combined] => split_merge_input(combined)?,
            _ => return Err(
                "Merge needs base, left, and right text. Use three inputs or the --- BASE ---, --- LEFT ---, and --- RIGHT --- markers.".into(),
            ),
        };
        if base
            .len()
            .saturating_add(left.len())
            .saturating_add(right.len())
            > MAX_TEXT_INPUT
        {
            return Err("Three-way merge is limited to 16 MB combined input".into());
        }
        let (merged, conflicts) = merge_three_way(base, left, right);
        let mut result = text_result(manifest, merged, "text/plain");
        if conflicts > 0 {
            result.warnings.push(format!(
                "Found {conflicts} overlapping changes. Resolve the conflict markers before using the merged text."
            ));
        }
        return Ok(result);
    }
    if operation != "diff" {
        return Err("Choose diff or merge".into());
    }
    let (left, right) =
        match values.as_slice() {
            [left, right] => (left.as_str(), right.as_str()),
            [combined] => split_diff_input(combined)?,
            _ => return Err(
                "Compare two text inputs, or separate them with a line containing `--- RIGHT ---`."
                    .into(),
            ),
        };
    if left.len().saturating_add(right.len()) > MAX_TEXT_INPUT {
        return Err("Text diff is limited to 16 MB combined input".into());
    }
    let rendered = match option_str(request, "view", "unified") {
        "unified" => TextDiff::from_lines(left, right)
            .unified_diff()
            .header("Left", "Right")
            .to_string(),
        "side-by-side" => render_side_by_side_diff(left, right),
        "inline" => render_inline_diff(left, right),
        _ => return Err("Choose unified, side-by-side, or inline diff view".into()),
    };
    Ok(text_result(manifest, rendered, "text/plain"))
}

fn render_side_by_side_diff(left: &str, right: &str) -> String {
    let diff = TextDiff::from_lines(left, right);
    let mut rendered = String::from("LEFT | RIGHT\n");
    for operation in diff.ops() {
        let old = &diff.old_slices()[operation.old_range()];
        let new = &diff.new_slices()[operation.new_range()];
        let equal = operation.tag() == similar::DiffTag::Equal;
        for index in 0..old.len().max(new.len()) {
            let old_line = old.get(index).copied().unwrap_or("");
            let new_line = new.get(index).copied().unwrap_or("");
            if equal {
                rendered.push_str("  ");
            } else if old_line.is_empty() {
                rendered.push_str("+ ");
            } else if new_line.is_empty() {
                rendered.push_str("- ");
            } else {
                rendered.push_str("~ ");
            }
            rendered.push_str(old_line.trim_end_matches(['\r', '\n']));
            rendered.push_str(" | ");
            rendered.push_str(new_line.trim_end_matches(['\r', '\n']));
            rendered.push('\n');
        }
    }
    rendered
}

fn render_inline_diff(left: &str, right: &str) -> String {
    let diff = TextDiff::from_lines(left, right);
    let mut rendered = String::new();
    for operation in diff.ops() {
        let old = &diff.old_slices()[operation.old_range()];
        let new = &diff.new_slices()[operation.new_range()];
        let equal = operation.tag() == similar::DiffTag::Equal;
        for index in 0..old.len().max(new.len()) {
            let old_line = old
                .get(index)
                .copied()
                .unwrap_or("")
                .trim_end_matches(['\r', '\n']);
            let new_line = new
                .get(index)
                .copied()
                .unwrap_or("")
                .trim_end_matches(['\r', '\n']);
            if equal {
                rendered.push_str("  ");
                rendered.push_str(old_line);
            } else if old_line.is_empty() {
                rendered.push_str("+ ");
                rendered.push_str(new_line);
            } else if new_line.is_empty() {
                rendered.push_str("- ");
                rendered.push_str(old_line);
            } else {
                rendered.push_str("~ ");
                rendered.push_str(&inline_line_changes(old_line, new_line));
            }
            rendered.push('\n');
        }
    }
    rendered
}

fn inline_line_changes(old: &str, new: &str) -> String {
    if old.len().saturating_add(new.len()) > 4096 {
        return format!("[-{old}-]{{+{new}+}}");
    }
    let diff = TextDiff::from_chars(old, new);
    let mut rendered = String::new();
    let mut deleted = String::new();
    let mut inserted = String::new();
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                append_inline_change(&mut rendered, &mut deleted, &mut inserted);
                rendered.push_str(change.value());
            }
            similar::ChangeTag::Delete => deleted.push_str(change.value()),
            similar::ChangeTag::Insert => inserted.push_str(change.value()),
        }
    }
    append_inline_change(&mut rendered, &mut deleted, &mut inserted);
    rendered
}

fn append_inline_change(rendered: &mut String, deleted: &mut String, inserted: &mut String) {
    if !deleted.is_empty() {
        rendered.push_str("[-");
        rendered.push_str(deleted);
        rendered.push_str("-]");
        deleted.clear();
    }
    if !inserted.is_empty() {
        rendered.push_str("{+");
        rendered.push_str(inserted);
        rendered.push_str("+}");
        inserted.clear();
    }
}

fn split_merge_input(input: &str) -> Result<(&str, &str, &str), String> {
    let input = input
        .strip_prefix("--- BASE ---\n")
        .ok_or("Start three-way merge input with a line containing `--- BASE ---`.")?;
    let (base, rest) = input
        .split_once("\n--- LEFT ---\n")
        .ok_or("Add a line containing `--- LEFT ---` after the base text.")?;
    let (left, right) = rest
        .split_once("\n--- RIGHT ---\n")
        .ok_or("Add a line containing `--- RIGHT ---` after the left text.")?;
    Ok((base, left, right))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextEdit {
    start: usize,
    end: usize,
    replacement: Vec<String>,
}

fn merge_three_way(base: &str, left: &str, right: &str) -> (String, usize) {
    let base_lines = base.split_inclusive('\n').collect::<Vec<_>>();
    let left_edits = text_edits(base, left);
    let right_edits = text_edits(base, right);
    let mut left_index = 0;
    let mut right_index = 0;
    let mut cursor = 0;
    let mut output = String::new();
    let mut conflicts = 0;
    while left_index < left_edits.len() || right_index < right_edits.len() {
        let next_left = left_edits.get(left_index);
        let next_right = right_edits.get(right_index);
        match (next_left, next_right) {
            (Some(left_edit), Some(right_edit)) if left_edit == right_edit => {
                append_base(&mut output, &base_lines, cursor, left_edit.start);
                output.push_str(&left_edit.replacement.concat());
                cursor = cursor.max(left_edit.end);
                left_index += 1;
                right_index += 1;
            }
            (Some(left_edit), Some(right_edit)) if edits_overlap(left_edit, right_edit) => {
                let cluster_start = left_edit.start.min(right_edit.start);
                let mut cluster_end = left_edit.end.max(right_edit.end);
                let mut left_group = vec![left_edit.clone()];
                let mut right_group = vec![right_edit.clone()];
                left_index += 1;
                right_index += 1;
                loop {
                    let mut expanded = false;
                    while let Some(candidate) = left_edits.get(left_index) {
                        if right_group
                            .iter()
                            .any(|other| edits_overlap(candidate, other))
                        {
                            cluster_end = cluster_end.max(candidate.end);
                            left_group.push(candidate.clone());
                            left_index += 1;
                            expanded = true;
                        } else {
                            break;
                        }
                    }
                    while let Some(candidate) = right_edits.get(right_index) {
                        if left_group
                            .iter()
                            .any(|other| edits_overlap(candidate, other))
                        {
                            cluster_end = cluster_end.max(candidate.end);
                            right_group.push(candidate.clone());
                            right_index += 1;
                            expanded = true;
                        } else {
                            break;
                        }
                    }
                    if !expanded {
                        break;
                    }
                }
                append_base(&mut output, &base_lines, cursor, cluster_start);
                let base_segment = lines_to_string(&base_lines[cluster_start..cluster_end]);
                let left_result =
                    apply_edits_to_segment(&base_lines, cluster_start, cluster_end, &left_group);
                let right_result =
                    apply_edits_to_segment(&base_lines, cluster_start, cluster_end, &right_group);
                if left_result == right_result {
                    output.push_str(&left_result);
                } else if left_result == base_segment {
                    output.push_str(&right_result);
                } else if right_result == base_segment {
                    output.push_str(&left_result);
                } else {
                    conflicts += 1;
                    append_conflict(&mut output, &left_result, &right_result);
                }
                cursor = cursor.max(cluster_end);
            }
            (Some(left_edit), Some(right_edit)) if left_edit.start <= right_edit.start => {
                append_base(&mut output, &base_lines, cursor, left_edit.start);
                output.push_str(&left_edit.replacement.concat());
                cursor = cursor.max(left_edit.end);
                left_index += 1;
            }
            (Some(_), Some(right_edit)) => {
                append_base(&mut output, &base_lines, cursor, right_edit.start);
                output.push_str(&right_edit.replacement.concat());
                cursor = cursor.max(right_edit.end);
                right_index += 1;
            }
            (Some(left_edit), None) => {
                append_base(&mut output, &base_lines, cursor, left_edit.start);
                output.push_str(&left_edit.replacement.concat());
                cursor = cursor.max(left_edit.end);
                left_index += 1;
            }
            (None, Some(right_edit)) => {
                append_base(&mut output, &base_lines, cursor, right_edit.start);
                output.push_str(&right_edit.replacement.concat());
                cursor = cursor.max(right_edit.end);
                right_index += 1;
            }
            (None, None) => break,
        }
    }
    append_base(&mut output, &base_lines, cursor, base_lines.len());
    (output, conflicts)
}

fn text_edits(base: &str, variant: &str) -> Vec<TextEdit> {
    let diff = TextDiff::from_lines(base, variant);
    diff.ops()
        .iter()
        .filter(|operation| operation.tag() != similar::DiffTag::Equal)
        .map(|operation| {
            let old = operation.old_range();
            let new = operation.new_range();
            TextEdit {
                start: old.start,
                end: old.end,
                replacement: diff.new_slices()[new]
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect(),
            }
        })
        .collect()
}

fn edits_overlap(left: &TextEdit, right: &TextEdit) -> bool {
    match (left.start == left.end, right.start == right.end) {
        (true, true) => left.start == right.start,
        (true, false) => left.start >= right.start && left.start < right.end,
        (false, true) => right.start >= left.start && right.start < left.end,
        (false, false) => left.start < right.end && right.start < left.end,
    }
}

fn append_base(output: &mut String, base: &[&str], start: usize, end: usize) {
    if start < end {
        output.push_str(&lines_to_string(
            &base[start.min(base.len())..end.min(base.len())],
        ));
    }
}

fn lines_to_string(lines: &[&str]) -> String {
    lines.concat()
}

fn apply_edits_to_segment(base: &[&str], start: usize, end: usize, edits: &[TextEdit]) -> String {
    let mut output = String::new();
    let mut cursor = start;
    for edit in edits {
        append_base(&mut output, base, cursor, edit.start);
        output.push_str(&edit.replacement.concat());
        cursor = cursor.max(edit.end);
    }
    append_base(&mut output, base, cursor, end);
    output
}

fn append_conflict(output: &mut String, left: &str, right: &str) {
    output.push_str("<<<<<<< LEFT\n");
    output.push_str(left);
    if !left.ends_with('\n') {
        output.push('\n');
    }
    output.push_str("=======\n");
    output.push_str(right);
    if !right.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(">>>>>>> RIGHT\n");
}

fn split_diff_input(input: &str) -> Result<(&str, &str), String> {
    let marker = "--- RIGHT ---";
    let Some((left, right)) = input.split_once(marker) else {
        return Err("Paste two texts with a line containing `--- RIGHT ---` between them.".into());
    };
    let left = left.strip_suffix('\n').unwrap_or(left);
    let right = right.strip_prefix('\n').unwrap_or(right);
    Ok((left, right))
}

fn structured_data(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let input = single_text(request, runtime, MAX_TEXT_INPUT, cancelled)?;
    let converted = structured::convert(
        &input,
        option_str(request, "from", "auto"),
        option_str(request, "to", "same"),
        option_str(request, "indent", "2"),
    )?;
    let (from, to) = (
        converted.from.to_ascii_uppercase(),
        converted.to.to_ascii_uppercase(),
    );
    let mut result = text_result(
        manifest,
        converted.text,
        &format!("structured/{}", converted.to),
    );
    result.message = Some(if from == to {
        format!("Valid {from}")
    } else {
        format!("Converted {from} to {to}")
    });
    result
        .metadata
        .insert("detectedFormat".into(), json!(converted.from));
    result.warnings = converted.warnings;
    Ok(result)
}

fn csv_workbench(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let input_value = request
        .inputs
        .first()
        .ok_or("Select or paste a CSV table")?;
    let input = input_text(input_value, runtime, MAX_CSV_INPUT, cancelled)?;
    if request.inputs.len() != 1 {
        return Err("CSV workbench takes one table at a time".into());
    }
    let requested_format = option_str(request, "inputFormat", "auto");
    let json_input = match requested_format {
        "auto" => {
            let trimmed = input.trim_start();
            trimmed.starts_with('{') || trimmed.starts_with('[')
        }
        "json" => true,
        "csv" | "tsv" => false,
        other => return Err(format!("Unsupported input format: {other}")),
    };
    let (input_delimiter, headers, mut rows) = if json_input {
        let (headers, rows) = parse_json_table(&input)?;
        (b',', headers, rows)
    } else {
        let input_delimiter = if requested_format == "tsv" {
            b'\t'
        } else {
            match option_str(request, "inputDelimiter", "auto") {
                "comma" => b',',
                "tab" => b'\t',
                "semicolon" => b';',
                "auto" => detect_delimiter(&input),
                other => return Err(format!("Unsupported delimiter: {other}")),
            }
        };
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(input_delimiter)
            .flexible(false)
            .from_reader(input.as_bytes());
        let headers = reader
            .headers()
            .map_err(|error| format!("CSV header: {error}"))?
            .clone();
        let mut rows = Vec::new();
        for (index, row) in reader.records().enumerate() {
            check_cancelled(cancelled)?;
            if index >= MAX_CSV_ROWS {
                return Err(format!("CSV is limited to {MAX_CSV_ROWS} data rows"));
            }
            rows.push(row.map_err(|error| format!("CSV row {}: {error}", index + 2))?);
        }
        (input_delimiter, headers, rows)
    };
    let operation = option_str(request, "operation", "to-json");
    let column = option_str(request, "column", "");
    match operation {
        "inspect" => {
            let delimiter_name = match input_delimiter {
                b',' => "comma",
                b'\t' => "tab",
                b';' => "semicolon",
                _ => "custom",
            };
            return Ok(text_result(
                manifest,
                json!({
                    "delimiter":delimiter_name,
                    "columns":headers.iter().collect::<Vec<_>>(),
                    "rowCount":rows.len(),
                    "sample":rows.iter().take(20).map(|row| row.iter().collect::<Vec<_>>()).collect::<Vec<_>>()
                })
                .to_string(),
                "structured/csv-preview",
            ));
        }
        "sort" => {
            let column = column_index(&headers, column)?;
            let sort_mode = option_str(request, "sortMode", "text");
            if !matches!(sort_mode, "text" | "numeric") {
                return Err("Sort mode must be text or numeric".into());
            }
            let case_sensitive = option_bool(request, "caseSensitive", false);
            let descending = option_bool(request, "descending", false);
            rows.sort_by(|left, right| {
                let left_value = left.get(column).unwrap_or("");
                let right_value = right.get(column).unwrap_or("");
                let ordering = if sort_mode == "numeric" {
                    let left_number = left_value.parse::<f64>().ok().filter(|n| n.is_finite());
                    let right_number = right_value.parse::<f64>().ok().filter(|n| n.is_finite());
                    match (left_number, right_number) {
                        (Some(a), Some(b)) => a.total_cmp(&b),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => left_value.cmp(right_value),
                    }
                } else if case_sensitive {
                    left_value.cmp(right_value)
                } else {
                    left_value
                        .to_lowercase()
                        .cmp(&right_value.to_lowercase())
                        .then_with(|| left_value.cmp(right_value))
                };
                if descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
        }
        "deduplicate" => {
            let column = column_index(&headers, column)?;
            let mut seen = HashSet::new();
            rows.retain(|row| seen.insert(row.get(column).unwrap_or("").to_owned()));
        }
        "filter" => {
            let column = column_index(&headers, column)?;
            let needle = option_str(request, "contains", "").to_lowercase();
            rows.retain(|row| {
                row.get(column)
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&needle)
            });
        }
        "select-columns" => {
            let names = option_str(request, "columns", "")
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>();
            if names.is_empty() {
                return Err("Enter one or more comma-separated column names".into());
            }
            let indices = names
                .iter()
                .map(|name| column_index(&headers, name))
                .collect::<Result<Vec<_>, _>>()?;
            let new_headers = indices
                .iter()
                .map(|index| headers.get(*index).unwrap_or(""))
                .collect::<Vec<_>>();
            let selected_rows = rows
                .iter()
                .map(|row| {
                    indices
                        .iter()
                        .map(|index| row.get(*index).unwrap_or(""))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            return write_csv_result(
                manifest,
                request,
                runtime,
                cancelled,
                new_headers,
                selected_rows,
                "csv",
            );
        }
        "rename-columns" => {
            let renames = option_str(request, "renames", "");
            if renames.trim().is_empty() {
                return Err("Enter one old = new column mapping per line".into());
            }
            let mut renamed_headers = headers.iter().map(str::to_owned).collect::<Vec<_>>();
            let mut renamed_indices = HashSet::new();
            for mapping in renames
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                let (old, new) = mapping
                    .split_once('=')
                    .ok_or("Use one old = new column mapping per line")?;
                let old = old.trim();
                let new = new.trim();
                if new.is_empty() {
                    return Err("New column names cannot be empty".into());
                }
                let index = column_index(&headers, old)?;
                if !renamed_indices.insert(index) {
                    return Err(format!("Column `{old}` was renamed more than once"));
                }
                renamed_headers[index] = new.to_owned();
            }
            let mut unique = HashSet::new();
            if renamed_headers
                .iter()
                .any(|name| !unique.insert(name.to_lowercase()))
            {
                return Err("Renamed column names must be unique".into());
            }
            return write_csv_result(
                manifest,
                request,
                runtime,
                cancelled,
                renamed_headers.iter().map(String::as_str).collect(),
                rows.iter().map(|row| row.iter().collect()).collect(),
                "csv",
            );
        }
        "to-json" => {
            let data = rows
                .iter()
                .map(|row| {
                    let mut record = Map::new();
                    for (index, header) in headers.iter().enumerate() {
                        record.insert(
                            header.to_owned(),
                            Value::String(row.get(index).unwrap_or("").to_owned()),
                        );
                    }
                    Value::Object(record)
                })
                .collect::<Vec<_>>();
            return Ok(text_result(
                manifest,
                serde_json::to_string_pretty(&data).map_err(|error| error.to_string())?,
                "structured/json",
            ));
        }
        "to-csv" | "to-tsv" => {}
        other => return Err(format!("Unknown CSV operation: {other}")),
    }
    let format = if operation == "to-tsv" { "tsv" } else { "csv" };
    write_csv_result(
        manifest,
        request,
        runtime,
        cancelled,
        headers.iter().collect(),
        rows.iter().map(|row| row.iter().collect()).collect(),
        format,
    )
}

fn write_csv_result(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    headers: Vec<&str>,
    rows: Vec<Vec<&str>>,
    format: &str,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    let mut bytes = Vec::new();
    let delimiter = if format == "tsv" { b'\t' } else { b',' };
    {
        let mut writer = csv::WriterBuilder::new()
            .delimiter(delimiter)
            .from_writer(&mut bytes);
        writer
            .write_record(headers)
            .map_err(|error| error.to_string())?;
        for row in rows {
            check_cancelled(cancelled)?;
            writer
                .write_record(row)
                .map_err(|error| error.to_string())?;
        }
        writer.flush().map_err(|error| error.to_string())?;
    }
    let extension = if format == "tsv" { "tsv" } else { "csv" };
    let default_name = format!("table.{extension}");
    let requested_output_name = option_str(request, "outputName", &default_name);
    validate_portable_filename(requested_output_name)?;
    let output_name = ensure_extension(requested_output_name, extension);
    validate_portable_filename(&output_name)?;
    let temp = tempfile::NamedTempFile::new_in(runtime.artifact_staging_root())
        .map_err(|error| error.to_string())?;
    std::fs::write(temp.path(), bytes).map_err(|error| error.to_string())?;
    let destination = request
        .options
        .get("destinationGrant")
        .and_then(Value::as_str);
    let grant = runtime
        .publish_staged_output(destination, temp.path(), &output_name, cancelled)
        .map_err(|error| error.to_string())?;
    Ok(ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![grant.as_tool_value()],
        message: None,
        warnings: vec![],
        metadata: Default::default(),
    })
}

fn detect_delimiter(input: &str) -> u8 {
    [b',', b'\t', b';']
        .into_iter()
        .map(|delimiter| {
            let mut reader = csv::ReaderBuilder::new()
                .delimiter(delimiter)
                .flexible(true)
                .from_reader(input.as_bytes());
            let header_width = reader
                .headers()
                .map(|headers| headers.len())
                .unwrap_or_default();
            let mut consistent = 0i64;
            let mut inconsistent = 0i64;
            for record in reader.records().take(20).flatten() {
                if record.len() == header_width {
                    consistent += 1;
                } else {
                    inconsistent += 1;
                }
            }
            let score = if header_width > 1 {
                header_width as i64 * 4 + consistent * 3 - inconsistent * 5
            } else {
                i64::MIN
            };
            (delimiter, score)
        })
        .max_by_key(|(_, score)| *score)
        .filter(|(_, score)| *score != i64::MIN)
        .map(|(delimiter, _)| delimiter)
        .unwrap_or(b',')
}

fn parse_json_table(input: &str) -> Result<(csv::StringRecord, Vec<csv::StringRecord>), String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("JSON table: {error}"))?;
    let source_rows = match value {
        Value::Array(rows) => rows,
        object @ Value::Object(_) => vec![object],
        _ => return Err("JSON table must be an object or an array of objects".into()),
    };
    if source_rows.len() > MAX_CSV_ROWS {
        return Err(format!("JSON table is limited to {MAX_CSV_ROWS} rows"));
    }
    let mut header_names = Vec::new();
    let mut header_set = HashSet::new();
    for row in &source_rows {
        let object = row
            .as_object()
            .ok_or("Every JSON table row must be an object")?;
        for name in object.keys() {
            if header_set.insert(name.clone()) {
                header_names.push(name.clone());
            }
        }
    }
    if header_names.is_empty() {
        return Err("JSON table has no columns".into());
    }
    let mut headers = csv::StringRecord::new();
    for name in &header_names {
        headers.push_field(name);
    }
    let rows = source_rows
        .iter()
        .map(|row| {
            let object = row.as_object().expect("row objects checked above");
            let mut record = csv::StringRecord::new();
            for name in &header_names {
                let value = object.get(name).map(json_cell).unwrap_or_default();
                record.push_field(&value);
            }
            record
        })
        .collect();
    Ok((headers, rows))
}

fn json_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn ensure_extension(name: &str, extension: &str) -> String {
    let path = std::path::Path::new(name);
    match path.extension().and_then(|value| value.to_str()) {
        Some(_) => format!(
            "{}.{}",
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("table"),
            extension
        ),
        None => format!("{name}.{extension}"),
    }
}

fn column_index(headers: &csv::StringRecord, name: &str) -> Result<usize, String> {
    if let Ok(index) = name.parse::<usize>() {
        if index < headers.len() {
            return Ok(index);
        }
    }
    headers
        .iter()
        .position(|header| header.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("Column '{name}' was not found"))
}

fn markdown_export(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let markdown = single_text(request, runtime, MAX_TEXT_INPUT, cancelled)?;
    let mut options = Options::empty();
    options
        .insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(&markdown, options).map(|event| match event {
        Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) if !safe_markdown_url(&dest_url) => Event::Start(Tag::Link {
            link_type,
            dest_url: CowStr::from(""),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) if !safe_markdown_url(&dest_url) => Event::Start(Tag::Image {
            link_type,
            dest_url: CowStr::from(""),
            title,
            id,
        }),
        other => other,
    });
    let mut body = String::new();
    html::push_html(&mut body, parser);
    let document = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\"><title>Arcade Box Markdown Export</title></head><body>{body}</body></html>"
    );
    let default_name = "markdown-export.html";
    let output_name = option_str(request, "outputName", default_name);
    validate_portable_filename(output_name)?;
    let temp = tempfile::NamedTempFile::new_in(runtime.artifact_staging_root())
        .map_err(|error| error.to_string())?;
    std::fs::write(temp.path(), document.as_bytes()).map_err(|error| error.to_string())?;
    let destination = request
        .options
        .get("destinationGrant")
        .and_then(Value::as_str);
    let grant = runtime
        .publish_staged_output(destination, temp.path(), output_name, cancelled)
        .map_err(|error| error.to_string())?;
    Ok(ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![grant.as_tool_value()],
        message: None,
        warnings: vec![],
        metadata: Default::default(),
    })
}

fn safe_markdown_url(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("https://")
        || value.starts_with("http://")
        || value.starts_with("mailto:")
        || value.starts_with('#')
}

fn text_statistics(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let input = single_text(request, runtime, MAX_TEXT_INPUT, cancelled)?;
    let words = input.unicode_words().count();
    let sentences = input
        .chars()
        .filter(|character| matches!(character, '.' | '!' | '?' | '。' | '！' | '？'))
        .count()
        .max(usize::from(words > 0));
    let syllables = input
        .unicode_words()
        .map(approximate_syllables)
        .sum::<usize>();
    let sentences_f = sentences.max(1) as f64;
    let words_f = words.max(1) as f64;
    let readability = if words == 0 {
        None
    } else {
        Some(206.835 - 1.015 * (words_f / sentences_f) - 84.6 * (syllables as f64 / words_f))
    };
    let value = json!({
        "bytesUtf8": input.len(),
        "characters": input.chars().count(),
        "graphemes": input.graphemes(true).count(),
        "words": words,
        "lines": if input.is_empty() { 0 } else { input.lines().count() },
        "sentences": sentences,
        "estimatedReadingSeconds": ((words as f64 / 220.0) * 60.0).round() as u64,
        "estimatedSyllables": syllables,
        "fleschReadingEaseApprox": readability,
        "readabilityNote": "Syllables are estimated with an English-oriented heuristic; treat readability as approximate for other languages.",
    });
    Ok(text_result(
        manifest,
        serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?,
        "structured/text-statistics",
    ))
}

fn approximate_syllables(word: &str) -> usize {
    let lower = word.to_lowercase();
    let mut groups = 0usize;
    let mut previous_vowel = false;
    for character in lower.chars() {
        let vowel = "aeiouy".contains(character);
        if vowel && !previous_vowel {
            groups += 1;
        }
        previous_vowel = vowel;
    }
    if lower.ends_with('e') && groups > 1 {
        groups -= 1;
    }
    groups.max(1)
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod diff_tests {
    use super::*;

    #[test]
    fn three_way_merge_combines_separate_changes_and_marks_conflicts() {
        let (merged, conflicts) = merge_three_way(
            "first\nshared\nlast\n",
            "FIRST\nshared\nlast\n",
            "first\nshared\nLAST\n",
        );
        assert_eq!(merged, "FIRST\nshared\nLAST\n");
        assert_eq!(conflicts, 0);

        let (merged, conflicts) = merge_three_way("first\n", "left edit\n", "right edit\n");
        assert_eq!(conflicts, 1);
        assert!(merged.contains("<<<<<<< LEFT\nleft edit\n=======\nright edit\n>>>>>>> RIGHT\n"));
    }

    #[test]
    fn two_way_diff_supports_side_by_side_and_inline_views() {
        let left = "hello brave world\n";
        let right = "hello new world\n";
        assert!(
            render_side_by_side_diff(left, right).contains("~ hello brave world | hello new world")
        );
        let inline = render_inline_diff(left, right);
        assert!(inline.contains("[-"), "{inline}");
        assert!(inline.contains("{+"), "{inline}");
    }
}

#[cfg(test)]
mod csv_tests {
    use super::*;
    use arcade_contract::ToolValue;

    #[test]
    fn delimiter_detection_respects_quoted_commas() {
        assert_eq!(
            detect_delimiter("name;description;count\nAda;\"a, b\";3\nLin;\"c, d\";4\n"),
            b';'
        );
        assert_eq!(
            detect_delimiter("name,description,count\nAda,\"a, b\",3\nLin,\"c, d\",4\n"),
            b','
        );
    }

    #[test]
    fn json_table_can_be_renamed_and_exported_as_csv() {
        let runtime = Arcade::in_memory().unwrap();
        let manifest = runtime
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.text.csv")
            .unwrap();
        let request = ToolRequest {
            tool_id: manifest.id.clone(),
            inputs: vec![ToolValue::text(
                r#"[{"name":"Ada","age":36},{"name":"Lin","age":8}]"#,
                "text/plain",
            )],
            options: json!({
                "operation":"rename-columns",
                "renames":"age = years",
                "outputName":"people.csv"
            }),
        };
        let result = csv_workbench(&manifest, &request, &runtime, &AtomicBool::new(false)).unwrap();
        let output_path = runtime.grants().resolve(&result.outputs[0].value).unwrap();
        let output = std::fs::read_to_string(output_path).unwrap();
        // Columns keep the order they have in the JSON.
        assert_eq!(output, "name,years\nAda,36\nLin,8\n");
    }
}
