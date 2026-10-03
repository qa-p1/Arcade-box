//! API keys for the few cloud-backed tools. Keys are read on demand from the
//! environment or a `.env` file and are never logged, returned in results, or
//! placed in a provider's argument vector.

use crate::provider::app_data_dir;
use std::{fs, path::PathBuf};

/// The Groq API key, from `GROQ_API_KEY` in the environment, then
/// `<app data>/.env`, then (debug builds only) the workspace `.env`.
pub(crate) fn groq_api_key() -> Option<String> {
    key_from_sources("GROQ_API_KEY")
}

/// Where a user should put their keys, for error messages.
pub(crate) fn env_file_hint() -> String {
    let mut places = Vec::new();
    if let Some(dir) = app_data_dir() {
        places.push(dir.join(".env").display().to_string());
    }
    if cfg!(debug_assertions) {
        places.push("the project's .env file".into());
    }
    places.push("the GROQ_API_KEY environment variable".into());
    places.join(", or ")
}

fn key_from_sources(name: &str) -> Option<String> {
    if let Some(value) = std::env::var(name).ok().and_then(clean) {
        return Some(value);
    }
    env_files().into_iter().find_map(|path| {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| value_in(&text, name))
    })
}

fn env_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(dir) = app_data_dir() {
        files.push(dir.join(".env"));
    }
    if cfg!(debug_assertions) {
        files.push(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.env"
        )));
    }
    files
}

/// `NAME=value` from dotenv text, ignoring comments, `export`, and quotes.
fn value_in(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (key, value) = line.split_once('=')?;
        (key.trim() == name).then(|| clean(value.trim().trim_matches(['"', '\'']).to_owned()))?
    })
}

/// A plausible key: non-empty, printable, and without whitespace, so it can
/// never break out of the HTTP header it is written into.
fn clean(value: impl Into<String>) -> Option<String> {
    let value = value.into().trim().to_owned();
    (!value.is_empty() && value.len() <= 512 && value.bytes().all(|byte| byte.is_ascii_graphic()))
        .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_values_are_parsed_and_validated() {
        let text = "# comment\nOTHER=1\nexport GROQ_API_KEY=\"gsk_abc123\"\n";
        assert_eq!(
            value_in(text, "GROQ_API_KEY").as_deref(),
            Some("gsk_abc123")
        );
        assert_eq!(value_in("GROQ_API_KEY=", "GROQ_API_KEY"), None);
        assert_eq!(value_in("GROQ_API_KEY=a b", "GROQ_API_KEY"), None);
        assert_eq!(
            value_in("GROQ_API_KEY=x\r\nInjected: 1", "GROQ_API_KEY").as_deref(),
            Some("x")
        );
    }
}
