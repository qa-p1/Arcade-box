//! Opens a URL that the user explicitly reviewed, after strict scheme and
//! authority validation. Plugin UIs do not receive this capability.

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

const MAX_URL_BYTES: usize = 8 * 1024;

pub fn open_reviewed_url(app: AppHandle, input: String) -> Result<(), String> {
    let parsed = validate_reviewed_url(&input)?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|error| format!("Could not open this link: {error}"))
}

fn validate_reviewed_url(input: &str) -> Result<url::Url, String> {
    if input.len() > MAX_URL_BYTES || input.chars().any(char::is_control) {
        return Err("This link is too long or contains unsupported characters".into());
    }
    let trimmed = input.trim();
    let raw_authority = trimmed
        .split_once("://")
        .map(|(_, remainder)| remainder.split(['/', '?', '#']).next().unwrap_or_default())
        .unwrap_or_default();
    if raw_authority.contains('@') {
        return Err("Links containing embedded usernames or passwords are not opened".into());
    }
    let parsed = url::Url::parse(trimmed).map_err(|_| "This is not a valid web link".to_owned())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS links can be opened".into());
    }
    if parsed.host_str().is_none() {
        return Err("This link does not contain a valid host".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Links containing embedded usernames or passwords are not opened".into());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parser_preserves_only_safe_web_destinations() {
        for value in [
            "https://example.org/path?q=1",
            "http://127.0.0.1:8080/status",
            "https://[::1]/",
        ] {
            let parsed = validate_reviewed_url(value).unwrap();
            assert!(matches!(parsed.scheme(), "http" | "https"));
            assert!(parsed.host_str().is_some());
        }
        for value in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "mailto:user@example.org",
        ] {
            assert!(validate_reviewed_url(value).is_err());
        }
        for value in [
            "https://user@example.org/",
            "https://user:pass@example.org/",
            "https://@example.org/",
        ] {
            assert!(validate_reviewed_url(value).is_err());
        }
        assert!(validate_reviewed_url("https://example.org/\n/path").is_err());
        assert!(validate_reviewed_url(&"https://example.org/".repeat(MAX_URL_BYTES)).is_err());
    }
}
