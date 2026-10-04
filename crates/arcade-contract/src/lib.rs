//! Stable, serialized contracts shared by the desktop app, CLI, and public SDK.
//! Rust's in-memory layout is deliberately not the plugin ABI.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

pub const TOOL_API_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    pub api_version: String,
    pub tools: Vec<ToolManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolManifest {
    pub id: String,
    pub version: String,
    pub api_version: String,
    pub name: String,
    pub description: String,
    pub category: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub privacy_class: PrivacyClass,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub providers: Vec<String>,
    pub status: ImplementationStatus,
    #[serde(default)]
    pub platforms: BTreeMap<String, String>,
    #[serde(default)]
    pub permissions: Value,
    #[serde(default)]
    pub execution: Value,
    #[serde(default)]
    pub phrases: Vec<String>,
    #[serde(default)]
    pub related_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<StandardUi>,
    /// Named option sets that other Arcade apps offer as one-click actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<ToolPreset>,
    /// How Arcade Link peers present this tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<ToolLink>,
}

/// A named option set, exposed to other Arcade apps as `box:<tool-id>#<id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolPreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub options: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolLink {
    /// Link content types for which peers show this tool's presets inline.
    #[serde(default)]
    pub featured_for: Vec<String>,
}

impl ToolManifest {
    pub fn preset(&self, id: &str) -> Option<&ToolPreset> {
        self.presets.iter().find(|preset| preset.id == id)
    }
}

/// The host-rendered form available equally to first-party and external tools.
/// Advanced views can be added beside this surface without changing tool I/O.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StandardUi {
    pub version: u32,
    pub input: UiInput,
    #[serde(default)]
    pub controls: Vec<UiControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiInput {
    pub kind: UiInputKind,
    pub label: String,
    #[serde(default)]
    pub min_items: Option<u32>,
    #[serde(default)]
    pub max_items: Option<u32>,
    #[serde(default)]
    pub sortable: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiInputKind {
    None,
    Text,
    Url,
    File,
    Files,
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiControl {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: UiControlKind,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub choices: Vec<UiChoice>,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub maximum: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub advanced: bool,
    #[serde(default)]
    pub show_when: Option<UiCondition>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiControlKind {
    Text,
    Password,
    Number,
    Select,
    Toggle,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiChoice {
    pub value: String,
    pub label: String,
    /// Provider capabilities this choice needs, such as `encoder:libx265`.
    /// Hosts disable the choice when no compatible provider offers all of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UiCondition {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub one_of: Vec<Value>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrivacyClass {
    Local,
    Network,
    Cloud,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationStatus {
    Planned,
    Partial,
    Implemented,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRequest {
    pub tool_id: String,
    pub inputs: Vec<ToolValue>,
    #[serde(default)]
    pub options: Value,
}

/// File values are references, not serialized file contents. The host must scope
/// a file reference to a user selection before granting a tool read access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolValue {
    pub kind: ValueKind,
    pub value: String,
    pub mime: String,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Text,
    File,
    Url,
    Artifact,
}

impl ToolValue {
    pub fn text(value: impl Into<String>, mime: impl Into<String>) -> Self {
        Self {
            kind: ValueKind::Text,
            value: value.into(),
            mime: mime.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub tool_id: String,
    pub status: ResultStatus,
    pub outputs: Vec<ToolValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSuggestion {
    pub tool_id: String,
    pub reason: String,
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("unsupported tool API version {0}")]
    ApiVersion(String),
    #[error("invalid tool ID {0}")]
    ToolId(String),
    #[error("invalid semantic version {version} for tool {tool}")]
    ToolVersion { tool: String, version: String },
    #[error("missing tool description or name for {0}")]
    MissingText(String),
    #[error("tool {0} has no output type")]
    MissingOutput(String),
    #[error("duplicate tool ID {0}")]
    DuplicateId(String),
    #[error("invalid standard UI for {tool}: {reason}")]
    InvalidUi { tool: String, reason: String },
}

impl Catalog {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.api_version != TOOL_API_VERSION {
            return Err(ContractError::ApiVersion(self.api_version.clone()));
        }
        let mut seen = std::collections::HashSet::new();
        for tool in &self.tools {
            tool.validate()?;
            if !seen.insert(tool.id.as_str()) {
                return Err(ContractError::DuplicateId(tool.id.clone()));
            }
        }
        Ok(())
    }
}

impl ToolManifest {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.api_version != TOOL_API_VERSION {
            return Err(ContractError::ApiVersion(self.api_version.clone()));
        }
        if !self.id.starts_with("arcade.")
            || self.id.split('.').count() < 3
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        {
            return Err(ContractError::ToolId(self.id.clone()));
        }
        if semver::Version::parse(&self.version).is_err() {
            return Err(ContractError::ToolVersion {
                tool: self.id.clone(),
                version: self.version.clone(),
            });
        }
        if self.name.trim().is_empty() || self.description.trim().is_empty() {
            return Err(ContractError::MissingText(self.id.clone()));
        }
        if self.outputs.is_empty() {
            return Err(ContractError::MissingOutput(self.id.clone()));
        }
        let mut preset_ids = std::collections::HashSet::new();
        for preset in &self.presets {
            if preset.id.is_empty()
                || !preset
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                || preset.name.trim().is_empty()
                || !preset_ids.insert(preset.id.as_str())
            {
                return Err(ContractError::InvalidUi {
                    tool: self.id.clone(),
                    reason: format!("invalid or duplicate preset {:?}", preset.id),
                });
            }
        }
        if let Some(ui) = &self.ui {
            ui.validate().map_err(|reason| ContractError::InvalidUi {
                tool: self.id.clone(),
                reason,
            })?;
            let declared = match ui.input.kind {
                UiInputKind::File | UiInputKind::Files => {
                    self.inputs.iter().any(|kind| kind.starts_with("file/"))
                }
                UiInputKind::Folder => {
                    self.inputs.iter().any(|kind| kind == "folder/reference")
                        && self
                            .permissions
                            .pointer("/filesystem/read")
                            .and_then(Value::as_str)
                            == Some("user-selected")
                }
                UiInputKind::Text => self.inputs.iter().any(|kind| {
                    kind.starts_with("text/")
                        || kind.starts_with("structured/")
                        || matches!(kind.as_str(), "network/host" | "network/ip" | "rows/csv")
                }),
                UiInputKind::Url => self
                    .inputs
                    .iter()
                    .any(|kind| kind == "text/url" || kind == "network/url"),
                UiInputKind::None => true,
            };
            if !declared {
                return Err(ContractError::InvalidUi {
                    tool: self.id.clone(),
                    reason: "input control is incompatible with declared input types".into(),
                });
            }
            for preset in &self.presets {
                for (key, value) in &preset.options {
                    let Some(control) = ui.controls.iter().find(|control| &control.key == key)
                    else {
                        return Err(ContractError::InvalidUi {
                            tool: self.id.clone(),
                            reason: format!("preset {} sets unknown option {key}", preset.id),
                        });
                    };
                    if control.kind == UiControlKind::Select
                        && !control
                            .choices
                            .iter()
                            .any(|choice| Some(choice.value.as_str()) == value.as_str())
                    {
                        return Err(ContractError::InvalidUi {
                            tool: self.id.clone(),
                            reason: format!("preset {} sets {key} to an unknown choice", preset.id),
                        });
                    }
                }
            }
            if ui
                .controls
                .iter()
                .any(|control| control.kind == UiControlKind::Directory)
                && self
                    .permissions
                    .pointer("/filesystem/write")
                    .and_then(Value::as_str)
                    != Some("user-selected")
            {
                return Err(ContractError::InvalidUi {
                    tool: self.id.clone(),
                    reason: "directory picker requires user-selected filesystem write permission"
                        .into(),
                });
            }
        }
        Ok(())
    }
}

impl StandardUi {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported form schema version".into());
        }
        if self.input.label.trim().is_empty() {
            return Err("input label is empty".into());
        }
        if self.input.sortable && self.input.kind != UiInputKind::Files {
            return Err("only multiple-file inputs can be sortable".into());
        }
        if self.input.max_items == Some(0) {
            return Err("input maximum must be positive".into());
        }
        if let (Some(min), Some(max)) = (self.input.min_items, self.input.max_items) {
            if min > max || max == 0 {
                return Err("invalid input item bounds".into());
            }
        }
        if self.controls.len() > 32 {
            return Err("more than 32 option controls".into());
        }
        let mut keys = std::collections::HashSet::new();
        for (index, control) in self.controls.iter().enumerate() {
            if control.key.is_empty()
                || !control
                    .key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                || control.label.trim().is_empty()
                || !keys.insert(control.key.as_str())
            {
                return Err("invalid or duplicate option key/label".into());
            }
            if let Some(condition) = &control.show_when {
                if !self.controls[..index]
                    .iter()
                    .any(|prior| prior.key == condition.key)
                {
                    return Err("visibility condition must refer to an earlier option".into());
                }
                if condition.equals.is_some() == !condition.one_of.is_empty() {
                    return Err("visibility condition needs equals or oneOf".into());
                }
            }
            match control.kind {
                UiControlKind::Select => {
                    if control.choices.is_empty()
                        || control
                            .choices
                            .iter()
                            .any(|choice| choice.value.is_empty() || choice.label.trim().is_empty())
                    {
                        return Err("select option needs nonempty choices".into());
                    }
                    if control
                        .choices
                        .iter()
                        .flat_map(|choice| &choice.requires)
                        .any(|capability| {
                            capability.split_once(':').is_none_or(|(kind, name)| {
                                kind.is_empty()
                                    || name.is_empty()
                                    || !kind
                                        .chars()
                                        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
                                    || !name.chars().all(|ch| {
                                        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')
                                    })
                            })
                        })
                    {
                        return Err("choice requirements must be capability identifiers".into());
                    }
                    let mut choices = std::collections::HashSet::new();
                    if control
                        .choices
                        .iter()
                        .any(|choice| !choices.insert(&choice.value))
                    {
                        return Err("select option has duplicate choices".into());
                    }
                    if let Some(default) = &control.default {
                        if !control
                            .choices
                            .iter()
                            .any(|choice| Some(choice.value.as_str()) == default.as_str())
                        {
                            return Err("select default is not a choice".into());
                        }
                    }
                }
                UiControlKind::Number => {
                    let default = control.default.as_ref().and_then(Value::as_f64);
                    if control.minimum.is_some_and(|value| !value.is_finite())
                        || control.maximum.is_some_and(|value| !value.is_finite())
                        || control
                            .step
                            .is_some_and(|value| !value.is_finite() || value <= 0.0)
                        || matches!((control.minimum, control.maximum), (Some(min), Some(max)) if min > max)
                        || control.default.is_some() && default.is_none()
                        || matches!((control.minimum, default), (Some(min), Some(value)) if value < min)
                        || matches!((control.maximum, default), (Some(max), Some(value)) if value > max)
                    {
                        return Err("invalid numeric option bounds/default".into());
                    }
                }
                UiControlKind::Text => {
                    if control
                        .default
                        .as_ref()
                        .is_some_and(|value| !value.is_string())
                    {
                        return Err("text default must be a string".into());
                    }
                }
                UiControlKind::Password => {
                    if control.default.is_some()
                        || !control.choices.is_empty()
                        || control.minimum.is_some()
                        || control.maximum.is_some()
                        || control.step.is_some()
                    {
                        return Err(
                            "password controls cannot have defaults, choices, or numeric bounds"
                                .into(),
                        );
                    }
                }
                UiControlKind::Toggle => {
                    if control
                        .default
                        .as_ref()
                        .is_some_and(|value| !value.is_boolean())
                    {
                        return Err("toggle default must be a boolean".into());
                    }
                }
                UiControlKind::Directory => {
                    if control
                        .default
                        .as_ref()
                        .is_some_and(|value| !value.is_null())
                    {
                        return Err("directory controls cannot have a default path or token".into());
                    }
                }
            }
            if control.kind != UiControlKind::Select && !control.choices.is_empty() {
                return Err("only select options have choices".into());
            }
            if control.kind != UiControlKind::Number
                && (control.minimum.is_some()
                    || control.maximum.is_some()
                    || control.step.is_some())
            {
                return Err("only numeric options have numeric bounds".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_id() {
        let tool = ToolManifest {
            id: "../../evil".into(),
            version: "1.0.0".into(),
            api_version: "1".into(),
            name: "Evil".into(),
            description: "Evil tool".into(),
            category: "test".into(),
            aliases: vec![],
            privacy_class: PrivacyClass::Local,
            inputs: vec![],
            outputs: vec!["text/plain".into()],
            providers: vec![],
            status: ImplementationStatus::Planned,
            platforms: BTreeMap::new(),
            permissions: Value::Null,
            execution: Value::Null,
            phrases: vec![],
            related_tools: vec![],
            ui: None,
            presets: vec![],
            link: None,
        };
        assert!(matches!(tool.validate(), Err(ContractError::ToolId(_))));
    }

    #[test]
    fn standard_form_rejects_unknown_visibility_and_unsafe_defaults() {
        let mut tool: ToolManifest = serde_json::from_value(serde_json::json!({
            "id": "arcade.test.form",
            "version": "1.0.0",
            "apiVersion": "1",
            "name": "Form test",
            "description": "Validate a standard tool form",
            "category": "test",
            "privacyClass": "LOCAL",
            "inputs": ["file/image"],
            "outputs": ["file/image"],
            "status": "implemented",
            "ui": {
                "version": 1,
                "input": {"kind": "file", "label": "Image"},
                "controls": [
                    {"key": "format", "label": "Format", "type": "select", "default": "png", "choices": [{"value": "png", "label": "PNG"}]},
                    {"key": "quality", "label": "Quality", "type": "number", "default": 80, "minimum": 1, "maximum": 100, "showWhen": {"key": "format", "equals": "png"}}
                ]
            }
        })).unwrap();
        assert!(tool.validate().is_ok());
        tool.ui.as_mut().unwrap().controls[1]
            .show_when
            .as_mut()
            .unwrap()
            .key = "missing".into();
        assert!(matches!(
            tool.validate(),
            Err(ContractError::InvalidUi { .. })
        ));
        tool.ui.as_mut().unwrap().controls[1]
            .show_when
            .as_mut()
            .unwrap()
            .key = "format".into();
        tool.ui.as_mut().unwrap().controls[1].default = Some(serde_json::json!(101));
        assert!(matches!(
            tool.validate(),
            Err(ContractError::InvalidUi { .. })
        ));
    }

    #[test]
    fn directory_picker_requires_user_selected_write_permission() {
        let mut tool: ToolManifest = serde_json::from_value(serde_json::json!({
            "id": "arcade.test.directory",
            "version": "1.0.0",
            "apiVersion": "1",
            "name": "Directory test",
            "description": "A user-selected output folder",
            "category": "test",
            "privacyClass": "LOCAL",
            "inputs": [],
            "outputs": ["file/any"],
            "permissions": {"filesystem": {"read": "none", "write": "none"}},
            "status": "implemented",
            "ui": {
                "version": 1,
                "input": {"kind": "none", "label": "No input"},
                "controls": [{"key": "destinationGrant", "label": "Output folder", "type": "directory"}]
            }
        })).unwrap();
        assert!(matches!(
            tool.validate(),
            Err(ContractError::InvalidUi { .. })
        ));
        tool.permissions["filesystem"]["write"] = serde_json::json!("user-selected");
        assert!(tool.validate().is_ok());
    }

    #[test]
    fn folder_picker_requires_scoped_user_selected_read_permission() {
        let mut tool: ToolManifest = serde_json::from_value(serde_json::json!({
            "id":"arcade.test.folder",
            "version":"1.0.0",
            "apiVersion":"1",
            "name":"Folder Tool",
            "description":"Reads one selected folder.",
            "category":"Test",
            "privacyClass":"LOCAL",
            "inputs":["folder/reference"],
            "outputs":["text/plain"],
            "status":"planned",
            "permissions":{"filesystem":{"read":"user-selected","write":"none"},"network":"none"},
            "ui":{"version":1,"input":{"kind":"folder","label":"Folder"}}
        }))
        .unwrap();
        assert!(tool.validate().is_ok());
        tool.permissions["filesystem"]["read"] = Value::String("none".into());
        assert!(tool.validate().is_err());
    }
}
