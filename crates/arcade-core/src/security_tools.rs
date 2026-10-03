//! Password generation/strength and file or text encryption. Secret values are
//! consumed only for the current invocation; this module never adds them to
//! results, logs, or persistent state.

use crate::{
    Arcade,
    artifacts::validate_portable_filename,
    tool_kit::{check_cancelled, number_in, option_bool, option_str, success},
};
use age::{
    Decryptor, Encryptor,
    armor::{ArmoredReader, ArmoredWriter, Format},
    secrecy::SecretString,
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use rand::Rng;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{BufReader, Cursor, Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
};

const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const COPY_BUFFER: usize = 1024 * 1024;

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    match manifest.id.as_str() {
        "arcade.security.password" => password_tool(manifest, request),
        "arcade.security.encrypt" => age_encrypt_decrypt(manifest, request, runtime, cancelled),
        _ => Err(format!(
            "No security executor is registered for {}",
            manifest.id
        )),
    }
}

/// Generate a password, passphrase, or PIN, or check how strong a password
/// is. A checked password is read for this call only and never returned.
fn password_tool(manifest: &ToolManifest, request: &ToolRequest) -> Result<ToolResult, String> {
    let mode = option_str(request, "mode", "password");
    if mode == "check" {
        let password = secret_option(request, "password")?;
        let report = strength_report(password, None);
        let summary = format!(
            "Strength: {}",
            report["rating"].as_str().unwrap_or("unknown")
        );
        return Ok(success(
            manifest,
            vec![ToolValue::text(
                report.to_string(),
                "structured/password-strength",
            )],
            Some(summary),
            vec![],
        ));
    }
    let mut rng = rand::rng();
    let (secret, entropy_bits) = match mode {
        "password" => {
            let length = number_in(
                request,
                "length",
                "Password length",
                Some(20.0),
                8.0..=128.0,
            )? as usize;
            let mut alphabet =
                String::from("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");
            if option_bool(request, "symbols", true) {
                alphabet.push_str("!#$%&()*+,-./:;<=>?@[]^_{|}~");
            }
            if option_bool(request, "excludeAmbiguous", true) {
                alphabet.retain(|character| !"0O1Il|`'\"".contains(character));
            }
            let alphabet = alphabet.chars().collect::<Vec<_>>();
            let value = (0..length)
                .map(|_| alphabet[rng.random_range(0..alphabet.len())])
                .collect::<String>();
            (value, length as f64 * (alphabet.len() as f64).log2())
        }
        "passphrase" => {
            let words =
                number_in(request, "words", "Number of words", Some(5.0), 3.0..=12.0)? as usize;
            // Pronounceable pseudo-words: 16 × 16 × 16 = 4096 choices (12 bits) each.
            const ONSETS: [&str; 16] = [
                "b", "br", "d", "dr", "f", "g", "gr", "k", "kr", "l", "m", "n", "p", "pr", "s",
                "st",
            ];
            const VOWELS: [&str; 16] = [
                "a", "e", "i", "o", "u", "ae", "ai", "au", "ea", "ee", "ie", "oa", "oo", "ou",
                "ui", "y",
            ];
            const CODAS: [&str; 16] = [
                "b", "d", "f", "g", "k", "l", "m", "n", "p", "r", "s", "t", "v", "x", "z", "th",
            ];
            let value = (0..words)
                .map(|_| {
                    format!(
                        "{}{}{}",
                        ONSETS[rng.random_range(0..16)],
                        VOWELS[rng.random_range(0..16)],
                        CODAS[rng.random_range(0..16)]
                    )
                })
                .collect::<Vec<_>>()
                .join(option_str(request, "separator", "-"));
            (value, words as f64 * 12.0)
        }
        "pin" => {
            let digits =
                number_in(request, "digits", "PIN length", Some(6.0), 4.0..=12.0)? as usize;
            let value = (0..digits)
                .map(|_| char::from(b'0' + rng.random_range(0..10u8)))
                .collect::<String>();
            (value, digits as f64 * 10f64.log2())
        }
        other => return Err(format!("Unknown mode: {other}")),
    };
    let report = strength_report(&secret, Some(entropy_bits));
    Ok(success(
        manifest,
        vec![
            ToolValue::text(secret, "text/plain"),
            ToolValue::text(report.to_string(), "structured/password-strength"),
        ],
        Some("Generated on this device with the system's secure random source".into()),
        vec![],
    ))
}

fn strength_report(password: &str, entropy_bits: Option<f64>) -> Value {
    let estimate = zxcvbn::zxcvbn(password, &[]);
    let score = u8::from(estimate.score());
    let feedback = estimate.feedback();
    let rating = ["very weak", "weak", "fair", "strong", "very strong"][usize::from(score.min(4))];
    json!({
        "score": score,
        "rating": rating,
        "length": password.chars().count(),
        "estimatedEntropyBits": entropy_bits.map(f64::round),
        "guessesLog10": (estimate.guesses_log10() * 10.0).round() / 10.0,
        "crackTimeOnline": estimate.crack_times().online_throttling_100_per_hour().to_string(),
        "crackTimeOffline": estimate.crack_times().offline_slow_hashing_1e4_per_second().to_string(),
        "warning": feedback.and_then(|feedback| feedback.warning()).map(|warning| warning.to_string()),
        "suggestions": feedback
            .map(|feedback| feedback.suggestions().iter().map(ToString::to_string).collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}

fn age_encrypt_decrypt(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let action = option_str(request, "action", "encrypt");
    let mode = option_str(request, "mode", "passphrase");
    if !matches!(mode, "passphrase" | "recipient") {
        return Err("Choose passphrase or age recipient-key mode".into());
    }
    let encrypting = action == "encrypt";
    if !encrypting && action != "decrypt" {
        return Err("Choose encrypt or decrypt".into());
    }
    let password = if mode == "passphrase" {
        let password = request
            .options
            .get("password")
            .and_then(Value::as_str)
            .filter(|password| !password.is_empty())
            .ok_or("Enter a passphrase in the masked password field")?;
        if password.len() > 4096 {
            return Err("Passphrase exceeds the 4 KiB limit".into());
        }
        Some(password)
    } else {
        None
    };
    let recipient = if mode == "recipient" && encrypting {
        let value = request
            .options
            .get("recipient")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or("Enter an age recipient key")?;
        if value.len() > 4096 {
            return Err("Age recipient key exceeds the 4 KiB limit".into());
        }
        Some(
            value
                .trim()
                .parse::<age::x25519::Recipient>()
                .map_err(|_| {
                    "Age recipient key is invalid; use an age1... X25519 public key".to_string()
                })?,
        )
    } else {
        None
    };
    let identity = if mode == "recipient" && !encrypting {
        let value = request
            .options
            .get("identity")
            .and_then(Value::as_str)
            .or_else(|| request.options.get("recipient").and_then(Value::as_str))
            .filter(|value| !value.trim().is_empty())
            .ok_or("Enter an age private identity")?;
        if value.len() > 4096 {
            return Err("Age private identity exceeds the 4 KiB limit".into());
        }
        Some(
            value
                .trim()
                .parse::<age::x25519::Identity>()
                .map_err(|_| "Age private identity is invalid".to_string())?,
        )
    } else {
        None
    };
    let Some(input) = request.inputs.first() else {
        return Err("Choose a file or provide text through a pipeline".into());
    };
    if request.inputs.len() != 1 {
        return Err("Choose one file or text input".into());
    }
    let source_name = if input.kind == ValueKind::Artifact || input.kind == ValueKind::File {
        runtime
            .grants()
            .resolve(&input.value)
            .map_err(|error| error.to_string())?
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input")
            .to_owned()
    } else {
        "text.txt".into()
    };
    let stage = tempfile::tempdir_in(runtime.artifact_staging_root())
        .map_err(|error| format!("Create private encryption workspace: {error}"))?;
    let default_name = if encrypting {
        format!("{source_name}.age")
    } else {
        let path = Path::new(&source_name);
        let file_name = path
            .file_name()
            .and_then(|part| part.to_str())
            .unwrap_or("decrypted");
        if file_name.to_ascii_lowercase().ends_with(".age") {
            let original_name = &file_name[..file_name.len() - 4];
            if original_name.is_empty() {
                "decrypted".to_owned()
            } else {
                original_name.to_owned()
            }
        } else {
            let stem = path
                .file_stem()
                .and_then(|part| part.to_str())
                .unwrap_or("decrypted");
            match path.extension().and_then(|part| part.to_str()) {
                Some(extension) => format!("{stem}-decrypted.{extension}"),
                None => format!("{stem}-decrypted"),
            }
        }
    };
    let output_name = request
        .options
        .get("outputName")
        .and_then(Value::as_str)
        .unwrap_or(&default_name);
    validate_portable_filename(output_name)?;
    let output_path = stage.path().join(output_name);
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .map_err(|error| format!("Create private output: {error}"))?;
    let source = open_or_cursor(input, runtime, cancelled)?;
    let mut source = BufReader::new(source);
    let mut output = output;
    if encrypting {
        let encryptor = if let Some(password) = password {
            Encryptor::with_user_passphrase(SecretString::from(password.to_owned()))
        } else {
            let recipient = recipient.as_ref().ok_or("Age recipient key is missing")?;
            Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
                .map_err(|_| "Could not initialize age recipient encryption".to_string())?
        };
        let armored = ArmoredWriter::wrap_output(&mut output, Format::AsciiArmor)
            .map_err(|error| format!("Initialize age armor: {error}"))?;
        let mut writer = encryptor
            .wrap_output(armored)
            .map_err(|error| format!("Initialize age encryption: {error}"))?;
        let mut buffer = vec![0; COPY_BUFFER];
        loop {
            check_cancelled(cancelled)?;
            let count = source
                .read(&mut buffer)
                .map_err(|error| format!("Read input: {error}"))?;
            if count == 0 {
                break;
            }
            writer
                .write_all(&buffer[..count])
                .map_err(|error| format!("Encrypt input: {error}"))?;
        }
        let armored = writer
            .finish()
            .map_err(|error| format!("Finish age encryption: {error}"))?;
        let _ = armored
            .finish()
            .map_err(|error| format!("Finish age armor: {error}"))?;
    } else {
        let decryptor = Decryptor::new(ArmoredReader::new(source))
            .map_err(|_| "Input is not a supported age-encrypted file".to_string())?;
        let mut reader = if let Some(password) = password {
            let identity = age::scrypt::Identity::new(SecretString::from(password.to_owned()));
            decryptor
                .decrypt(std::iter::once(&identity as &dyn age::Identity))
                .map_err(|_| {
                    "Could not decrypt this age file with the supplied passphrase".to_string()
                })?
        } else {
            let identity = identity.as_ref().ok_or("Age private identity is missing")?;
            decryptor
                .decrypt(std::iter::once(identity as &dyn age::Identity))
                .map_err(|_| {
                    "Could not decrypt this age file with the supplied identity".to_string()
                })?
        };
        let mut buffer = vec![0; COPY_BUFFER];
        loop {
            check_cancelled(cancelled)?;
            let count = reader.read(&mut buffer).map_err(|_| {
                "Age decryption failed; no complete output was published".to_string()
            })?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| format!("Write decrypted output: {error}"))?;
        }
    }
    output
        .flush()
        .map_err(|error| format!("Flush encrypted output: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("Sync encrypted output: {error}"))?;
    drop(output);
    let artifact = runtime
        .publish_staged_output(
            request
                .options
                .get("destinationGrant")
                .and_then(Value::as_str),
            &output_path,
            output_name,
            cancelled,
        )
        .map_err(|error| error.to_string())?;
    let mut result = success(
        manifest,
        vec![artifact.as_tool_value()],
        Some(match (encrypting, mode) {
            (true, "passphrase") => "Encrypted locally using the age passphrase format".into(),
            (false, "passphrase") => "Decrypted locally using the age passphrase format".into(),
            (true, _) => "Encrypted locally to the age recipient key".into(),
            (false, _) => "Decrypted locally using the age private identity".into(),
        }),
        vec![],
    );
    if encrypting && let Some(password) = password {
        if password.chars().count() < 12 {
            result.warnings.push(
                "A short passphrase may be easier to guess; use a long, unique passphrase.".into(),
            );
        }
    }
    Ok(result)
}

fn open_or_cursor<'a>(
    input: &'a ToolValue,
    runtime: &'a Arcade,
    cancelled: &'a AtomicBool,
) -> Result<Box<dyn Read + 'a>, String> {
    check_cancelled(cancelled)?;
    if input.kind == ValueKind::Artifact || input.kind == ValueKind::File {
        return runtime
            .grants()
            .open_scoped(&input.value)
            .map(|file| Box::new(file) as Box<dyn Read>)
            .map_err(|error| error.to_string());
    }
    if input.kind == ValueKind::Text || input.kind == ValueKind::Url {
        if input.value.len() > MAX_TEXT_BYTES {
            return Err("Text input exceeds 16 MiB".into());
        }
        return Ok(Box::new(Cursor::new(input.value.as_bytes().to_vec())));
    }
    Err("Choose a user-selected file or text input".into())
}

fn secret_option<'a>(request: &'a ToolRequest, key: &str) -> Result<&'a str, String> {
    request
        .options
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Enter {key} in its masked field"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_contract::ResultStatus;

    #[test]
    fn pseudo_word_password_has_configured_number_of_tokens() {
        let manifest = Arcade::in_memory()
            .unwrap()
            .list_tools()
            .into_iter()
            .find(|tool| tool.id == "arcade.security.password")
            .unwrap();
        let request = ToolRequest {
            tool_id: manifest.id.clone(),
            inputs: vec![],
            options: json!({"mode":"passphrase","words":5}),
        };
        let result = password_tool(&manifest, &request).unwrap();
        let text = &result.outputs[0].value;
        assert_eq!(text.split('-').count(), 5);
        assert!(
            text.chars()
                .all(|character| character.is_ascii_lowercase() || character == '-')
        );
    }

    #[test]
    fn age_file_encrypt_decrypt_round_trip_runs_through_registry() {
        let runtime = Arcade::in_memory().unwrap();
        let input_dir = tempfile::tempdir().unwrap();
        let input_path = input_dir.path().join("notes.txt");
        std::fs::write(&input_path, b"private document content\n").unwrap();
        let selected = runtime.grants().grant(&input_path).unwrap();
        let password = "a long unique passphrase for local test";

        let encrypted = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.security.encrypt".into(),
                inputs: vec![selected.as_tool_value()],
                options: json!({"action":"encrypt","password":password}),
            })
            .unwrap();
        assert_eq!(
            encrypted.status,
            ResultStatus::Success,
            "{:?}",
            encrypted.message
        );
        let encrypted_value = encrypted.outputs[0].clone();
        let encrypted_name = runtime
            .grants()
            .resolve(&encrypted_value.value)
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(encrypted_name.ends_with(".age"));

        let decrypted = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.security.encrypt".into(),
                inputs: vec![encrypted_value],
                options: json!({"action":"decrypt","password":password}),
            })
            .unwrap();
        assert_eq!(
            decrypted.status,
            ResultStatus::Success,
            "{:?}",
            decrypted.message
        );
        let restored = runtime
            .grants()
            .open_scoped(&decrypted.outputs[0].value)
            .unwrap();
        assert_eq!(
            restored.metadata().unwrap().len(),
            b"private document content\n".len() as u64
        );
        assert_eq!(
            std::fs::read(
                runtime
                    .grants()
                    .resolve(&decrypted.outputs[0].value)
                    .unwrap()
            )
            .unwrap(),
            b"private document content\n"
        );
    }

    #[test]
    fn age_recipient_encrypt_decrypt_round_trip_runs_through_registry() {
        use age::secrecy::ExposeSecret;

        let runtime = Arcade::in_memory().unwrap();
        let input_dir = tempfile::tempdir().unwrap();
        let input_path = input_dir.path().join("notes.txt");
        let plaintext = b"recipient encrypted document content\n";
        std::fs::write(&input_path, plaintext).unwrap();
        let selected = runtime.grants().grant(&input_path).unwrap();
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        let identity = identity.to_string().expose_secret().to_owned();

        let encrypted = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.security.encrypt".into(),
                inputs: vec![selected.as_tool_value()],
                options: json!({
                    "action":"encrypt",
                    "mode":"recipient",
                    "recipient":recipient
                }),
            })
            .unwrap();
        assert_eq!(
            encrypted.status,
            ResultStatus::Success,
            "{:?}",
            encrypted.message
        );

        let decrypted = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.security.encrypt".into(),
                inputs: vec![encrypted.outputs[0].clone()],
                options: json!({
                    "action":"decrypt",
                    "mode":"recipient",
                    "recipient":identity
                }),
            })
            .unwrap();
        assert_eq!(
            decrypted.status,
            ResultStatus::Success,
            "{:?}",
            decrypted.message
        );
        let restored_path = runtime
            .grants()
            .resolve(&decrypted.outputs[0].value)
            .unwrap();
        assert_eq!(std::fs::read(restored_path).unwrap(), plaintext);
    }
}
