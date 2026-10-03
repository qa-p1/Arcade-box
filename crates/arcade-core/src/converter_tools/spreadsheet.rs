//! Excel ↔ CSV. Spreadsheets (xlsx, xlsm, xlsb, xls, ods) become one CSV per
//! sheet; one or more CSV/TSV files become a single .xlsx workbook with one
//! sheet per file.

use crate::{
    Arcade,
    tool_kit::{
        check_cancelled, input_bytes, input_stem, option_bool, option_str, output_name,
        publish_bytes, success,
    },
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use calamine::{Data, Reader, open_workbook_auto_from_rs};
use rust_xlsxwriter::{Format, Workbook};
use std::{collections::HashSet, io::Cursor, sync::atomic::AtomicBool};

const MAX_INPUT_BYTES: usize = 100 * 1024 * 1024;
const MAX_ROWS: usize = 1_048_576;
const MAX_COLUMNS: usize = 16_384;

pub(super) fn convert(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let delimited = |input: &ToolValue| matches!(input.mime.as_str(), "file/csv" | "file/tsv");
    match request.inputs.as_slice() {
        [] => Err("Select a spreadsheet, or one or more CSV files".into()),
        [input] if input.mime == "file/spreadsheet" => {
            spreadsheet_to_csv(manifest, request, runtime, input, cancelled)
        }
        inputs if inputs.iter().all(delimited) => {
            csv_to_workbook(manifest, request, runtime, inputs, cancelled)
        }
        _ => Err("Select either one spreadsheet, or only CSV/TSV files".into()),
    }
}

fn delimiter(request: &ToolRequest) -> Result<u8, String> {
    match option_str(request, "delimiter", "comma") {
        "comma" => Ok(b','),
        "semicolon" => Ok(b';'),
        "tab" => Ok(b'\t'),
        other => Err(format!("Unknown delimiter `{other}`")),
    }
}

fn spreadsheet_to_csv(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    input: &ToolValue,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let bytes = input_bytes(input, runtime, MAX_INPUT_BYTES, cancelled)?;
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|error| format!("Could not open this spreadsheet: {error}"))?;
    let separator = delimiter(request)?;
    let stem = input_stem(runtime, input, "spreadsheet");
    let mut names = workbook.sheet_names();
    if option_str(request, "sheets", "all") == "first" {
        names.truncate(1);
    }
    let mut sheets = Vec::new();
    let mut skipped = Vec::new();
    for name in names {
        check_cancelled(cancelled)?;
        let range = workbook
            .worksheet_range(&name)
            .map_err(|error| format!("Could not read sheet `{name}`: {error}"))?;
        if range.is_empty() {
            skipped.push(name);
            continue;
        }
        let mut writer = csv::WriterBuilder::new()
            .delimiter(separator)
            .from_writer(Vec::new());
        for row in range.rows() {
            writer
                .write_record(row.iter().map(cell_text))
                .map_err(|error| error.to_string())?;
        }
        let csv = writer.into_inner().map_err(|error| error.to_string())?;
        sheets.push((name, range.height(), csv));
    }
    if sheets.is_empty() {
        return Err("Every sheet in this spreadsheet is empty".into());
    }
    let single = sheets.len() == 1;
    let mut outputs = Vec::new();
    let mut summary = Vec::new();
    for (name, rows, csv) in &sheets {
        let file_name = if single {
            output_name(request, &format!("{stem}.csv"))?
        } else {
            format!("{stem} - {}.csv", sheet_file_part(name))
        };
        outputs.push(publish_bytes(request, runtime, csv, &file_name, cancelled)?);
        summary.push(format!("{name} ({rows} rows)"));
    }
    let mut warnings = Vec::new();
    if !skipped.is_empty() {
        warnings.push(format!("Skipped empty sheets: {}", skipped.join(", ")));
    }
    Ok(success(
        manifest,
        outputs,
        Some(format!(
            "Saved {} CSV file{}: {}",
            sheets.len(),
            if single { "" } else { "s" },
            summary.join(", ")
        )),
        warnings,
    ))
}

/// Cell text as a spreadsheet user sees it: whole numbers without `.0` and
/// dates in ISO form.
fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Float(value) if value.fract() == 0.0 && value.abs() < 1e15 => format!("{value:.0}"),
        Data::DateTime(value) => match value.as_datetime() {
            Some(moment) if moment.time() == chrono::NaiveTime::MIN => moment.date().to_string(),
            Some(moment) => moment.format("%Y-%m-%d %H:%M:%S").to_string(),
            None => cell.to_string(),
        },
        Data::Bool(value) => if *value { "TRUE" } else { "FALSE" }.into(),
        _ => cell.to_string(),
    }
}

fn sheet_file_part(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_control() || "/\\<>:\"|?*".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_owned()
}

fn csv_to_workbook(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    inputs: &[ToolValue],
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let detect_numbers = option_bool(request, "detectNumbers", true);
    let header = option_bool(request, "header", true);
    let bold = Format::new().set_bold();
    let mut workbook = Workbook::new();
    let mut used_names = HashSet::new();
    let mut summary = Vec::new();
    let mut warnings = Vec::new();
    for input in inputs {
        check_cancelled(cancelled)?;
        let bytes = input_bytes(input, runtime, MAX_INPUT_BYTES, cancelled)?;
        let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))
            .into_owned();
        let separator = if input.mime == "file/tsv" {
            b'\t'
        } else {
            sniff_delimiter(&text)
        };
        let stem = input_stem(runtime, input, "Sheet");
        let sheet_name = unique_sheet_name(&stem, &mut used_names);
        let sheet = workbook.add_worksheet();
        sheet
            .set_name(&sheet_name)
            .map_err(|error| error.to_string())?;
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(separator)
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut rows = 0usize;
        for record in reader.records() {
            if rows.is_multiple_of(4096) {
                check_cancelled(cancelled)?;
            }
            let record = record.map_err(|error| format!("{stem}: {error}"))?;
            if rows >= MAX_ROWS {
                warnings.push(format!(
                    "{stem}: stopped at Excel's limit of {MAX_ROWS} rows"
                ));
                break;
            }
            for (column, value) in record.iter().enumerate().take(MAX_COLUMNS) {
                let (row, column) = (rows as u32, column as u16);
                let number = (detect_numbers && !(header && rows == 0))
                    .then(|| as_number(value))
                    .flatten();
                let written = match number {
                    Some(number) => sheet.write_number(row, column, number).map(|_| ()),
                    None if header && rows == 0 => sheet
                        .write_string_with_format(row, column, value, &bold)
                        .map(|_| ()),
                    None if value.is_empty() => Ok(()),
                    None => sheet.write_string(row, column, value).map(|_| ()),
                };
                written.map_err(|error| format!("{stem}: {error}"))?;
            }
            rows += 1;
        }
        if header && rows > 1 {
            sheet
                .set_freeze_panes(1, 0)
                .map_err(|error| error.to_string())?;
        }
        sheet.set_autofit_max_width(400).autofit();
        summary.push(format!("{sheet_name} ({rows} rows)"));
    }
    let bytes = workbook
        .save_to_buffer()
        .map_err(|error| format!("Could not build the workbook: {error}"))?;
    let fallback = if inputs.len() == 1 {
        format!("{}.xlsx", input_stem(runtime, &inputs[0], "workbook"))
    } else {
        "Combined.xlsx".to_owned()
    };
    let name = output_name(request, &fallback)?;
    let output = publish_bytes(request, runtime, &bytes, &name, cancelled)?;
    Ok(success(
        manifest,
        vec![output],
        Some(format!(
            "Built a workbook with {} sheet{}: {}",
            summary.len(),
            if summary.len() == 1 { "" } else { "s" },
            summary.join(", ")
        )),
        warnings,
    ))
}

/// The most frequent of comma, semicolon, and tab on the first line.
fn sniff_delimiter(text: &str) -> u8 {
    let first = text.lines().next().unwrap_or_default();
    (*b",;\t")
        .into_iter()
        .max_by_key(|candidate| first.bytes().filter(|byte| byte == candidate).count())
        .unwrap_or(b',')
}

/// A plain number, keeping codes with leading zeros (007, phone numbers) and
/// long digit strings (card or ID numbers) as text.
fn as_number(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    let digits = trimmed.trim_start_matches(['-', '+']);
    if trimmed.is_empty()
        || trimmed.len() > 15
        || (digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0."))
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+' | b'e' | b'E'))
    {
        return None;
    }
    trimmed
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// Excel sheet names: at most 31 characters, none of `[]:*?/\`, unique.
fn unique_sheet_name(stem: &str, used: &mut HashSet<String>) -> String {
    let base = stem
        .chars()
        .map(|ch| if "[]:*?/\\".contains(ch) { '_' } else { ch })
        .take(31)
        .collect::<String>();
    let base = match base.trim_matches('\'') {
        "" => "Sheet",
        name => name,
    }
    .to_owned();
    let mut name = base.clone();
    let mut number = 2;
    while !used.insert(name.to_lowercase()) {
        let suffix = format!(" ({number})");
        name = format!(
            "{}{suffix}",
            base.chars().take(31 - suffix.len()).collect::<String>()
        );
        number += 1;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_detection_keeps_codes_as_text() {
        assert_eq!(as_number("42"), Some(42.0));
        assert_eq!(as_number("-3.5"), Some(-3.5));
        assert_eq!(as_number("0.25"), Some(0.25));
        assert_eq!(as_number("007"), None);
        assert_eq!(as_number("4111111111111111"), None);
        assert_eq!(as_number("12 apples"), None);
    }

    #[test]
    fn sheet_names_are_valid_and_unique() {
        let mut used = HashSet::new();
        assert_eq!(unique_sheet_name("sales/2026", &mut used), "sales_2026");
        assert_eq!(unique_sheet_name("Sales_2026", &mut used), "Sales_2026 (2)");
        let long = "x".repeat(40);
        assert_eq!(unique_sheet_name(&long, &mut used).len(), 31);
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3"), b';');
    }
}
