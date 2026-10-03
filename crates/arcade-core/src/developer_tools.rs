//! Local developer utilities backed by bounded parsers and shared providers.

use crate::{
    Arcade,
    tool_kit::{
        check_cancelled, input_bytes, json_result, option_str, output_name, publish_bytes,
        single_text, success, text_result,
    },
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use data_encoding::BASE32_NOPAD;
use rand::RngCore;
use regex::RegexBuilder;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256, Sha512};
use std::{collections::BTreeSet, io::Read, sync::atomic::AtomicBool};
use uuid::Uuid;

const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    match manifest.id.as_str() {
        "arcade.developer.regex" => regex_playground(manifest, request, runtime, cancelled),
        "arcade.developer.encoding" => encoding_tool(manifest, request, runtime, cancelled),
        "arcade.developer.url" => url_tool(manifest, request, runtime, cancelled),
        "arcade.developer.identifier" => identifier_tool(manifest, request),
        "arcade.developer.jwt" => jwt_inspector(manifest, request, runtime, cancelled),
        "arcade.developer.cron" => cron_explainer(manifest, request, runtime, cancelled),
        "arcade.developer.hash" => hash_tool(manifest, request, runtime, cancelled),
        _ => Err(format!(
            "No developer executor is registered for {}",
            manifest.id
        )),
    }
}

fn regex_playground(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = single_text(request, runtime, MAX_TEXT_BYTES, cancelled)?;
    let (pattern, text, replacement, flags) = if request.inputs[0].mime == "structured/regex-query"
    {
        let value: Value = serde_json::from_str(&source)
            .map_err(|error| format!("Regex query JSON is invalid: {error}"))?;
        (
            value
                .get("pattern")
                .and_then(Value::as_str)
                .ok_or("Regex query needs a string `pattern`")?
                .to_owned(),
            value
                .get("text")
                .and_then(Value::as_str)
                .ok_or("Regex query needs a string `text`")?
                .to_owned(),
            value
                .get("replacement")
                .and_then(Value::as_str)
                .map(str::to_owned),
            value
                .get("flags")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        )
    } else {
        (
            option_str(request, "pattern", "").to_owned(),
            source,
            request
                .options
                .get("replacement")
                .and_then(Value::as_str)
                .map(str::to_owned),
            option_str(request, "flags", "").to_owned(),
        )
    };
    if pattern.is_empty() {
        return Err("Enter a regular expression".into());
    }
    if pattern.len() > 64 * 1024 || text.len() > MAX_TEXT_BYTES {
        return Err("The regular expression or test text is too large".into());
    }
    let mut builder = RegexBuilder::new(&pattern);
    builder
        .case_insensitive(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dot_matches_new_line(flags.contains('s'))
        .ignore_whitespace(flags.contains('x'))
        .size_limit(10 * 1024 * 1024)
        .dfa_size_limit(10 * 1024 * 1024);
    let regex = builder
        .build()
        .map_err(|error| format!("Regular expression: {error}"))?;
    let mut matches = Vec::new();
    for captures in regex.captures_iter(&text).take(10_000) {
        check_cancelled(cancelled)?;
        let whole = captures
            .get(0)
            .ok_or("Regex engine returned an invalid match")?;
        let groups = captures
            .iter()
            .enumerate()
            .skip(1)
            .map(|(index, capture)| {
                let name = regex
                    .capture_names()
                    .nth(index)
                    .flatten()
                    .map(str::to_owned);
                json!({
                    "index": index,
                    "name": name,
                    "value": capture.map(|value| value.as_str()),
                    "startByte": capture.map(|value| value.start()),
                    "endByte": capture.map(|value| value.end()),
                })
            })
            .collect::<Vec<_>>();
        matches.push(json!({
            "value": whole.as_str(),
            "startByte": whole.start(),
            "endByte": whole.end(),
            "groups": groups,
        }));
    }
    let replacement_result = replacement.map(|replacement| {
        if flags.contains('g') {
            regex.replace_all(&text, replacement.as_str()).into_owned()
        } else {
            regex.replace(&text, replacement.as_str()).into_owned()
        }
    });
    let value = json!({
        "engine": "Rust regex (Unicode-aware, linear-time matching)",
        "limitations": "Look-around and backreferences are not supported by this regex engine.",
        "pattern": pattern,
        "flags": flags,
        "matched": !matches.is_empty(),
        "matchCount": matches.len(),
        "matches": matches,
        "replacement": replacement_result,
        "truncated": matches.len() == 10_000,
    });
    Ok(json_result(manifest, value, "structured/regex-result"))
}

fn encoding_tool(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let [input] = request.inputs.as_slice() else {
        return Err("Provide one text value or selected file".into());
    };
    let action = option_str(request, "action", "encode");
    let encoding = option_str(request, "encoding", "base64");
    if !matches!(encoding, "base64" | "base32" | "hex" | "binary") {
        return Err("Choose Base64, Base32, hexadecimal, or binary encoding".into());
    }
    if action == "encode" {
        let bytes = input_bytes(input, runtime, MAX_FILE_BYTES, cancelled)?;
        let encoded = encode_bytes(&bytes, encoding)?;
        if encoded.len() > MAX_TEXT_BYTES {
            return Err("Encoded output exceeds the 16 MB text limit".into());
        }
        return Ok(text_result(manifest, encoded, "text/plain"));
    }
    if action != "decode" {
        return Err("Choose encode or decode".into());
    }
    if input.kind != ValueKind::Text {
        return Err("Decode expects encoded text".into());
    }
    let decoded = decode_bytes(&input.value, encoding)?;
    check_cancelled(cancelled)?;
    if let Ok(text) = String::from_utf8(decoded.clone()) {
        return Ok(text_result(manifest, text, "text/plain"));
    }
    let name = output_name(request, "decoded.bin")?;
    let artifact = publish_bytes(request, runtime, &decoded, &name, cancelled)?;
    Ok(success(manifest, vec![artifact], None, vec![]))
}

fn hash_tool(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let [input] = request.inputs.as_slice() else {
        return Err("Enter text or select one file to hash".into());
    };
    let algorithm = option_str(request, "algorithm", "sha256");
    let digest = match input.kind {
        ValueKind::Artifact | ValueKind::File => {
            let file = runtime
                .grants()
                .open_scoped(&input.value)
                .map_err(|error| error.to_string())?;
            digest_reader(file, algorithm, cancelled)?
        }
        _ => digest_reader(input.value.as_bytes(), algorithm, cancelled)?,
    };
    let expected = option_str(request, "expected", "")
        .trim()
        .to_ascii_lowercase();
    if expected.is_empty() {
        return Ok(text_result(manifest, digest, "text/plain"));
    }
    let matches = expected == digest;
    let message = if matches {
        "Checksum matches".to_owned()
    } else {
        "Checksum does NOT match the expected value".to_owned()
    };
    let mut result = success(
        manifest,
        vec![ToolValue::text(digest, "text/plain")],
        Some(message.clone()),
        vec![],
    );
    if !matches {
        result.warnings.push(message);
    }
    Ok(result)
}

/// Stream `reader` through SHA-256, SHA-512, or BLAKE3 and return lowercase hex.
pub(crate) fn digest_reader(
    mut reader: impl Read,
    algorithm: &str,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    enum Digest {
        Sha256(Sha256),
        Sha512(Sha512),
        Blake3(Box<blake3::Hasher>),
    }
    let mut digest = match algorithm {
        "sha256" => Digest::Sha256(Sha256::new()),
        "sha512" => Digest::Sha512(Sha512::new()),
        "blake3" => Digest::Blake3(Box::new(blake3::Hasher::new())),
        _ => return Err(format!("Unsupported hash algorithm: {algorithm}")),
    };
    let mut chunk = vec![0u8; 1024 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let count = reader.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        match &mut digest {
            Digest::Sha256(hasher) => hasher.update(&chunk[..count]),
            Digest::Sha512(hasher) => hasher.update(&chunk[..count]),
            Digest::Blake3(hasher) => {
                hasher.update(&chunk[..count]);
            }
        }
    }
    Ok(match digest {
        Digest::Sha256(hasher) => format!("{:x}", hasher.finalize()),
        Digest::Sha512(hasher) => format!("{:x}", hasher.finalize()),
        Digest::Blake3(hasher) => hasher.finalize().to_hex().to_string(),
    })
}

fn encode_bytes(bytes: &[u8], encoding: &str) -> Result<String, String> {
    match encoding {
        "base64" => Ok(BASE64.encode(bytes)),
        "base32" => Ok(BASE32_NOPAD.encode(bytes)),
        "hex" => Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect()),
        "binary" => Ok(bytes
            .iter()
            .map(|byte| format!("{byte:08b}"))
            .collect::<Vec<_>>()
            .join(" ")),
        _ => Err("Unsupported encoding".into()),
    }
}

fn decode_bytes(input: &str, encoding: &str) -> Result<Vec<u8>, String> {
    let compact = input
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>();
    match encoding {
        "base64" => BASE64
            .decode(compact.as_bytes())
            .map_err(|_| "Input is not valid Base64".into()),
        "base32" => BASE32_NOPAD
            .decode(compact.to_ascii_uppercase().as_bytes())
            .map_err(|_| "Input is not valid Base32".into()),
        "hex" => {
            if compact.len() % 2 != 0 {
                return Err("Hex input needs an even number of digits".into());
            }
            compact
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let high = hex_digit(pair[0])
                        .ok_or("Hex input contains a non-hexadecimal character")?;
                    let low = hex_digit(pair[1])
                        .ok_or("Hex input contains a non-hexadecimal character")?;
                    Ok((high << 4) | low)
                })
                .collect()
        }
        "binary" => compact
            .as_bytes()
            .chunks_exact(8)
            .map(|bits| {
                let mut value = 0u8;
                for bit in bits {
                    value = value.checked_mul(2).ok_or("Binary value is out of range")?;
                    match bit {
                        b'0' => {}
                        b'1' => value += 1,
                        _ => return Err("Binary input must contain only 0 and 1".into()),
                    }
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, String>>()
            .and_then(|bytes| {
                if compact.len() % 8 != 0 {
                    Err("Binary input length must be a multiple of eight".into())
                } else {
                    Ok(bytes)
                }
            }),
        _ => Err("Unsupported encoding".into()),
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn url_tool(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let input = single_text(request, runtime, MAX_TEXT_BYTES, cancelled)?;
    match option_str(request, "action", "inspect") {
        "encode" => Ok(text_result(
            manifest,
            percent_encode(input.as_bytes()),
            "text/url",
        )),
        "decode" => {
            let decoded = percent_decode(&input)?;
            Ok(text_result(manifest, decoded, "text/url"))
        }
        "inspect" => {
            let parsed = url::Url::parse(input.trim()).map_err(|error| format!("URL: {error}"))?;
            if !matches!(
                parsed.scheme(),
                "http" | "https" | "ftp" | "file" | "mailto"
            ) {
                return Err("This URL scheme is not supported by the inspector".into());
            }
            let pairs = parsed
                .query_pairs()
                .map(|(key, value)| json!({"key": key, "value": value}))
                .collect::<Vec<_>>();
            let result = json!({
                "scheme": parsed.scheme(),
                "host": parsed.host_str(),
                "port": parsed.port_or_known_default(),
                "path": parsed.path(),
                "query": pairs,
                "fragment": parsed.fragment(),
                "credentialsPresent": !parsed.username().is_empty() || parsed.password().is_some(),
                "safeDisplay": safe_url_display(&parsed),
            });
            Ok(json_result(manifest, result, "structured/url-query"))
        }
        _ => Err("Choose encode, decode, or inspect".into()),
    }
}

fn percent_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len());
    for byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~') {
            output.push(*byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    output
}

fn percent_decode(input: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("URL ends with an incomplete percent escape".into());
            }
            let high = hex_digit(bytes[index + 1]).ok_or("URL has an invalid percent escape")?;
            let low = hex_digit(bytes[index + 2]).ok_or("URL has an invalid percent escape")?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(if bytes[index] == b'+' {
                b' '
            } else {
                bytes[index]
            });
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| "Decoded URL text is not valid UTF-8".into())
}

fn safe_url_display(parsed: &url::Url) -> String {
    let mut copy = parsed.clone();
    if !copy.username().is_empty() || copy.password().is_some() {
        let _ = copy.set_username("");
        let _ = copy.set_password(None);
    }
    copy.to_string()
}

fn identifier_tool(manifest: &ToolManifest, request: &ToolRequest) -> Result<ToolResult, String> {
    let count = request
        .options
        .get("count")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if !(1..=1000).contains(&count) {
        return Err("Generate between 1 and 1,000 identifiers".into());
    }
    let format = option_str(request, "format", "uuid-v4");
    let mut output = Vec::with_capacity(count as usize);
    for _ in 0..count {
        output.push(match format {
            "uuid-v4" | "uuid" => Uuid::new_v4().to_string(),
            "ulid" => new_ulid(),
            _ => return Err("Choose UUID v4 or ULID".into()),
        });
    }
    Ok(text_result(manifest, output.join("\n"), "text/plain"))
}

fn new_ulid() -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let timestamp = Utc::now().timestamp_millis().clamp(0, (1i64 << 48) - 1) as u128;
    let mut random = [0u8; 10];
    rand::rng().fill_bytes(&mut random);
    let mut value = timestamp << 80;
    for (index, byte) in random.iter().enumerate() {
        value |= (*byte as u128) << (72 - index * 8);
    }
    let mut output = [b'0'; 26];
    for index in (0..26).rev() {
        output[index] = ALPHABET[(value & 31) as usize];
        value >>= 5;
    }
    String::from_utf8(output.to_vec()).unwrap_or_default()
}

fn jwt_inspector(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let token = single_text(request, runtime, 64 * 1024, cancelled)?;
    let parts = token.trim().split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("A signed JWT needs header, payload, and signature sections".into());
    }
    let header = decode_jwt_section(parts[0])?;
    let payload = decode_jwt_section(parts[1])?;
    let header_json: Value =
        serde_json::from_slice(&header).map_err(|error| format!("JWT header JSON: {error}"))?;
    let payload_json: Value =
        serde_json::from_slice(&payload).map_err(|error| format!("JWT payload JSON: {error}"))?;
    let times = ["exp", "iat", "nbf"]
        .into_iter()
        .filter_map(|name| {
            let seconds = payload_json.get(name)?.as_i64()?;
            let iso = DateTime::<Utc>::from_timestamp(seconds, 0).map(|time| time.to_rfc3339());
            Some((
                name.to_owned(),
                json!({"unixSeconds": seconds, "isoUtc": iso}),
            ))
        })
        .collect::<serde_json::Map<_, _>>();
    let value = json!({
        "header": header_json,
        "payload": payload_json,
        "signatureBytes": decode_jwt_section(parts[2]).map(|bytes| bytes.len()).unwrap_or(0),
        "registeredTimes": times,
        "signatureVerified": false,
        "note": "The token was decoded locally. Its signature, issuer, audience, and claims are not verified.",
    });
    let mut result = json_result(manifest, value, "structured/jwt");
    result
        .warnings
        .push("Decoded locally; signature and claims were not verified.".into());
    Ok(result)
}

fn decode_jwt_section(value: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| URL_SAFE.decode(value))
        .map_err(|_| "JWT contains invalid Base64URL data".into())
}

fn cron_explainer(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let expression = single_text(request, runtime, 4096, cancelled)?;
    let parts = expression.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 5 {
        return Err("Enter a standard five-field cron expression: minute hour day-of-month month day-of-week".into());
    }
    let minute = CronField::parse(parts[0], 0, 59, false)?;
    let hour = CronField::parse(parts[1], 0, 23, false)?;
    let day = CronField::parse(parts[2], 1, 31, false)?;
    let month = CronField::parse(parts[3], 1, 12, false)?;
    let weekday = CronField::parse(parts[4], 0, 7, true)?;
    let zone: Tz = option_str(request, "timeZone", "UTC")
        .parse()
        .map_err(|_| "Choose a valid IANA time zone, for example UTC or Asia/Kolkata")?;
    let mut next = Vec::new();
    let start = Utc::now()
        .with_second(0)
        .unwrap_or_else(Utc::now)
        .with_nanosecond(0)
        .unwrap_or_else(Utc::now)
        + chrono::Duration::minutes(1);
    for offset in 0..(2 * 366 * 24 * 60) {
        if offset % 4096 == 0 {
            check_cancelled(cancelled)?;
        }
        let instant = start + chrono::Duration::minutes(offset);
        let local = instant.with_timezone(&zone);
        let dom = day.contains(local.day() as usize);
        let dow = weekday.contains(local.weekday().num_days_from_sunday() as usize);
        let day_match = match (day.is_wildcard(), weekday.is_wildcard()) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        };
        if minute.contains(local.minute() as usize)
            && hour.contains(local.hour() as usize)
            && month.contains(local.month() as usize)
            && day_match
        {
            next.push(local.to_rfc3339());
            if next.len() == 5 {
                break;
            }
        }
    }
    if next.is_empty() {
        return Err("No matching occurrence was found within the next two years".into());
    }
    let description = format!(
        "Five-field cron schedule in {zone}: minute {}, hour {}, day-of-month {}, month {}, day-of-week {}.",
        parts[0], parts[1], parts[2], parts[3], parts[4]
    );
    Ok(json_result(
        manifest,
        json!({"expression": expression.trim(), "dialect": "Unix five-field cron", "timeZone": zone.name(), "description": description, "nextOccurrences": next}),
        "structured/cron",
    ))
}

#[derive(Clone)]
struct CronField {
    values: BTreeSet<usize>,
    wildcard: bool,
}

impl CronField {
    fn parse(field: &str, min: usize, max: usize, weekday: bool) -> Result<Self, String> {
        let wildcard = field == "*" || field.starts_with("*/");
        let mut values = BTreeSet::new();
        for segment in field.split(',') {
            let (range, step) = match segment.split_once('/') {
                Some((range, step)) => (
                    range,
                    step.parse::<usize>()
                        .map_err(|_| "Cron step must be a positive integer")?,
                ),
                None => (segment, 1),
            };
            if step == 0 {
                return Err("Cron step must be greater than zero".into());
            }
            let (start, end) = if range == "*" {
                (min, max)
            } else if let Some((start, end)) = range.split_once('-') {
                (
                    parse_cron_number(start, min, max)?,
                    parse_cron_number(end, min, max)?,
                )
            } else {
                let value = parse_cron_number(range, min, max)?;
                (value, value)
            };
            if start > end {
                return Err("Cron ranges must ascend".into());
            }
            let mut value = start;
            while value <= end {
                values.insert(if weekday && value == 7 { 0 } else { value });
                let Some(next) = value.checked_add(step) else {
                    break;
                };
                value = next;
            }
        }
        Ok(Self { values, wildcard })
    }
    fn contains(&self, value: usize) -> bool {
        self.values.contains(&value)
    }
    fn is_wildcard(&self) -> bool {
        self.wildcard
    }
}

fn parse_cron_number(value: &str, min: usize, max: usize) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| "Cron fields must use numeric values in the supported five-field dialect")?;
    if parsed < min || parsed > max {
        return Err(format!("Cron field value must be between {min} and {max}"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_round_trips_common_formats() {
        let input = b"Arcade Box\0data";
        for format in ["base64", "base32", "hex", "binary"] {
            assert_eq!(
                decode_bytes(&encode_bytes(input, format).unwrap(), format).unwrap(),
                input
            );
        }
    }

    #[test]
    fn url_encoding_is_component_safe_and_decodes_utf8() {
        let encoded = percent_encode("a b/क".as_bytes());
        assert_eq!(encoded, "a%20b%2F%E0%A4%95");
        assert_eq!(percent_decode(&encoded).unwrap(), "a b/क");
        assert!(percent_decode("bad%2").is_err());
    }

    #[test]
    fn ulid_has_canonical_width_and_alphabet() {
        let value = new_ulid();
        assert_eq!(value.len(), 26);
        assert!(
            value
                .bytes()
                .all(|byte| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte))
        );
    }

    #[test]
    fn cron_parser_handles_lists_steps_and_sunday_alias() {
        let field = CronField::parse("*/15", 0, 59, false).unwrap();
        assert!(field.contains(30));
        assert!(!field.contains(31));
        let sunday = CronField::parse("0,7", 0, 7, true).unwrap();
        assert!(sunday.contains(0));
    }
}
