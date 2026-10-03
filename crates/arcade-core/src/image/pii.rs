//! Local OCR suggestions for regions that may hold personal information
//! (emails, phone numbers, card and ID numbers). Matched text never leaves
//! this module; only its kind and a normalized bounding box are returned.

use crate::{
    process::{self, ProcessSpec},
    provider::discover_tesseract,
};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{OnceLock, atomic::AtomicBool},
    time::Duration,
};

const MAX_SUGGESTIONS: usize = 64;

pub(super) fn suggest(
    source: &Path,
    dimensions: (u32, u32),
    language: &str,
    cancelled: &AtomicBool,
) -> Result<Value, String> {
    if language.is_empty()
        || language.len() > 64
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'+')
    {
        return Err("Choose a valid OCR language code".into());
    }
    let provider = discover_tesseract()
        .into_iter()
        .find(|provider| {
            provider.compatible
                && language.split('+').all(|code| {
                    provider
                        .capabilities
                        .iter()
                        .any(|capability| capability == &format!("ocr:language:{code}"))
                })
        })
        .ok_or_else(|| {
            format!(
                "Finding personal info needs Tesseract OCR with the `{language}` language installed"
            )
        })?;
    let output = process::run(
        &ProcessSpec {
            executable: provider.executable_path.clone(),
            args: vec![
                source.as_os_str().to_os_string(),
                "stdout".into(),
                "-l".into(),
                language.into(),
                "tsv".into(),
            ],
            current_dir: None,
            timeout: Duration::from_secs(15 * 60),
            output_limit: 16 * 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| format!("Local OCR could not complete: {error}"))?;
    if !output.status.success() {
        return Err("Local OCR could not read this image".into());
    }
    let tsv = String::from_utf8(output.stdout).map_err(|_| "Local OCR returned invalid text")?;
    let suggestions = suggestions_from_tsv(&tsv, dimensions);
    Ok(json!({
        "suggestions": suggestions,
        "count": suggestions.len(),
        "message": if suggestions.is_empty() {
            "No emails, phone numbers, or card/ID numbers were found. OCR can miss text, so check the image yourself."
        } else {
            "Review the suggested boxes. OCR can miss sensitive text or mark ordinary text."
        },
    }))
}

struct Word {
    start: usize,
    end: usize,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

fn patterns() -> &'static [(&'static str, Regex)] {
    static PATTERNS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            (
                "email address",
                r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b",
            ),
            ("card number", r"\b(?:\d[ -]?){12,18}\d\b"),
            ("ID number", r"\b\d{4}[ -]?\d{4}[ -]?\d{4}\b"),
            ("ID number", r"\b[A-Z]{5}\d{4}[A-Z]\b"),
            ("ID number", r"\b\d{3}-\d{2}-\d{4}\b"),
            ("phone number", r"\+?\(?\d[\d ().-]{6,}\d"),
        ]
        .into_iter()
        .map(|(kind, pattern)| (kind, Regex::new(pattern).expect("static PII pattern")))
        .collect()
    })
}

fn luhn_valid(digits: &str) -> bool {
    let digits = digits
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|byte| u32::from(byte - b'0'))
        .collect::<Vec<_>>();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            if index % 2 == 1 {
                let doubled = digit * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                *digit
            }
        })
        .sum();
    sum % 10 == 0
}

/// Group OCR words into lines, match patterns against each line's text, and
/// return padded, normalized boxes covering the matching words.
fn suggestions_from_tsv(tsv: &str, (width, height): (u32, u32)) -> Vec<Value> {
    let mut lines = BTreeMap::<(u32, u32, u32, u32), (String, Vec<Word>)>::new();
    for row in tsv.lines().skip(1) {
        let fields = row.splitn(12, '\t').collect::<Vec<_>>();
        if fields.len() < 12 || fields[0] != "5" {
            continue;
        }
        let number = |index: usize| fields[index].parse::<u32>().ok();
        let (Some(page), Some(block), Some(paragraph), Some(line)) =
            (number(1), number(2), number(3), number(4))
        else {
            continue;
        };
        let (Some(left), Some(top), Some(word_width), Some(word_height)) =
            (number(6), number(7), number(8), number(9))
        else {
            continue;
        };
        let text = fields[11].trim();
        if text.is_empty() {
            continue;
        }
        let (line_text, words) = lines.entry((page, block, paragraph, line)).or_default();
        if !line_text.is_empty() {
            line_text.push(' ');
        }
        let start = line_text.len();
        line_text.push_str(text);
        words.push(Word {
            start,
            end: line_text.len(),
            left,
            top,
            right: left.saturating_add(word_width),
            bottom: top.saturating_add(word_height),
        });
    }
    let mut found = BTreeMap::<(u32, u32, u32, u32), &'static str>::new();
    for (text, words) in lines.values() {
        let mut claimed = Vec::<(usize, usize)>::new();
        for (kind, pattern) in patterns() {
            for matched in pattern.find_iter(text) {
                let range = (matched.start(), matched.end());
                if claimed
                    .iter()
                    .any(|(start, end)| range.0 < *end && *start < range.1)
                {
                    continue;
                }
                let digits = matched.as_str().bytes().filter(u8::is_ascii_digit).count();
                let accept = match *kind {
                    "card number" => luhn_valid(matched.as_str()),
                    "phone number" => (8..=15).contains(&digits),
                    _ => true,
                };
                if !accept {
                    continue;
                }
                let covered = words
                    .iter()
                    .filter(|word| word.start < range.1 && range.0 < word.end)
                    .collect::<Vec<_>>();
                let Some(first) = covered.first() else {
                    continue;
                };
                let mut bounds = (first.left, first.top, first.right, first.bottom);
                for word in &covered[1..] {
                    bounds = (
                        bounds.0.min(word.left),
                        bounds.1.min(word.top),
                        bounds.2.max(word.right),
                        bounds.3.max(word.bottom),
                    );
                }
                claimed.push(range);
                found.insert(bounds, kind);
            }
        }
    }
    found
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|((left, top, right, bottom), kind)| {
            let pad_x = ((right - left) / 20).max(2);
            let pad_y = ((bottom - top) / 4).max(2);
            let x = left.saturating_sub(pad_x);
            let y = top.saturating_sub(pad_y);
            let right = right.saturating_add(pad_x).min(width);
            let bottom = bottom.saturating_add(pad_y).min(height);
            let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
            json!({
                "kind": kind,
                "region": {
                    "x": f64::from(x) / w,
                    "y": f64::from(y) / h,
                    "width": f64::from(right.saturating_sub(x)) / w,
                    "height": f64::from(bottom.saturating_sub(y)) / h,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(line: u32, word: u32, left: u32, text: &str) -> String {
        format!("5\t1\t1\t1\t{line}\t{word}\t{left}\t10\t40\t12\t95\t{text}")
    }

    #[test]
    fn line_level_matching_finds_split_numbers_and_hides_text() {
        let tsv = [
            "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext".to_owned(),
            word(1, 1, 10, "Call"),
            word(1, 2, 60, "+91"),
            word(1, 3, 110, "98765"),
            word(1, 4, 160, "43210"),
            word(2, 1, 10, "mail:"),
            word(2, 2, 60, "ada@example.com"),
            word(3, 1, 10, "4111"),
            word(3, 2, 60, "1111"),
            word(3, 3, 110, "1111"),
            word(3, 4, 160, "1111"),
            word(4, 1, 10, "Invoice"),
            word(4, 2, 60, "2024"),
        ]
        .join("\n");
        let found = suggestions_from_tsv(&tsv, (400, 100));
        let kinds = found
            .iter()
            .map(|item| item["kind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(kinds.contains(&"phone number"));
        assert!(kinds.contains(&"email address"));
        assert!(kinds.contains(&"card number"));
        let phone = found
            .iter()
            .find(|item| item["kind"] == "phone number")
            .unwrap();
        assert!(phone["region"]["width"].as_f64().unwrap() > 0.3);
        assert!(!serde_json::to_string(&found).unwrap().contains("ada@"));
    }
}
