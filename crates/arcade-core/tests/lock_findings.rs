//! Behaviour lock for audit findings in the core (docs/v1/audit/box.md, box-real.md), run through `Arcade`, the same
//! entry point the CLI, the Island and Link use. A test named `known_bug_*` pins a finding that is not fixed yet:
//! it is `#[should_panic(expected = "KNOWN BUG <id>")]`, so it passes while the bug is there and fails once the fix
//! lands, and the marker goes with the fix (docs/v1/DECISIONS.md D3).
use arcade_contract::{ToolRequest, ToolResult, ToolValue};
use arcade_core::{Arcade, CoreError};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;

fn runtime() -> Arcade {
    Arcade::in_memory().unwrap()
}

fn run(arcade: &Arcade, tool: &str, input: &str, options: Value) -> Result<ToolResult, CoreError> {
    arcade.run_tool(ToolRequest {
        tool_id: tool.into(),
        inputs: vec![ToolValue::text(input, "text/plain")],
        options,
    })
}

fn output(result: &ToolResult) -> String {
    result
        .outputs
        .first()
        .map(|o| o.value.clone())
        .unwrap_or_default()
}

fn json_output(result: &ToolResult) -> Value {
    serde_json::from_str(&output(result)).unwrap_or(Value::Null)
}

fn run_file(
    arcade: &Arcade,
    tool: &str,
    path: &Path,
    options: Value,
) -> Result<ToolResult, CoreError> {
    let input = arcade.grants().grant(path).unwrap().as_tool_value();
    arcade.run_tool(ToolRequest {
        tool_id: tool.into(),
        inputs: vec![input],
        options,
    })
}

fn first_result(arcade: &Arcade, query: &str) -> String {
    arcade
        .search_tools(query)
        .first()
        .map(|tool| tool.id.clone())
        .unwrap_or_default()
}

/// Panics with `KNOWN BUG <id>` and every failed check, so one test covers one finding.
fn expect_all(id: &str, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "KNOWN BUG {id}: {}",
        failures.join("; ")
    );
}

#[test]
fn a_tool_name_ranks_first() {
    let arcade = runtime();
    for (query, id) in [
        ("calculator", "arcade.convert.calculator"),
        ("unit converter", "arcade.convert.units"),
        ("my ip address", "arcade.network.ip"),
    ] {
        assert_eq!(first_result(&arcade, query), id, "{query}");
    }
}

#[test]
fn search_finds_what_people_type() {
    let arcade = runtime();
    let failures = [
        ("kg to lb", "arcade.convert.units"),
        ("14 gb to mb", "arcade.convert.units"),
        ("72 f to c", "arcade.convert.units"),
        ("6*7", "arcade.convert.calculator"),
        ("#ff8800", "arcade.convert.color"),
        ("2026-01-01", "arcade.convert.date-duration"),
        ("100 usd to inr", "arcade.convert.currency"),
        ("remove background", "arcade.image.background-remove"),
        ("resize", "arcade.image.resize"),
        ("ip", "arcade.network.ip"),
    ]
    .into_iter()
    .filter_map(|(query, id)| {
        let found = first_result(&arcade, query);
        (found != id).then(|| format!("{query:?} → {found:?}, not {id}"))
    })
    .collect();
    expect_all("BR-4", failures);
}

#[test]
fn calculator_handles_common_syntax() {
    let arcade = runtime();
    let mut failures = Vec::new();
    for (expression, expected) in [
        ("100+10%", 110.0),
        ("5!", 120.0),
        ("10 mod 3", 1.0),
        ("1,000+2", 1002.0),
        ("2**3", 8.0),
    ] {
        match run(&arcade, "arcade.convert.calculator", expression, json!({})) {
            Ok(result) if json_output(&result)["value"].as_f64() == Some(expected) => {}
            Ok(result) => {
                failures.push(format!("{expression} = {}", json_output(&result)["value"]))
            }
            Err(error) => failures.push(format!("{expression}: {error}")),
        }
    }
    let big = run(
        &arcade,
        "arcade.convert.calculator",
        "99999999999999999999*99999999999999999999",
        json!({}),
    )
    .unwrap();
    let formatted = json_output(&big)["formatted"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if formatted.contains("303786") {
        failures.push(format!("float noise in {formatted}"));
    }
    match run(
        &arcade,
        "arcade.convert.calculator",
        "sin(30)",
        json!({"angle": "degrees"}),
    ) {
        Ok(result)
            if (json_output(&result)["value"].as_f64().unwrap_or(9.0) - 0.5).abs() < 1e-9 => {}
        other => failures.push(format!("sin(30) in degrees: {other:?}")),
    }
    expect_all("B-7", failures);
}

#[test]
fn calculator_keeps_operator_precedence() {
    let result = run(&runtime(), "arcade.convert.calculator", "2+3*4", json!({})).unwrap();
    assert_eq!(json_output(&result)["value"].as_f64(), Some(14.0));
}

#[test]
fn units_read_decimal_commas_and_exponents() {
    let arcade = runtime();
    let mut failures = Vec::new();
    for (input, expected) in [("1,5 m to cm", 150.0), ("1.5e3 m to km", 1.5)] {
        match run(&arcade, "arcade.convert.units", input, json!({})) {
            Ok(result) if json_output(&result)["value"].as_f64() == Some(expected) => {}
            Ok(result) => failures.push(format!("{input} → {}", json_output(&result)["headline"])),
            Err(error) => failures.push(format!("{input}: {error}")),
        }
    }
    expect_all("B-8", failures);
}

#[test]
fn units_convert_a_plain_quantity() {
    let result = run(&runtime(), "arcade.convert.units", "5 km to m", json!({})).unwrap();
    assert_eq!(json_output(&result)["value"].as_f64(), Some(5000.0));
}

#[test]
fn a_negative_span_is_negative_in_every_unit() {
    let result = run(
        &runtime(),
        "arcade.convert.date-duration",
        "2026-12-25",
        json!({"action": "difference", "end": "2026-01-01", "timeZone": "UTC"}),
    )
    .unwrap();
    let span = json_output(&result);
    let failures = ["totalDays", "totalHours", "totalMinutes", "totalSeconds"]
        .into_iter()
        .filter(|key| span[*key].as_i64().is_none_or(|value| value >= 0))
        .map(|key| format!("{key} = {}", span[key]))
        .collect();
    expect_all("B-9", failures);
}

#[test]
fn time_zones_read_common_inputs_and_abbreviations() {
    let arcade = runtime();
    let tomorrow = (chrono::Utc::now() + chrono::Duration::days(1)).format("%Y-%m-%d");
    let mut failures = Vec::new();
    for (input, source, targets, key, expected) in [
        (
            "2026-07-01 09:00",
            "utc",
            "asia/kolkata",
            "iso",
            "2026-07-01T14:30:00+05:30".to_string(),
        ),
        ("9:00", "UTC", "Asia/Kolkata", "time", "14:30".to_string()),
        ("5pm", "UTC", "UTC", "time", "17:00".to_string()),
        (
            "tomorrow 5pm",
            "UTC",
            "UTC",
            "iso",
            format!("{tomorrow}T17:00:00Z"),
        ),
        (
            "1735689600",
            "UTC",
            "UTC",
            "iso",
            "2025-01-01T00:00:00Z".to_string(),
        ),
        (
            "2026-01-15 12:00",
            "UTC",
            "IST",
            "offset",
            "UTC+05:30".to_string(),
        ),
        (
            "2026-01-15 12:00",
            "UTC",
            "PST",
            "offset",
            "UTC-08:00".to_string(),
        ),
        (
            "2026-07-15 12:00",
            "UTC",
            "EST",
            "offset",
            "UTC-04:00".to_string(),
        ),
        (
            "2026-01-15 12:00",
            "IST",
            "UTC",
            "iso",
            "2026-01-15T06:30:00Z".to_string(),
        ),
    ] {
        let seen = run(
            &arcade,
            "arcade.convert.time-zone",
            input,
            json!({"sourceTimeZone": source, "timeZones": targets}),
        )
        .map(|result| json_output(&result)["conversions"][0][key].clone());
        match seen {
            Ok(Value::String(value)) if value.contains(&expected) => {}
            other => failures.push(format!(
                "`{input}` from {source} to {targets}: {key} {other:?}, expected {expected}"
            )),
        }
    }
    expect_all("B-9", failures);
}

#[test]
fn diff_of_identical_texts_is_empty() {
    let arcade = runtime();
    let mut failures = Vec::new();
    for input in [
        "a\n--- RIGHT ---\na\n",
        "a\r\nb\r\n--- RIGHT ---\r\na\r\nb\r\n",
    ] {
        let diff = output(&run(&arcade, "arcade.text.diff", input, json!({})).unwrap());
        if diff.lines().any(|line| {
            line.starts_with('-') && !line.starts_with("---")
                || line.starts_with('+') && !line.starts_with("+++")
        }) {
            failures.push(format!("{input:?} → {diff:?}"));
        }
    }
    expect_all("B-10", failures);
}

#[test]
fn diff_shows_a_real_change() {
    let diff = output(
        &run(
            &runtime(),
            "arcade.text.diff",
            "a\nb\n--- RIGHT ---\na\nc\n",
            json!({}),
        )
        .unwrap(),
    );
    assert!(diff.contains("-b") && diff.contains("+c"), "{diff}");
}

#[cfg(target_os = "linux")]
#[test]
fn process_finder_lists_processes_not_threads() {
    // A second thread of this test process, named so it can be found.
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("lockthreadb25".into())
        .spawn(move || {
            let _ = stopped.recv();
        })
        .unwrap();
    let result = run(
        &runtime(),
        "arcade.system.process",
        "lockthreadb25",
        json!({}),
    );
    stop.send(()).unwrap();
    thread.join().unwrap();
    let report = json_output(&result.unwrap());
    let matches = report["totalMatches"].as_u64().unwrap_or(0);
    expect_all(
        "B-25",
        if matches == 0 {
            vec![]
        } else {
            vec![format!("a thread was listed as {matches} process(es)")]
        },
    );
}

#[test]
fn numeric_values_for_string_options_are_used() {
    let arcade = runtime();
    let as_text = output(
        &run(
            &arcade,
            "arcade.text.clean",
            "\tx",
            json!({"tabs": "4", "trim": false}),
        )
        .unwrap(),
    );
    let as_number = run(
        &arcade,
        "arcade.text.clean",
        "\tx",
        json!({"tabs": 4, "trim": false}),
    )
    .map(|r| output(&r));
    let failures = match as_number {
        Ok(text) if text == as_text => vec![],
        other => vec![format!(
            "tabs=4 as a number gave {other:?}, as text {as_text:?}"
        )],
    };
    expect_all("U-1", failures);
}

#[test]
fn document_to_pdf_takes_text_and_markdown() {
    let arcade = runtime();
    let dir = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    for (name, body) in [
        ("notes.txt", "Plain notes\nsecond line\n"),
        ("notes.md", "# Title\n\n- one\n- two\n"),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, body).unwrap();
        if let Err(error) = run_file(&arcade, "arcade.pdf.convert", &path, json!({})) {
            failures.push(format!("{name}: {error}"));
        }
    }
    expect_all("BR-7", failures);
}

/// One HTTP/1.1 exchange per connection. `head` answers HEAD; GET streams `body` with or without a Content-Type.
fn serve(body: Vec<u8>, head: &'static str, content_type: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request_line = String::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            if request_line.starts_with("HEAD") {
                let _ = stream.write_all(head.as_bytes());
                continue;
            }
            let content_type = if content_type {
                "Content-Type: application/octet-stream\r\n"
            } else {
                ""
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{content_type}Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{address}/file.bin")
}

fn download(arcade: &Arcade, url: &str) -> Result<usize, String> {
    let result = arcade
        .run_tool(ToolRequest {
            tool_id: "arcade.web.file-downloader".into(),
            inputs: vec![ToolValue {
                kind: arcade_contract::ValueKind::Url,
                value: url.into(),
                mime: "network/url".into(),
            }],
            options: json!({"outputName": "file.bin"}),
        })
        .map_err(|error| error.to_string())?;
    let file = result
        .outputs
        .iter()
        .find(|o| o.kind != arcade_contract::ValueKind::Text)
        .ok_or_else(|| format!("no file: {:?}", result.message))?;
    let path = arcade
        .grants()
        .resolve(&file.value)
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    Ok(bytes.len())
}

const BODY: usize = 64 * 1024;

#[test]
fn a_complete_download_with_a_content_type_is_kept() {
    let url = serve(
        vec![7; BODY],
        "HTTP/1.1 200 OK\r\nContent-Length: 65536\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
        true,
    );
    assert_eq!(download(&runtime(), &url), Ok(BODY));
}

#[test]
fn a_failed_head_does_not_decide_the_length() {
    let url = serve(
        vec![7; BODY],
        "HTTP/1.1 501 Not Implemented\r\nContent-Length: 482\r\nConnection: close\r\n\r\n",
        true,
    );
    let got = download(&runtime(), &url);
    expect_all(
        "BR-15",
        if got == Ok(BODY) {
            vec![]
        } else {
            vec![format!("{got:?}")]
        },
    );
}

#[test]
fn a_download_without_a_content_type_is_kept() {
    let url = serve(
        vec![7; BODY],
        "HTTP/1.1 200 OK\r\nContent-Length: 65536\r\nConnection: close\r\n\r\n",
        false,
    );
    let got = download(&runtime(), &url);
    expect_all(
        "BR-16",
        if got == Ok(BODY) {
            vec![]
        } else {
            vec![format!("{got:?}")]
        },
    );
}
