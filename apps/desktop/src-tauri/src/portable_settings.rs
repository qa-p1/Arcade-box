//! Portable Box settings are an explicit allowlist. History, aliases,
//! provider selections, plugin state, credentials and device paths stay local.

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use arcade_core::{Arcade, pipeline::Pipeline};
use arcade_link::{Content, Handoff, LinkError, Locations};
use serde::{Deserialize, Serialize};

type StorageRows = (Vec<(String, u32, String)>, Vec<(String, String)>);

const APP_ID: &str = "arcade.box";
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_FAVORITES: usize = 2048;
const MAX_PIPELINES: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableSettings {
    pub schema: u32,
    pub app: String,
    pub version: String,
    pub favorites: Vec<String>,
    pub pipelines: Vec<Pipeline>,
    /// Deliberately limited to settings with no device identity or secrets.
    pub preferences: PortablePreferences,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortablePreferences {
    pub theme: String,
    pub onboarding_complete: bool,
    pub global_shortcut: String,
}

fn io_error(error: io::Error) -> LinkError {
    LinkError::unavailable(error.to_string())
}

fn setting(runtime: &Arcade, key: &str, fallback: &str) -> String {
    runtime
        .storage()
        .setting(key)
        .ok()
        .flatten()
        .unwrap_or_else(|| fallback.into())
}

fn canonical_shortcut(value: &str) -> Result<String, LinkError> {
    crate::shortcut_syntax::canonical_trigger(value)
        .map_err(|error| LinkError::denied(&format!("invalid global shortcut: {error}")))
}

fn strip_device_options(pipelines: &mut [Pipeline], runtime: &Arcade) {
    let tools = runtime.list_tools();
    for pipeline in pipelines {
        for node in &mut pipeline.nodes {
            let Some(options) = node.options.as_object_mut() else {
                continue;
            };
            let sensitive: Vec<_> = tools
                .iter()
                .find(|tool| tool.id == node.tool_id)
                .and_then(|tool| tool.ui.as_ref())
                .map(|ui| {
                    ui.controls
                        .iter()
                        .filter(|control| {
                            matches!(
                                control.kind,
                                arcade_contract::UiControlKind::Directory
                                    | arcade_contract::UiControlKind::Password
                            )
                        })
                        .map(|control| control.key.as_str())
                        .collect()
                })
                .unwrap_or_default();
            for key in sensitive {
                options.remove(key);
            }
        }
    }
}

impl PortableSettings {
    pub fn export(runtime: &Arcade) -> Result<Self, LinkError> {
        let mut pipelines = runtime
            .list_pipelines()
            .map_err(|e| LinkError::unavailable(e.to_string()))?;
        strip_device_options(&mut pipelines, runtime);
        let theme = setting(runtime, "theme", "system");
        let onboarding_complete = setting(runtime, "onboarding_complete", "false");
        let onboarding_complete = match onboarding_complete.as_str() {
            "true" => true,
            "false" => false,
            _ => {
                return Err(LinkError::unavailable(
                    "invalid saved onboarding preference",
                ));
            }
        };
        let shortcut = canonical_shortcut(&setting(
            runtime,
            "global_shortcut",
            crate::shortcut_syntax::DEFAULT,
        ))?;
        let settings = Self {
            schema: 1,
            app: APP_ID.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            favorites: runtime
                .storage()
                .favorites()
                .map_err(|e| LinkError::unavailable(e.to_string()))?,
            pipelines,
            preferences: PortablePreferences {
                theme,
                onboarding_complete,
                global_shortcut: shortcut,
            },
        };
        settings.validate_shape()?;
        Ok(settings)
    }

    fn validate_shape(&self) -> Result<(), LinkError> {
        if self.schema != 1
            || self.app != APP_ID
            || self.version.is_empty()
            || self.version.len() > 64
            || self.favorites.len() > MAX_FAVORITES
            || self.pipelines.len() > MAX_PIPELINES
            || !matches!(self.preferences.theme.as_str(), "system" | "light" | "dark")
        {
            return Err(LinkError::denied("invalid Arcade Box settings document"));
        }
        canonical_shortcut(&self.preferences.global_shortcut)?;
        for id in &self.favorites {
            if id.len() > 160
                || !id.starts_with("arcade.")
                || !id.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
                })
            {
                return Err(LinkError::denied("invalid favorite tool ID"));
            }
        }
        Ok(())
    }

    pub fn validate_for(&self, runtime: &Arcade) -> Result<(), LinkError> {
        self.validate_shape()?;
        let tools = runtime.list_tools();
        for favorite in &self.favorites {
            if !tools.iter().any(|tool| tool.id == *favorite) {
                return Err(LinkError::denied(&format!(
                    "unknown favorite tool ID: {favorite}"
                )));
            }
        }
        let mut ids = std::collections::HashSet::new();
        for pipeline in &self.pipelines {
            if !ids.insert(pipeline.id.as_str()) {
                return Err(LinkError::denied("duplicate pipeline ID"));
            }
            pipeline.validate(runtime).map_err(|error| {
                LinkError::denied(&format!("invalid pipeline {}: {error}", pipeline.id))
            })?;
        }
        let bytes = serde_json::to_vec(self).map_err(|e| LinkError::internal(e.to_string()))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(LinkError::denied("settings file exceeds 1 MiB"));
        }
        Ok(())
    }

    pub fn storage_rows(&self) -> Result<StorageRows, LinkError> {
        let pipelines = self
            .pipelines
            .iter()
            .map(|pipeline| {
                let definition = serde_json::to_string(pipeline)
                    .map_err(|e| LinkError::internal(e.to_string()))?;
                Ok((pipeline.id.clone(), pipeline.version, definition))
            })
            .collect::<Result<Vec<_>, LinkError>>()?;
        let settings = vec![
            ("theme".into(), self.preferences.theme.clone()),
            (
                "onboarding_complete".into(),
                self.preferences.onboarding_complete.to_string(),
            ),
            (
                "global_shortcut".into(),
                self.preferences.global_shortcut.clone(),
            ),
        ];
        Ok((pipelines, settings))
    }
}

pub fn export_file(runtime: &Arcade, locations: &Locations) -> Result<Vec<Content>, LinkError> {
    let settings = PortableSettings::export(runtime)?;
    let bytes =
        serde_json::to_vec_pretty(&settings).map_err(|e| LinkError::internal(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(LinkError::denied("settings file exceeds 1 MiB"));
    }
    let handoff = Handoff::create(locations, APP_ID).map_err(io_error)?;
    let mut file = handoff
        .file("arcade-box-settings.json", &bytes)
        .map_err(io_error)?;
    file.kind = "file/any".into();
    handoff.keep();
    Ok(vec![file])
}

pub fn read_import(inputs: &[Content], runtime: &Arcade) -> Result<PortableSettings, LinkError> {
    let [input] = inputs else {
        return Err(LinkError::denied("import requires one settings file"));
    };
    if input.kind != "file/any" || !input.paths.is_empty() {
        return Err(LinkError::denied("import requires a file/any path"));
    }
    let path = input
        .path
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| LinkError::denied("import requires an absolute path"))?;
    let file = fs::File::open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(LinkError::denied("import requires a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(LinkError::denied("settings file exceeds 1 MiB"));
    }
    let mut settings: PortableSettings = serde_json::from_slice(&bytes)
        .map_err(|e| LinkError::denied(&format!("invalid settings: {e}")))?;
    strip_device_options(&mut settings.pipelines, runtime);
    settings.validate_for(runtime)?;
    Ok(settings)
}

/// A private recovery copy of the allowlisted data, kept if a later manifest
/// update fails after the database transaction has committed.
pub fn write_backup(settings: &PortableSettings, data_dir: &Path) -> Result<PathBuf, LinkError> {
    let token = arcade_link::endpoint::new_token().map_err(io_error)?;
    let directory = data_dir.join("portable-import-backups");
    arcade_link::paths::ensure_private_dir(&directory).map_err(io_error)?;
    let path = directory.join(format!("{token}.json"));
    let bytes =
        serde_json::to_vec_pretty(settings).map_err(|e| LinkError::internal(e.to_string()))?;
    arcade_link::paths::write_atomic(&path, &bytes, true).map_err(io_error)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_document_is_an_allowlist_without_history_or_device_state() {
        let settings = PortableSettings {
            schema: 1,
            app: APP_ID.into(),
            version: "0.1.2".into(),
            favorites: vec!["arcade.image.resize".into()],
            pipelines: vec![],
            preferences: PortablePreferences {
                theme: "dark".into(),
                onboarding_complete: true,
                global_shortcut: "CTRL+ALT+SPACE".into(),
            },
        };
        settings.validate_shape().unwrap();
        let json = serde_json::to_string(&settings).unwrap();
        for forbidden in [
            "history",
            "credential",
            "secret",
            "deviceIdentity",
            "providerChoices",
        ] {
            assert!(!json.contains(forbidden));
        }
    }

    #[test]
    fn rejects_unknown_fields_and_noncanonical_shortcuts_are_normalized() {
        let json = r#"{"schema":1,"app":"arcade.box","version":"0.1.2","favorites":[],"pipelines":[],"preferences":{"theme":"system","onboardingComplete":false,"globalShortcut":"CTRL+ALT+SPACE"},"history":[]}"#;
        assert!(serde_json::from_str::<PortableSettings>(json).is_err());
        assert_eq!(
            canonical_shortcut("CTRL+ALT+SPACE").unwrap(),
            "Ctrl+Alt+Space"
        );
    }
}
