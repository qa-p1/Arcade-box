//! The built-in tool catalog and how a request is prepared against a tool's manifest: input types it accepts,
//! the form's defaults for omitted options, and options sent as numbers.

use crate::*;

/// The built-in tool catalog, without opening any database (fast enough for
/// `arcade-box tools --json`, which other apps call).
pub fn builtin_catalog() -> Result<Catalog, CoreError> {
    load_catalog()
}

pub(crate) fn load_catalog() -> Result<Catalog, CoreError> {
    let catalog: Catalog = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../catalog/tools.json"
    )))?;
    catalog.validate()?;
    Ok(catalog)
}

pub(crate) fn input_compatible(actual: &str, declared: &str) -> bool {
    let declared = declared.strip_suffix("[]").unwrap_or(declared);
    actual == declared
        || declared == "file/any" && actual.starts_with("file/")
        || declared == "file/media" && matches!(actual, "file/video" | "file/audio")
}

/// B-23: the CLI, Link and pipelines get the same defaults as the form. An
/// option the caller leaves out takes its control's catalog default when that
/// control would be shown, exactly as the UI serializes a fresh form
/// (`serializeStandardUiOptions`), so "compress" means the same everywhere.
pub(crate) fn fill_ui_defaults(manifest: &ToolManifest, options: &mut serde_json::Value) {
    fn as_ui_string(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(text) => text.clone(),
            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => value.to_string(),
            _ => String::new(),
        }
    }
    let Some(ui) = manifest.ui.as_ref() else {
        return;
    };
    if options.is_null() {
        *options = serde_json::json!({});
    }
    let Some(map) = options.as_object_mut() else {
        return;
    };
    let current = |map: &serde_json::Map<String, serde_json::Value>, key: &str| {
        map.get(key).map(as_ui_string).or_else(|| {
            ui.controls
                .iter()
                .find(|control| control.key == key)
                .and_then(|control| control.default.as_ref().map(as_ui_string))
        })
    };
    let mut fills = Vec::new();
    for control in &ui.controls {
        let Some(default) = control.default.as_ref() else {
            continue;
        };
        if map.contains_key(&control.key) || as_ui_string(default).is_empty() {
            continue;
        }
        let visible = match &control.show_when {
            None => true,
            Some(condition) => {
                let value = current(map, &condition.key);
                match &condition.equals {
                    Some(expected) => value.as_deref() == Some(as_ui_string(expected).as_str()),
                    None => condition
                        .one_of
                        .iter()
                        .any(|expected| value.as_deref() == Some(as_ui_string(expected).as_str())),
                }
            }
        };
        if visible {
            fills.push((control.key.clone(), default.clone()));
        }
    }
    map.extend(fills);
}

/// A text, select or password option sent as a JSON number or boolean (`--option tabs=4` over Link, a script) is
/// the same value as its text; executors read these options as strings and would otherwise ignore it (U-1).
pub(crate) fn text_options_from_numbers(manifest: &ToolManifest, options: &mut serde_json::Value) {
    use arcade_contract::UiControlKind;
    let (Some(ui), Some(map)) = (manifest.ui.as_ref(), options.as_object_mut()) else {
        return;
    };
    for control in &ui.controls {
        if matches!(
            control.kind,
            UiControlKind::Text | UiControlKind::Select | UiControlKind::Password
        ) && let Some(value) = map.get_mut(&control.key)
            && (value.is_number() || value.is_boolean())
        {
            *value = serde_json::Value::String(value.to_string());
        }
    }
}
