//! Line-oriented cleanup and case conversion.

use std::{cmp::Ordering, collections::HashSet};

/// Options for [`clean`], one per "Clean Up Text" control.
pub(super) struct CleanOptions<'a> {
    pub trim: bool,
    pub collapse: bool,
    pub remove_blank: bool,
    pub unique: bool,
    pub ignore_case: bool,
    pub sort: &'a str,
    pub tabs: &'a str,
    pub crlf: bool,
}

/// Apply every enabled cleanup in a fixed order: tabs, trim, collapse,
/// blank-line removal, de-duplication, ordering, and line endings.
pub(super) fn clean(input: &str, options: &CleanOptions<'_>) -> Result<String, String> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let tab = match options.tabs {
        "keep" => None,
        "2" => Some("  "),
        "4" => Some("    "),
        other => return Err(format!("Unknown tab option: {other}")),
    };
    let mut lines = normalized
        .split('\n')
        .map(|line| {
            let mut line = match tab {
                Some(spaces) => line.replace('\t', spaces),
                None => line.to_owned(),
            };
            if options.trim {
                line = line.trim().to_owned();
            }
            if options.collapse {
                line = collapse_spaces(&line);
            }
            line
        })
        .collect::<Vec<_>>();
    if normalized.ends_with('\n') {
        lines.pop();
    }
    if options.remove_blank {
        lines.retain(|line| !line.trim().is_empty());
    }
    if options.unique {
        let mut seen = HashSet::new();
        lines.retain(|line| {
            let key = if options.ignore_case {
                line.to_lowercase()
            } else {
                line.clone()
            };
            seen.insert(key)
        });
    }
    match options.sort {
        "keep" => {}
        "natural" => lines.sort_by(|a, b| natural_cmp(a, b)),
        "natural-desc" => lines.sort_by(|a, b| natural_cmp(b, a)),
        "numeric" => lines.sort_by(|a, b| numeric_cmp(a, b)),
        "length" => lines.sort_by(|a, b| {
            a.chars()
                .count()
                .cmp(&b.chars().count())
                .then_with(|| natural_cmp(a, b))
        }),
        "reverse" => lines.reverse(),
        "shuffle" => {
            use rand::seq::SliceRandom;
            lines.shuffle(&mut rand::rng());
        }
        other => return Err(format!("Unknown line order: {other}")),
    }
    let separator = if options.crlf { "\r\n" } else { "\n" };
    Ok(lines.join(separator))
}

/// Collapse runs of spaces/tabs inside a line, keeping any leading indentation.
fn collapse_spaces(line: &str) -> String {
    let body = line.trim_start_matches([' ', '\t']);
    let indent = &line[..line.len() - body.len()];
    let mut collapsed = String::with_capacity(line.len());
    collapsed.push_str(indent);
    let mut previous_space = false;
    for character in body.chars() {
        let space = character == ' ' || character == '\t';
        if !(space && previous_space) {
            collapsed.push(if space { ' ' } else { character });
        }
        previous_space = space;
    }
    collapsed
}

fn numeric_cmp(left: &str, right: &str) -> Ordering {
    let number = |value: &str| {
        let trimmed = value.trim().replace(',', "");
        trimmed.parse::<f64>().ok()
    };
    match (number(left), number(right)) {
        (Some(a), Some(b)) => a.total_cmp(&b).then_with(|| left.cmp(right)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => natural_cmp(left, right),
    }
}

/// Case-insensitive ordering that compares digit runs as numbers, so
/// "file2" sorts before "file10".
pub(crate) fn natural_cmp(left: &str, right: &str) -> Ordering {
    let left_parts = digit_parts(left);
    let right_parts = digit_parts(right);
    for (a, b) in left_parts.iter().zip(&right_parts) {
        let numeric = a.bytes().all(|byte| byte.is_ascii_digit())
            && b.bytes().all(|byte| byte.is_ascii_digit());
        let order = if numeric {
            let a_number = a.trim_start_matches('0');
            let b_number = b.trim_start_matches('0');
            a_number
                .len()
                .cmp(&b_number.len())
                .then(a_number.cmp(b_number))
                .then(a.len().cmp(&b.len()))
        } else {
            a.to_lowercase().cmp(&b.to_lowercase())
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left_parts
        .len()
        .cmp(&right_parts.len())
        .then(left.cmp(right))
}

fn digit_parts(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut digit = None;
    for (index, character) in value.char_indices() {
        let current = character.is_ascii_digit();
        if digit.is_some_and(|previous| previous != current) {
            parts.push(&value[start..index]);
            start = index;
        }
        digit = Some(current);
    }
    if start < value.len() {
        parts.push(&value[start..]);
    }
    parts
}

pub(super) fn change_case(input: &str, mode: &str) -> Result<String, String> {
    Ok(match mode {
        "upper" => input.to_uppercase(),
        "lower" => input.to_lowercase(),
        "title" => title_case(input),
        "sentence" => sentence_case(input),
        "camel" => words(input)
            .iter()
            .enumerate()
            .map(|(index, word)| {
                if index == 0 {
                    word.clone()
                } else {
                    capitalize(word)
                }
            })
            .collect(),
        "pascal" => words(input).iter().map(|word| capitalize(word)).collect(),
        "snake" => words(input).join("_"),
        "kebab" => words(input).join("-"),
        "constant" => words(input).join("_").to_uppercase(),
        _ => return Err(format!("Unknown case: {mode}")),
    })
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

fn title_case(input: &str) -> String {
    let mut new_word = true;
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        if character.is_alphabetic() {
            if new_word {
                out.extend(character.to_uppercase());
            } else {
                out.extend(character.to_lowercase());
            }
            new_word = false;
        } else {
            out.push(character);
            if !character.is_numeric() && character != '\'' && character != '’' {
                new_word = true;
            }
        }
    }
    out
}

fn sentence_case(input: &str) -> String {
    let mut new_sentence = true;
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        if character.is_alphabetic() {
            if new_sentence {
                out.extend(character.to_uppercase());
            } else {
                out.extend(character.to_lowercase());
            }
            new_sentence = false;
        } else {
            out.push(character);
            if matches!(character, '.' | '!' | '?' | '\n') {
                new_sentence = true;
            }
        }
    }
    out
}

/// Split identifiers and prose into lowercase words, breaking camelCase.
fn words(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous_lowercase = false;
    for character in input.chars() {
        if !character.is_alphanumeric() {
            if !current.is_empty() {
                parts.push(current.to_lowercase());
                current.clear();
            }
            previous_lowercase = false;
            continue;
        }
        if previous_lowercase && character.is_uppercase() && !current.is_empty() {
            parts.push(current.to_lowercase());
            current.clear();
        }
        previous_lowercase = character.is_lowercase() || character.is_numeric();
        current.push(character);
    }
    if !current.is_empty() {
        parts.push(current.to_lowercase());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(sort: &'static str) -> CleanOptions<'static> {
        CleanOptions {
            trim: true,
            collapse: false,
            remove_blank: false,
            unique: false,
            ignore_case: false,
            sort,
            tabs: "keep",
            crlf: false,
        }
    }

    #[test]
    fn cleanup_steps_combine_in_order() {
        let input = "b  item \r\n\r\nA item\nfile10\nfile2\nB  ITEM\n";
        let mut all = options("natural");
        all.collapse = true;
        all.remove_blank = true;
        all.unique = true;
        all.ignore_case = true;
        assert_eq!(clean(input, &all).unwrap(), "A item\nb item\nfile2\nfile10");
        let mut crlf = options("keep");
        crlf.crlf = true;
        assert_eq!(clean(" a \n\n b ", &crlf).unwrap(), "a\r\n\r\nb");
        assert_eq!(
            clean("10\n9\n1,000", &options("numeric")).unwrap(),
            "9\n10\n1,000"
        );
        assert_eq!(
            clean(
                "  a\t\tb",
                &CleanOptions {
                    collapse: true,
                    trim: false,
                    ..options("keep")
                }
            )
            .unwrap(),
            "  a b"
        );
    }

    #[test]
    fn cases_cover_word_boundaries_and_unicode() {
        assert_eq!(
            change_case("helloWorld TEST", "snake").unwrap(),
            "hello_world_test"
        );
        assert_eq!(change_case("hello world", "camel").unwrap(), "helloWorld");
        assert_eq!(change_case("élan vital", "title").unwrap(), "Élan Vital");
        assert_eq!(change_case("don't stop", "title").unwrap(), "Don't Stop");
        assert_eq!(
            change_case("hi. here! NEXT", "sentence").unwrap(),
            "Hi. Here! Next"
        );
        assert_eq!(
            change_case("max retry count", "constant").unwrap(),
            "MAX_RETRY_COUNT"
        );
    }
}
