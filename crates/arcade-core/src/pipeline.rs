use crate::{Arcade, CoreError};
use arcade_contract::{ResultStatus, ToolRequest, ToolValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};
use thiserror::Error;

/// A versioned DAG. Stage outputs are references; file content never enters the
/// pipeline definition or frontend IPC as a large serialized buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub version: u32,
    pub nodes: Vec<PipelineNode>,
    pub output_nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineNode {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkNode>,
    pub inputs: Vec<InputSource>,
    #[serde(default)]
    pub options: Value,
}

/// Peer identity is pinned to the action version that the user saved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkNode {
    pub app: String,
    pub action: String,
    pub version: u32,
}

#[derive(Debug, Clone)]
pub struct StageInfo {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub effects: Vec<String>,
    pub interactive: bool,
    pub required_inputs: usize,
}

impl PipelineNode {
    pub fn describe(&self, runtime: &Arcade) -> Result<StageInfo, PipelineError> {
        if let Some(link) = &self.link {
            if !self.tool_id.is_empty() || link.app == arcade_link::ids::BOX || link.version == 0 {
                return Err(PipelineError::StageFailed(
                    self.id.clone(),
                    "invalid Link node identity".into(),
                ));
            }
            let (_, action) = crate::link::consumer::peer_action(runtime, &link.app, &link.action)
                .ok_or_else(|| {
                    PipelineError::StageFailed(
                        self.id.clone(),
                        format!(
                            "{} is not installed, enabled, or available",
                            arcade_link::manifest::app_name(&link.app)
                        ),
                    )
                })?;
            if action.version != link.version {
                return Err(PipelineError::StageFailed(
                    self.id.clone(),
                    format!(
                        "Needs repair: {} action version changed from {} to {}",
                        link.action, link.version, action.version
                    ),
                ));
            }
            return Ok(StageInfo {
                required_inputs: usize::from(!action.accepts.is_empty()),
                inputs: action.accepts,
                outputs: action.produces,
                effects: action.effects,
                interactive: action.interactive,
            });
        }
        let tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == self.tool_id)
            .ok_or_else(|| PipelineError::UnknownTool(self.tool_id.clone()))?;
        if tool.id.starts_with("arcade.pipeline.") {
            return Err(PipelineError::StageFailed(
                self.id.clone(),
                "nested saved pipelines are not supported".into(),
            ));
        }
        let required_inputs = tool
            .ui
            .as_ref()
            .map(|ui| {
                ui.input.min_items.unwrap_or(match ui.input.kind {
                    arcade_contract::UiInputKind::None | arcade_contract::UiInputKind::Folder => 0,
                    _ => 1,
                }) as usize
            })
            .unwrap_or_else(|| usize::from(!tool.inputs.is_empty()));
        Ok(StageInfo {
            inputs: tool.inputs.clone(),
            outputs: tool.outputs.clone(),
            effects: crate::link::effects(&tool),
            interactive: false,
            required_inputs,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputSource {
    External {
        index: usize,
    },
    Node {
        #[serde(rename = "nodeId")]
        node_id: String,
        #[serde(rename = "outputIndex")]
        output_index: usize,
    },
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("duplicate stage ID {0}")]
    DuplicateStage(String),
    #[error("pipeline must expose at least one output stage")]
    NoOutputs,
    #[error("unknown stage {0}")]
    UnknownStage(String),
    #[error("unknown tool {0}")]
    UnknownTool(String),
    #[error("stage {0} requires at least one tool input")]
    MissingInputs(String),
    #[error("stage {stage} requires at least {required} tool inputs")]
    TooFewInputs { stage: String, required: usize },
    #[error("cycle in pipeline")]
    Cycle,
    #[error("incompatible connection from {from} ({output_type}) to {to} ({input_types:?})")]
    TypeMismatch {
        from: String,
        to: String,
        output_type: String,
        input_types: Vec<String>,
    },
    #[error("pipeline cancelled")]
    Cancelled,
    #[error("stage {0}: {1}")]
    Execution(String, #[source] CoreError),
    #[error("stage {0} produced no output")]
    MissingOutput(String),
    #[error("pipeline external input {0} is missing")]
    MissingExternal(usize),
    #[error(
        "stage {stage} uses sensitive option {key}; password controls cannot be saved in pipelines"
    )]
    SensitiveOption { stage: String, key: String },
    #[error("stage {0} failed: {1}")]
    StageFailed(String, String),
    #[error("interactive stage {0} must be the first stage")]
    InteractivePosition(String),
    #[error("Confirm pipeline effects before running: {0}")]
    NeedsConfirmation(String),
}

impl Pipeline {
    pub fn validate<'a>(
        &'a self,
        runtime: &Arcade,
    ) -> Result<Vec<&'a PipelineNode>, PipelineError> {
        if self.output_nodes.is_empty() {
            return Err(PipelineError::NoOutputs);
        }
        let tools = runtime.list_tools();
        let mut by_id = HashMap::new();
        for node in &self.nodes {
            if by_id.insert(node.id.as_str(), node).is_some() {
                return Err(PipelineError::DuplicateStage(node.id.clone()));
            }
            let description = node.describe(runtime)?;
            let required_inputs = description.required_inputs;
            if node.inputs.len() < required_inputs {
                return Err(if required_inputs == 1 {
                    PipelineError::MissingInputs(node.id.clone())
                } else {
                    PipelineError::TooFewInputs {
                        stage: node.id.clone(),
                        required: required_inputs,
                    }
                });
            }
            if let Some(controls) = tools
                .iter()
                .find(|t| t.id == node.tool_id)
                .and_then(|tool| tool.ui.as_ref().map(|ui| &ui.controls))
            {
                if let Some(key) = controls
                    .iter()
                    .filter(|control| control.kind == arcade_contract::UiControlKind::Password)
                    .map(|control| control.key.as_str())
                    .find(|key| node.options.get(*key).is_some())
                {
                    return Err(PipelineError::SensitiveOption {
                        stage: node.id.clone(),
                        key: key.to_owned(),
                    });
                }
            }
        }
        for id in &self.output_nodes {
            if !by_id.contains_key(id.as_str()) {
                return Err(PipelineError::UnknownStage(id.clone()));
            }
        }
        let mut ordered = Vec::new();
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for node in &self.nodes {
            visit(node, &by_id, &mut visiting, &mut visited, &mut ordered)?;
        }
        for (position, node) in ordered.iter().enumerate() {
            if node.describe(runtime)?.interactive
                && (position != 0 || self.nodes.first().map(|n| &n.id) != Some(&node.id))
            {
                return Err(PipelineError::InteractivePosition(node.id.clone()));
            }
            for input in &node.inputs {
                if let InputSource::Node {
                    node_id,
                    output_index,
                } = input
                {
                    let source = by_id
                        .get(node_id.as_str())
                        .ok_or_else(|| PipelineError::UnknownStage(node_id.clone()))?;
                    let from = source.describe(runtime)?;
                    let to = node.describe(runtime)?;
                    let output_type = from
                        .outputs
                        .get(*output_index)
                        .ok_or_else(|| PipelineError::MissingOutput(source.id.clone()))?;
                    if !to.inputs.iter().any(|input_type| {
                        connection_compatible(
                            output_type,
                            input_type,
                            source.link.is_some() || node.link.is_some(),
                        )
                    }) {
                        return Err(PipelineError::TypeMismatch {
                            from: source.id.clone(),
                            to: node.id.clone(),
                            output_type: output_type.clone(),
                            input_types: to.inputs.clone(),
                        });
                    }
                }
            }
        }
        Ok(ordered)
    }

    pub fn effects(&self, runtime: &Arcade) -> Result<Vec<String>, PipelineError> {
        let mut effects = std::collections::BTreeSet::new();
        for node in self.validate(runtime)? {
            effects.extend(node.describe(runtime)?.effects);
        }
        Ok(effects.into_iter().collect())
    }

    fn confirm_effects(&self, runtime: &Arcade) -> Result<(), PipelineError> {
        use sha2::{Digest, Sha256};
        let effects = self.effects(runtime)?;
        if !effects.iter().any(|e| {
            matches!(
                e.as_str(),
                "sends-to-device" | "network" | "executes-commands"
            )
        }) {
            return Ok(());
        }
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(self, &effects))
                    .map_err(|e| PipelineError::StageFailed(self.id.clone(), e.to_string()))?
            )
        );
        let key = format!("pipeline_effect_approval.{}", self.id);
        if runtime.storage().setting(&key).ok().flatten().as_deref() == Some(&fingerprint) {
            return Ok(());
        }
        if !runtime
            .pipeline_confirmation
            .get()
            .is_some_and(|confirm| confirm(self, &effects))
        {
            return Err(PipelineError::NeedsConfirmation(effects.join(", ")));
        }
        runtime
            .storage()
            .set_setting(&key, &fingerprint)
            .map_err(|e| PipelineError::StageFailed(self.id.clone(), e.to_string()))?;
        Ok(())
    }

    pub fn run(
        &self,
        runtime: &Arcade,
        external: Vec<ToolValue>,
        cancelled: &AtomicBool,
    ) -> Result<HashMap<String, Vec<ToolValue>>, PipelineError> {
        let order = self.validate(runtime)?;
        // Check every external input and local engine before opening any peer UI.
        for node in &order {
            let info = node.describe(runtime)?;
            for source in &node.inputs {
                if let InputSource::External { index } = source {
                    let input = external
                        .get(*index)
                        .ok_or(PipelineError::MissingExternal(*index))?;
                    if !info.inputs.iter().any(|accepted| {
                        connection_compatible(&input.mime, accepted, node.link.is_some())
                    }) {
                        return Err(PipelineError::TypeMismatch {
                            from: "external input".into(),
                            to: node.id.clone(),
                            output_type: input.mime.clone(),
                            input_types: info.inputs,
                        });
                    }
                }
            }
            if node.link.is_none() {
                let tool = runtime
                    .list_tools()
                    .into_iter()
                    .find(|t| t.id == node.tool_id)
                    .unwrap();
                if tool.status != arcade_contract::ImplementationStatus::Implemented {
                    return Err(PipelineError::StageFailed(
                        node.id.clone(),
                        "tool is unavailable".into(),
                    ));
                }
                let cache = crate::link::load_provider_cache(runtime)
                    .unwrap_or_else(|| crate::link::probe_for(runtime, &tool));
                if let Some(reason) = crate::link::missing_provider(&tool, Some(&cache)) {
                    return Err(PipelineError::StageFailed(node.id.clone(), reason));
                }
            }
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err(PipelineError::Cancelled);
        }
        self.confirm_effects(runtime)?;
        let mut workspace =
            Workspace::new(runtime).map_err(|e| PipelineError::StageFailed(self.id.clone(), e))?;
        let external = external
            .iter()
            .map(|v| workspace.keep(v, cancelled, false))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| PipelineError::StageFailed(self.id.clone(), e))?;
        let mut results: HashMap<String, Vec<ToolValue>> = HashMap::new();
        for node in order {
            if cancelled.load(Ordering::Relaxed) {
                return Err(PipelineError::Cancelled);
            }
            let inputs = node
                .inputs
                .iter()
                .map(|source| match source {
                    InputSource::External { index } => external
                        .get(*index)
                        .cloned()
                        .ok_or(PipelineError::MissingExternal(*index)),
                    InputSource::Node {
                        node_id,
                        output_index,
                    } => results
                        .get(node_id)
                        .and_then(|v| v.get(*output_index))
                        .cloned()
                        .ok_or_else(|| PipelineError::MissingOutput(node_id.clone())),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let outputs = if let Some(link) = &node.link {
                let mut request =
                    arcade_link::InvokeRequest::new(&link.action, arcade_link::ids::BOX)
                        .options(node.options.clone());
                request.version = Some(link.version);
                let handoff = arcade_link::Handoff::create(
                    &arcade_link::Locations::discover(),
                    arcade_link::ids::BOX,
                )
                .map_err(|e| PipelineError::StageFailed(node.id.clone(), e.to_string()))?;
                for input in &inputs {
                    let mut content = crate::link::consumer::output_content(runtime, input)
                        .map_err(|e| PipelineError::StageFailed(node.id.clone(), e.to_string()))?;
                    if !content.all_paths().is_empty() {
                        let path = runtime.grants().resolve(&input.value).map_err(|e| {
                            PipelineError::StageFailed(node.id.clone(), e.to_string())
                        })?;
                        let destination = handoff.dir().join(format!(
                            "{}-{}",
                            uuid::Uuid::new_v4(),
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ));
                        copy_file(
                            runtime.grants().open_scoped(&input.value).map_err(|e| {
                                PipelineError::StageFailed(node.id.clone(), e.to_string())
                            })?,
                            &destination,
                            cancelled,
                        )
                        .map_err(|e| PipelineError::StageFailed(node.id.clone(), e))?;
                        content.path = Some(destination.to_string_lossy().into_owned());
                        content.owner = Some(arcade_link::ids::BOX.into());
                    }
                    request.inputs.push(content);
                }
                let result = crate::link::consumer::invoke(runtime, &link.app, request, cancelled)
                    .map_err(|e| {
                        if e.code == arcade_link::ErrorCode::Cancelled {
                            PipelineError::Cancelled
                        } else {
                            PipelineError::StageFailed(
                                node.id.clone(),
                                e.user_message(arcade_link::manifest::app_name(&link.app)),
                            )
                        }
                    })?;
                let info = node.describe(runtime)?;
                let mut outputs = Vec::new();
                for content in result.outputs {
                    if !arcade_link::content::accepts_content(&info.outputs, &content) {
                        return Err(PipelineError::StageFailed(
                            node.id.clone(),
                            "peer returned an undeclared output type".into(),
                        ));
                    }
                    outputs.extend(
                        workspace
                            .receive(content, cancelled)
                            .map_err(|e| PipelineError::StageFailed(node.id.clone(), e))?,
                    );
                }
                outputs
            } else {
                // A node's directory destination belongs to this job. Persistent
                // folder grants are never serialized or used as intermediate output.
                let mut options = node.options.clone();
                if let Some(object) = options.as_object_mut() {
                    object.insert(
                        "destinationGrant".into(),
                        serde_json::json!(workspace.directory),
                    );
                }
                let result = runtime
                    .run_tool_with_cancel(
                        ToolRequest {
                            tool_id: node.tool_id.clone(),
                            inputs,
                            options,
                        },
                        cancelled,
                    )
                    .map_err(|e| PipelineError::Execution(node.id.clone(), e))?;
                if result.status == ResultStatus::Error {
                    return Err(PipelineError::StageFailed(
                        node.id.clone(),
                        result.message.unwrap_or_default(),
                    ));
                }
                result
                    .outputs
                    .iter()
                    .map(|v| workspace.keep(v, cancelled, true))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| PipelineError::StageFailed(node.id.clone(), e))?
            };
            results.insert(node.id.clone(), outputs);
        }
        let mut published: HashMap<String, Vec<ToolValue>> = HashMap::new();
        let mut created: Vec<crate::SelectedFile> = Vec::new();
        let publishing = (|| -> Result<(), String> {
            for id in &self.output_nodes {
                let mut values = Vec::new();
                for value in results.get(id).ok_or("missing final stage")? {
                    if value.kind == arcade_contract::ValueKind::Artifact {
                        let path = runtime
                            .grants()
                            .resolve(&value.value)
                            .map_err(|e| e.to_string())?;
                        let file = runtime
                            .publish_staged_output(
                                None,
                                &path,
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_str()
                                    .unwrap_or("pipeline-output"),
                                cancelled,
                            )
                            .map_err(|e| e.to_string())?;
                        values.push(file.as_tool_value());
                        created.push(file);
                    } else {
                        values.push(value.clone());
                    }
                }
                published.insert(id.clone(), values);
            }
            Ok(())
        })();
        if let Err(error) = publishing {
            for file in created {
                if let Ok(path) = runtime.grants().resolve(&file.token) {
                    let _ = std::fs::remove_file(path);
                }
                runtime.grants().revoke(&file.token);
            }
            return Err(if cancelled.load(Ordering::Relaxed) {
                PipelineError::Cancelled
            } else {
                PipelineError::StageFailed(self.id.clone(), error)
            });
        }
        Ok(published)
    }
}

/// All intermediates and their grants end with this run, including failures.
struct Workspace<'a> {
    runtime: &'a Arcade,
    root: tempfile::TempDir,
    directory: String,
    grants: Vec<String>,
}
impl<'a> Workspace<'a> {
    fn new(runtime: &'a Arcade) -> Result<Self, String> {
        let root = tempfile::Builder::new()
            .prefix("pipeline-job-")
            .tempdir_in(runtime.artifact_staging_root())
            .map_err(|e| e.to_string())?;
        let directory = runtime
            .grants()
            .grant_output_directory(root.path())
            .map_err(|e| e.to_string())?
            .token;
        Ok(Self {
            runtime,
            root,
            directory,
            grants: vec![],
        })
    }
    fn keep(
        &mut self,
        value: &ToolValue,
        cancel: &AtomicBool,
        owned: bool,
    ) -> Result<ToolValue, String> {
        use arcade_contract::ValueKind;
        if !matches!(value.kind, ValueKind::Artifact | ValueKind::File)
            || value.mime == "folder/reference"
        {
            return Ok(value.clone());
        }
        let mut selected = None;
        let token = if value.kind == ValueKind::File {
            selected = Some(
                self.runtime
                    .grants()
                    .grant(std::path::Path::new(&value.value))
                    .map_err(|e| e.to_string())?,
            );
            self.grants.push(selected.as_ref().unwrap().token.clone());
            &selected.as_ref().unwrap().token
        } else {
            &value.value
        };
        let source = self
            .runtime
            .grants()
            .resolve(token)
            .map_err(|e| e.to_string())?;
        if source.starts_with(self.root.path()) {
            self.grants.push(token.clone());
            return Ok(if value.kind == ValueKind::File {
                selected.as_ref().unwrap().as_tool_value()
            } else {
                value.clone()
            });
        }
        let destination = self.root.path().join(format!(
            "{}-{}",
            uuid::Uuid::new_v4(),
            source.file_name().unwrap_or_default().to_string_lossy()
        ));
        copy_file(
            self.runtime
                .grants()
                .open_scoped(token)
                .map_err(|e| e.to_string())?,
            &destination,
            cancel,
        )?;
        let selected = self
            .runtime
            .grants()
            .grant(&destination)
            .map_err(|e| e.to_string())?;
        self.grants.push(selected.token.clone());
        if owned && source.starts_with(self.runtime.artifact_staging_root()) {
            let _ = std::fs::remove_file(source);
            self.runtime.grants().revoke(token);
        }
        Ok(selected.as_tool_value())
    }
    fn receive(
        &mut self,
        content: arcade_link::Content,
        cancel: &AtomicBool,
    ) -> Result<Vec<ToolValue>, String> {
        let mut values = Vec::new();
        match arcade_link::content::family(&content.kind) {
            "file" => {
                for path in content.all_paths() {
                    values.push(self.keep(
                        &ToolValue {
                            kind: arcade_contract::ValueKind::File,
                            value: path.to_owned(),
                            mime: content.kind.clone(),
                        },
                        cancel,
                        false,
                    )?);
                }
            }
            "text" => values.push(ToolValue::text(
                arcade_link::handoff::read_text(&content).map_err(|e| e.to_string())?,
                &content.kind,
            )),
            "structured" | "screen" => values.push(ToolValue::text(
                content
                    .data
                    .ok_or("peer returned no structured data")?
                    .to_string(),
                &content.kind,
            )),
            _ => return Err("peer returned unsupported content".into()),
        }
        Ok(values)
    }
}
impl Drop for Workspace<'_> {
    fn drop(&mut self) {
        for token in &self.grants {
            self.runtime.grants().revoke(token);
        }
        self.runtime
            .grants()
            .revoke_output_directory(&self.directory);
    }
}
fn copy_file(
    mut input: std::fs::File,
    path: &std::path::Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    use std::io::{Read, Write};
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(path).map_err(|e| e.to_string())?;
    let mut buffer = [0; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("pipeline cancelled".into());
        }
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
    }
}
fn connection_compatible(output: &str, input: &str, link: bool) -> bool {
    if link {
        arcade_link::content::type_matches(
            &crate::link::link_type_of_output(input),
            &crate::link::link_type_of_output(output),
        )
    } else {
        compatible(output, input)
    }
}

fn visit<'a>(
    node: &'a PipelineNode,
    nodes: &HashMap<&str, &'a PipelineNode>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
    ordered: &mut Vec<&'a PipelineNode>,
) -> Result<(), PipelineError> {
    if visited.contains(node.id.as_str()) {
        return Ok(());
    }
    if !visiting.insert(node.id.as_str()) {
        return Err(PipelineError::Cycle);
    }
    for input in &node.inputs {
        if let InputSource::Node { node_id, .. } = input {
            let dependency = nodes
                .get(node_id.as_str())
                .ok_or_else(|| PipelineError::UnknownStage(node_id.clone()))?;
            visit(dependency, nodes, visiting, visited, ordered)?;
        }
    }
    visiting.remove(node.id.as_str());
    visited.insert(node.id.as_str());
    ordered.push(node);
    Ok(())
}

fn compatible(output_type: &str, input_type: &str) -> bool {
    output_type == input_type || input_type.strip_suffix("[]") == Some(output_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn linked_runtime() -> (tempfile::TempDir, arcade_link::Locations, Arcade) {
        let dir = tempfile::tempdir().unwrap();
        let locations = arcade_link::Locations::under(dir.path());
        let runtime = Arcade::in_memory().unwrap();
        let mut manifest = arcade_link::Manifest::new(
            arcade_link::ids::LENS,
            "1",
            std::env::current_exe().unwrap().to_str().unwrap(),
        );
        manifest.actions.push(
            arcade_link::Action::new("lens.capture", "Capture", "capture")
                .produces(&["file/image", "screen/region"])
                .effects(&["opens-ui"])
                .interactive(true),
        );
        arcade_link::manifest::write_manifest(&locations, &manifest).unwrap();
        runtime
            .link_registry
            .set(arcade_link::SharedRegistry::load(&locations))
            .ok()
            .unwrap();
        (dir, locations, runtime)
    }
    fn capture_pipeline() -> Pipeline {
        serde_json::from_value(serde_json::json!({"id":"capture","name":"Capture","version":1,"nodes":[
            {"id":"capture","link":{"app":"arcade.lens","action":"lens.capture","version":1},"inputs":[]},
            {"id":"resize","toolId":"arcade.image.resize","inputs":[{"kind":"node","nodeId":"capture","outputIndex":0}],"options":{"mode":"percentage","percentage":50}}
        ],"outputNodes":["resize"]})).unwrap()
    }
    #[test]
    fn link_nodes_check_types_availability_versions_and_interactive_position() {
        let (_dir, locations, runtime) = linked_runtime();
        let mut pipeline = capture_pipeline();
        assert!(pipeline.validate(&runtime).is_ok());
        pipeline.nodes[1].tool_id = "arcade.text.clean".into();
        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::TypeMismatch { .. })
        ));
        pipeline.nodes[1] = pipeline.nodes[0].clone();
        pipeline.nodes[1].id = "second-picker".into();
        pipeline.output_nodes = vec!["second-picker".into()];
        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::InteractivePosition(_))
        ));
        pipeline = capture_pipeline();
        pipeline.nodes[0].link.as_mut().unwrap().version = 2;
        assert!(
            pipeline
                .validate(&runtime)
                .unwrap_err()
                .to_string()
                .contains("Needs repair")
        );
        pipeline.nodes[0].link.as_mut().unwrap().version = 1;
        crate::link::LinkSettings {
            enabled: true,
            disabled_peers: vec![arcade_link::ids::LENS.into()],
        }
        .save(&runtime)
        .unwrap();
        assert!(pipeline.validate(&runtime).is_err());
        crate::link::LinkSettings::default().save(&runtime).unwrap();
        let mut manifest = crate::link::consumer::registry(&runtime)
            .snapshot()
            .get(arcade_link::ids::LENS)
            .unwrap()
            .clone();
        manifest.actions[0].available = false;
        arcade_link::manifest::write_manifest(&locations, &manifest).unwrap();
        crate::link::consumer::registry(&runtime).refresh();
        assert!(pipeline.validate(&runtime).is_err());
        std::fs::remove_file(locations.registry.join("arcade.lens.json")).unwrap();
        crate::link::consumer::registry(&runtime).refresh();
        assert!(pipeline.validate(&runtime).is_err());
    }
    #[test]
    fn effect_approval_is_required_and_bound_to_definition_and_effects() {
        let (_dir, locations, runtime) = linked_runtime();
        let mut manifest = crate::link::consumer::registry(&runtime)
            .snapshot()
            .get(arcade_link::ids::LENS)
            .unwrap()
            .clone();
        manifest.actions[0].effects.push("sends-to-device".into());
        arcade_link::manifest::write_manifest(&locations, &manifest).unwrap();
        crate::link::consumer::registry(&runtime).refresh();
        let mut pipeline = capture_pipeline();
        assert!(matches!(
            pipeline.confirm_effects(&runtime),
            Err(PipelineError::NeedsConfirmation(_))
        ));
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = count.clone();
        runtime.set_pipeline_confirmation(move |_, effects| {
            assert!(effects.iter().any(|e| e == "sends-to-device"));
            observed.fetch_add(1, Ordering::Relaxed);
            true
        });
        pipeline.confirm_effects(&runtime).unwrap();
        pipeline.confirm_effects(&runtime).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        pipeline.version += 1;
        pipeline.confirm_effects(&runtime).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 2);
        manifest.actions[0].effects.push("network".into());
        arcade_link::manifest::write_manifest(&locations, &manifest).unwrap();
        crate::link::consumer::registry(&runtime).refresh();
        pipeline.confirm_effects(&runtime).unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 3);
    }
    #[test]
    fn pipeline_files_stay_private_and_only_final_files_survive() {
        let runtime = Arcade::in_memory().unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("source.png");
        image::RgbImage::new(24, 16).save(&source).unwrap();
        let before = std::fs::read(&source).unwrap();
        let selected = runtime.grants().grant(&source).unwrap();
        let pipeline: Pipeline = serde_json::from_value(serde_json::json!({"id":"files","name":"Files","version":1,"nodes":[
            {"id":"resize","toolId":"arcade.image.resize","inputs":[{"kind":"external","index":0}],"options":{"mode":"percentage","percentage":50}},
            {"id":"convert","toolId":"arcade.image.convert","inputs":[{"kind":"node","nodeId":"resize","outputIndex":0}],"options":{"format":"webp"}}
        ],"outputNodes":["convert"]})).unwrap();
        if crate::provider::discover_vips(None).is_empty() {
            return;
        }
        let result = pipeline
            .run(
                &runtime,
                vec![selected.as_tool_value()],
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(result.len(), 1);
        let output = runtime
            .grants()
            .resolve(&result["convert"][0].value)
            .unwrap();
        assert!(output.is_file());
        assert_eq!(image::image_dimensions(output).unwrap(), (12, 8));
        assert_eq!(std::fs::read(&source).unwrap(), before);
        assert_eq!(std::fs::read_dir(source_dir.path()).unwrap().count(), 1);
        assert!(
            !std::fs::read_dir(runtime.artifact_staging_root())
                .unwrap()
                .any(|e| e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("pipeline-job-"))
        );
        let mut broken = pipeline.clone();
        broken.nodes[1].options = serde_json::json!({"format":"bad-format"});
        assert!(
            broken
                .run(
                    &runtime,
                    vec![selected.as_tool_value()],
                    &AtomicBool::new(false)
                )
                .is_err()
        );
        assert!(
            !std::fs::read_dir(runtime.artifact_staging_root())
                .unwrap()
                .any(|e| e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("pipeline-job-"))
        );
    }
    #[test]
    fn rejects_cycles_and_bad_types() {
        let runtime = Arcade::in_memory().unwrap();
        let mut pipeline = Pipeline {
            id: "test".into(),
            name: "Test Pipeline".into(),
            version: 1,
            nodes: vec![
                PipelineNode {
                    id: "a".into(),
                    link: None,
                    tool_id: "arcade.text.case".into(),
                    inputs: vec![InputSource::Node {
                        node_id: "b".into(),
                        output_index: 0,
                    }],
                    options: Value::Null,
                },
                PipelineNode {
                    id: "b".into(),
                    link: None,
                    tool_id: "arcade.text.clean".into(),
                    inputs: vec![InputSource::Node {
                        node_id: "a".into(),
                        output_index: 0,
                    }],
                    options: Value::Null,
                },
            ],
            output_nodes: vec!["b".into()],
        };
        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::Cycle)
        ));
        pipeline.nodes[0].inputs = vec![InputSource::External { index: 0 }];
        assert_eq!(pipeline.validate(&runtime).unwrap().len(), 2);
    }

    #[test]
    fn executes_typed_chain() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "clean".into(),
            name: "Clean Text".into(),
            version: 1,
            nodes: vec![
                PipelineNode {
                    id: "upper".into(),
                    link: None,
                    tool_id: "arcade.text.case".into(),
                    inputs: vec![InputSource::External { index: 0 }],
                    options: serde_json::json!({"mode":"upper"}),
                },
                PipelineNode {
                    id: "trim".into(),
                    link: None,
                    tool_id: "arcade.text.clean".into(),
                    inputs: vec![InputSource::Node {
                        node_id: "upper".into(),
                        output_index: 0,
                    }],
                    options: serde_json::json!({"trim":true}),
                },
            ],
            output_nodes: vec!["trim".into()],
        };
        let results = pipeline
            .run(
                &runtime,
                vec![ToolValue::text(" hello ", "text/plain")],
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(results["trim"][0].value, "HELLO");
    }

    #[test]
    fn validation_rejects_stages_without_required_inputs() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "missing-input".into(),
            name: "Missing Input".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "case".into(),
                link: None,
                tool_id: "arcade.text.case".into(),
                inputs: vec![],
                options: Value::Null,
            }],
            output_nodes: vec!["case".into()],
        };

        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::MissingInputs(stage)) if stage == "case"
        ));
    }

    #[test]
    fn validation_allows_stages_with_optional_inputs_to_receive_none() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "optional-input".into(),
            name: "Optional Input".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "processes".into(),
                link: None,
                tool_id: "arcade.system.process".into(),
                inputs: vec![],
                options: Value::Null,
            }],
            output_nodes: vec!["processes".into()],
        };

        assert!(pipeline.validate(&runtime).is_ok());
    }

    #[test]
    fn validation_enforces_multi_input_minimums() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "two-inputs".into(),
            name: "Two Inputs".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "merge".into(),
                link: None,
                tool_id: "arcade.pdf.merge".into(),
                inputs: vec![InputSource::External { index: 0 }],
                options: Value::Null,
            }],
            output_nodes: vec!["merge".into()],
        };

        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::TooFewInputs {
                stage,
                required: 2,
            }) if stage == "merge"
        ));
    }

    #[test]
    fn validation_rejects_pipelines_without_outputs() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "no-output".into(),
            name: "No Output".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "case".into(),
                link: None,
                tool_id: "arcade.text.case".into(),
                inputs: vec![InputSource::External { index: 0 }],
                options: Value::Null,
            }],
            output_nodes: vec![],
        };

        assert!(matches!(
            pipeline.validate(&runtime),
            Err(PipelineError::NoOutputs)
        ));
    }

    #[test]
    fn validation_allows_inputless_generator_stages() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "system-info".into(),
            name: "System Info".into(),
            version: 1,
            nodes: vec![PipelineNode {
                id: "info".into(),
                link: None,
                tool_id: "arcade.system.system-info".into(),
                inputs: vec![],
                options: Value::Null,
            }],
            output_nodes: vec!["info".into()],
        };

        assert!(pipeline.validate(&runtime).is_ok());
    }
}
