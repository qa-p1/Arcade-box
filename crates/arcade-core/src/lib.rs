//! The shared application runtime. Desktop and CLI call these same APIs.

pub mod artifacts;
pub mod barcode;
pub mod converter_tools;
pub mod developer_tools;
pub mod file_tools;
pub mod grants;
pub mod image;
pub mod jobs;
mod magick;
pub mod media;
pub mod network;
pub mod pdf;
pub mod pipeline;
pub mod plugins;
pub mod process;
pub mod provider;
pub mod screen_tools;
pub mod search;
mod secrets;
pub mod security_tools;
pub mod storage;
pub mod system_tools;
pub mod text_tools;
mod tool_kit;
pub mod utility_tools;
pub mod web;

use arcade_contract::{
    Catalog, ContractError, ImplementationStatus, PrivacyClass, ResultStatus, ToolManifest,
    ToolRequest, ToolResult, ToolValue, ValueKind,
};
use arcade_plugin_host::InstallApproval;
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, RwLock, atomic::AtomicBool},
};
use thiserror::Error;

pub use arcade_plugin_host::InstallApproval as PluginInstallApproval;
pub use arcade_plugin_host::PermissionGrant as PluginPermissionGrant;
pub use grants::{SelectedDirectory, SelectedFile};
pub use plugins::{PluginPreview, PluginSummary};

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("catalog: {0}")]
    Catalog(#[from] ContractError),
    #[error("catalog JSON: {0}")]
    CatalogJson(#[from] serde_json::Error),
    #[error("storage: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("plugin registry: {0}")]
    Plugin(String),
    #[error("temporary runtime storage: {0}")]
    Io(#[from] std::io::Error),
    #[error("file grant: {0}")]
    Grant(#[from] grants::GrantError),
    #[error("tool {0} does not exist")]
    UnknownTool(String),
    #[error("tool {0} is not implemented yet")]
    NotImplemented(String),
    #[error("tool {tool} does not accept input type {mime}")]
    InputType { tool: String, mime: String },
    #[error("{0}")]
    Execution(String),
}

pub struct Arcade {
    base_catalog: Catalog,
    catalog: RwLock<Catalog>,
    storage: storage::Storage,
    grants: grants::FileGrants,
    artifact_staging_root: PathBuf,
    default_output_directory: String,
    plugins: plugins::PluginRuntime,
    _ephemeral_data: Option<tempfile::TempDir>,
}

impl Arcade {
    pub fn open(db_path: &Path) -> Result<Self, CoreError> {
        let catalog = load_catalog()?;
        let storage = storage::Storage::open(db_path)?;
        let plugin_root = db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("plugins");
        Self::from_parts(catalog, storage, plugin_root, None, None)
    }

    /// Open the normal core runtime with an explicit trusted worker path. The
    /// desktop and CLI use `open`, which resolves the packaged worker beside
    /// their executable; tests and embedders can pass the worker directly.
    pub fn open_with_plugin_worker(
        db_path: &Path,
        worker_executable: &Path,
    ) -> Result<Self, CoreError> {
        let catalog = load_catalog()?;
        let storage = storage::Storage::open(db_path)?;
        let plugin_root = db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("plugins");
        Self::from_parts(
            catalog,
            storage,
            plugin_root,
            Some(worker_executable.to_path_buf()),
            None,
        )
    }

    pub fn in_memory() -> Result<Self, CoreError> {
        let catalog = load_catalog()?;
        let storage = storage::Storage::in_memory()?;
        let ephemeral_data = tempfile::tempdir()?;
        let plugin_root = ephemeral_data.path().join("plugins");
        Self::from_parts(catalog, storage, plugin_root, None, Some(ephemeral_data))
    }

    fn from_parts(
        base_catalog: Catalog,
        storage: storage::Storage,
        plugin_root: PathBuf,
        worker_executable: Option<PathBuf>,
        ephemeral_data: Option<tempfile::TempDir>,
    ) -> Result<Self, CoreError> {
        let first_party_ids = base_catalog
            .tools
            .iter()
            .map(|tool| tool.id.clone())
            .collect::<BTreeSet<_>>();
        let data_parent = plugin_root.parent().unwrap_or_else(|| Path::new("."));
        let artifact_staging_root = artifacts::prepare_private_artifact_root(data_parent)?;
        let grants = grants::FileGrants::default();
        let default_output_directory = grants.grant_output_directory(&artifact_staging_root)?.token;
        let plugins = if let Some(worker) = worker_executable {
            plugins::PluginRuntime::with_worker(plugin_root, first_party_ids, worker)
        } else {
            plugins::PluginRuntime::new(plugin_root, first_party_ids)
        };
        let runtime = Self {
            catalog: RwLock::new(base_catalog.clone()),
            base_catalog,
            storage,
            grants,
            artifact_staging_root,
            default_output_directory,
            plugins,
            _ephemeral_data: ephemeral_data,
        };
        runtime.refresh_plugin_catalog()?;
        Ok(runtime)
    }

    pub fn list_tools(&self) -> Vec<ToolManifest> {
        self.catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tools
            .clone()
    }

    pub fn search_tools(&self, query: &str) -> Vec<ToolManifest> {
        let signals = self.storage.ranking_signals().unwrap_or_default();
        let catalog = self
            .catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut ranked: Vec<_> = search::rank(&catalog.tools, query, &signals)
            .into_iter()
            .cloned()
            .collect();
        if let Ok(Some(id)) = self.storage.resolve_alias(query) {
            if let Some(index) = ranked.iter().position(|tool: &ToolManifest| tool.id == id) {
                let matched = ranked.remove(index);
                ranked.insert(0, matched);
            } else if let Some(matched) = catalog.tools.iter().find(|tool| tool.id == id) {
                ranked.insert(0, matched.clone());
            }
        }
        ranked
    }

    pub fn run_tool(&self, request: ToolRequest) -> Result<ToolResult, CoreError> {
        self.run_tool_with_cancel(request, &AtomicBool::new(false))
    }

    pub fn run_tool_with_cancel(
        &self,
        request: ToolRequest,
        cancelled: &AtomicBool,
    ) -> Result<ToolResult, CoreError> {
        self.run_tool_with_progress(request, cancelled, Arc::new(|_| {}))
    }

    pub fn run_tool_with_progress(
        &self,
        request: ToolRequest,
        cancelled: &AtomicBool,
        progress: Arc<dyn Fn(f64) + Send + Sync>,
    ) -> Result<ToolResult, CoreError> {
        let manifest = self
            .catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tools
            .iter()
            .find(|t| t.id == request.tool_id)
            .cloned()
            .ok_or_else(|| CoreError::UnknownTool(request.tool_id.clone()))?;
        if manifest.status == ImplementationStatus::Planned {
            return Err(CoreError::NotImplemented(request.tool_id));
        }
        for input in &request.inputs {
            if input.kind == ValueKind::Artifact {
                self.grants.verify_type(&input.value, &input.mime)?;
            }
            if !manifest
                .inputs
                .iter()
                .any(|accepted| input_compatible(&input.mime, accepted))
            {
                return Err(CoreError::InputType {
                    tool: manifest.id.clone(),
                    mime: input.mime.clone(),
                });
            }
        }
        let executed = if manifest
            .execution
            .get("runtime")
            .and_then(serde_json::Value::as_str)
            == Some("wasm")
        {
            self.plugins.execute(&request, &self.grants, cancelled)
        } else if manifest
            .execution
            .get("runtime")
            .and_then(serde_json::Value::as_str)
            == Some("pipeline")
        {
            self.run_pipeline_tool(&manifest, &request, cancelled)
        } else if manifest.id.starts_with("arcade.image.") {
            image::execute(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.pdf.") {
            pdf::execute(&manifest, &request, &self.grants, cancelled)
        } else if manifest.id == "arcade.audio.text-to-speech" {
            media::text_to_speech(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.video.")
            || manifest.id.starts_with("arcade.audio.")
        {
            media::execute_with_progress(&manifest, &request, &self.grants, cancelled, progress)
        } else if manifest.id.starts_with("arcade.barcode.") {
            barcode::execute(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.network.") {
            network::execute(&manifest, &request, cancelled)
        } else if manifest.id.starts_with("arcade.web.") {
            web::execute_with_progress(&manifest, &request, self, cancelled, progress)
        } else if manifest.id.starts_with("arcade.system.") {
            system_tools::execute(&manifest, &request, cancelled)
        } else if manifest.id.starts_with("arcade.files.") {
            file_tools::execute(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.security.") {
            security_tools::execute(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.developer.") {
            developer_tools::execute(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.convert.") {
            converter_tools::execute(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.text.") {
            text_tools::execute(&manifest, &request, self, cancelled)
        } else if manifest.id.starts_with("arcade.utility.") {
            utility_tools::execute(&manifest, &request, cancelled)
        } else {
            Err(format!("No executor is registered for {}", manifest.id))
        };
        let result = match executed {
            Ok(result) => result,
            Err(message) => ToolResult {
                tool_id: manifest.id.clone(),
                status: ResultStatus::Error,
                outputs: vec![],
                message: Some(message),
                warnings: vec![],
                metadata: Default::default(),
            },
        };
        self.storage.record_usage(&manifest.id, &result)?;
        Ok(result)
    }

    pub fn catalog(&self) -> Catalog {
        self.catalog
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn list_plugins(&self) -> Result<Vec<plugins::PluginSummary>, CoreError> {
        self.plugins.list().map_err(CoreError::Plugin)
    }

    pub fn preview_plugin(&self, source_dir: &Path) -> Result<plugins::PluginPreview, CoreError> {
        self.plugins.preview(source_dir).map_err(CoreError::Plugin)
    }

    pub fn install_plugin(
        &self,
        source_dir: &Path,
        approval: InstallApproval,
    ) -> Result<plugins::PluginSummary, CoreError> {
        let installed = self
            .plugins
            .install(source_dir, approval)
            .map_err(CoreError::Plugin)?;
        self.refresh_plugin_catalog()?;
        Ok(installed)
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) -> Result<(), CoreError> {
        self.plugins
            .uninstall(plugin_id)
            .map_err(CoreError::Plugin)?;
        self.refresh_plugin_catalog()
    }

    pub fn run_plugin_dev(
        &self,
        source_dir: &Path,
        request: &ToolRequest,
        approval: InstallApproval,
        cancelled: &AtomicBool,
    ) -> Result<ToolResult, CoreError> {
        self.plugins
            .run_dev(source_dir, request, approval, &self.grants, cancelled)
            .map_err(CoreError::Plugin)
    }

    pub fn list_pipelines(&self) -> Result<Vec<pipeline::Pipeline>, CoreError> {
        self.storage
            .list_pipeline_definitions()?
            .into_iter()
            .map(|(_, _, definition)| {
                let mut pipeline: pipeline::Pipeline =
                    serde_json::from_str(&definition).map_err(CoreError::CatalogJson)?;
                if pipeline.name.trim().is_empty() {
                    pipeline.name = humanize_id(&pipeline.id);
                }
                Ok(pipeline)
            })
            .collect()
    }

    pub fn save_pipeline(
        &self,
        mut definition: pipeline::Pipeline,
    ) -> Result<pipeline::Pipeline, CoreError> {
        validate_pipeline_id(&definition.id)?;
        definition.name = definition.name.trim().to_owned();
        if definition.name.is_empty()
            || definition.name.len() > 100
            || definition.name.chars().any(char::is_control)
        {
            return Err(CoreError::Execution(
                "pipeline name must contain 1 to 100 printable characters".into(),
            ));
        }
        let pipeline_tool_id = format!("arcade.pipeline.{}", definition.id);
        if self
            .plugins
            .list()
            .map_err(CoreError::Plugin)?
            .iter()
            .any(|plugin| plugin.manifest.tool_manifest.id == pipeline_tool_id)
        {
            return Err(CoreError::Execution(
                "pipeline ID conflicts with an installed plugin".into(),
            ));
        }
        if definition.version == 0 {
            return Err(CoreError::Execution(
                "pipeline version must be a positive integer".into(),
            ));
        }
        definition
            .validate(self)
            .map_err(|error| CoreError::Execution(error.to_string()))?;
        let encoded = serde_json::to_string(&definition)?;
        if encoded.len() > 1024 * 1024 {
            return Err(CoreError::Execution(
                "pipeline definition exceeds the 1 MiB limit".into(),
            ));
        }
        if let Some((old_version, old_definition)) =
            self.storage.pipeline_definition(&definition.id)?
        {
            if definition.version < old_version {
                return Err(CoreError::Execution(
                    "pipeline versions cannot be downgraded".into(),
                ));
            }
            if definition.version == old_version && encoded != old_definition {
                return Err(CoreError::Execution(
                    "changed pipeline definitions need a higher version".into(),
                ));
            }
        }
        self.storage
            .save_pipeline_definition(&definition.id, definition.version, &encoded)?;
        self.refresh_plugin_catalog()?;
        Ok(definition)
    }

    pub fn delete_pipeline(&self, id: &str) -> Result<(), CoreError> {
        validate_pipeline_id(id)?;
        self.storage.delete_pipeline_definition(id)?;
        self.refresh_plugin_catalog()
    }

    pub fn run_saved_pipeline(
        &self,
        id: &str,
        external: Vec<ToolValue>,
        cancelled: &AtomicBool,
    ) -> Result<HashMap<String, Vec<ToolValue>>, CoreError> {
        let pipeline = self.load_pipeline(id)?;
        pipeline
            .run(self, external, cancelled)
            .map_err(|error| CoreError::Execution(error.to_string()))
    }

    fn load_pipeline(&self, id: &str) -> Result<pipeline::Pipeline, CoreError> {
        let (_, definition) = self
            .storage
            .pipeline_definition(id)?
            .ok_or_else(|| CoreError::Execution(format!("pipeline {id} does not exist")))?;
        let mut pipeline: pipeline::Pipeline = serde_json::from_str(&definition)?;
        if pipeline.name.trim().is_empty() {
            pipeline.name = humanize_id(&pipeline.id);
        }
        Ok(pipeline)
    }

    fn run_pipeline_tool(
        &self,
        manifest: &ToolManifest,
        request: &ToolRequest,
        cancelled: &AtomicBool,
    ) -> Result<ToolResult, String> {
        let pipeline_id = manifest
            .id
            .strip_prefix("arcade.pipeline.")
            .ok_or("Invalid saved pipeline tool ID")?;
        let pipeline = self
            .load_pipeline(pipeline_id)
            .map_err(|error| error.to_string())?;
        let results = pipeline
            .run(self, request.inputs.clone(), cancelled)
            .map_err(|error| error.to_string())?;
        let mut outputs = Vec::new();
        for node_id in &pipeline.output_nodes {
            if let Some(node_outputs) = results.get(node_id) {
                outputs.extend(node_outputs.iter().cloned());
            }
        }
        Ok(ToolResult {
            tool_id: manifest.id.clone(),
            status: ResultStatus::Success,
            outputs,
            message: None,
            warnings: Vec::new(),
            metadata: Default::default(),
        })
    }

    fn refresh_plugin_catalog(&self) -> Result<(), CoreError> {
        let mut catalog = self.base_catalog.clone();
        catalog
            .tools
            .extend(self.plugins.installed_tools().map_err(CoreError::Plugin)?);
        let pipeline_tools = catalog.tools.clone();
        for pipeline in self.list_pipelines()? {
            catalog
                .tools
                .push(pipeline_manifest(&pipeline, &pipeline_tools));
        }
        catalog.validate()?;
        *self
            .catalog
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = catalog;
        Ok(())
    }

    pub fn storage(&self) -> &storage::Storage {
        &self.storage
    }

    pub fn grants(&self) -> &grants::FileGrants {
        &self.grants
    }

    pub fn artifact_staging_root(&self) -> &Path {
        &self.artifact_staging_root
    }

    pub fn grant_output_directory(&self, path: &Path) -> Result<SelectedDirectory, CoreError> {
        self.grants
            .grant_output_directory(path)
            .map_err(CoreError::Grant)
    }

    pub fn revoke_output_directory(&self, token: &str) {
        self.grants.revoke_output_directory(token);
    }

    pub fn grant_input_directory(&self, path: &Path) -> Result<SelectedDirectory, CoreError> {
        self.grants
            .grant_input_directory(path)
            .map_err(CoreError::Grant)
    }

    pub fn revoke_input_directory(&self, token: &str) {
        self.grants.revoke_input_directory(token);
    }

    pub fn publish_staged_output(
        &self,
        directory_token: Option<&str>,
        staged_path: &Path,
        requested_name: &str,
        cancelled: &AtomicBool,
    ) -> Result<SelectedFile, CoreError> {
        let token = directory_token.unwrap_or(&self.default_output_directory);
        self.grants
            .publish_staged_output(token, staged_path, requested_name, cancelled)
            .map_err(CoreError::Grant)
    }

    /// Copy one of Arcade Box's granted artifacts to a path selected by the
    /// native Save As dialog. The renderer never supplies an output path.
    pub fn save_artifact_as(
        &self,
        source_token: &str,
        destination: &Path,
    ) -> Result<SelectedFile, CoreError> {
        let parent = destination.parent().ok_or_else(|| {
            CoreError::Execution("Save As destination has no parent directory".into())
        })?;
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CoreError::Execution("Save As destination needs a valid filename".into())
            })?;
        let directory = self.grants.grant_output_directory(parent)?;
        let result = self.grants.copy_granted_file_to_directory_exact(
            source_token,
            &directory.token,
            name,
            &AtomicBool::new(false),
        );
        self.grants.revoke_output_directory(&directory.token);
        result.map_err(CoreError::Grant)
    }
}

fn validate_pipeline_id(id: &str) -> Result<(), CoreError> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || id.starts_with('-')
        || id.ends_with('-')
        || id.contains("--")
    {
        return Err(CoreError::Execution(
            "pipeline ID must use lowercase letters, numbers, and single hyphens".into(),
        ));
    }
    Ok(())
}

fn humanize_id(id: &str) -> String {
    id.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn pipeline_manifest(pipeline: &pipeline::Pipeline, tools: &[ToolManifest]) -> ToolManifest {
    let mut input_types = BTreeSet::new();
    let mut output_types = BTreeSet::new();
    let mut related_tools = BTreeSet::new();
    let mut privacy_class = PrivacyClass::Local;
    let mut implemented = true;
    let by_node = pipeline
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    for node in &pipeline.nodes {
        related_tools.insert(node.tool_id.clone());
        let Some(tool) = tools.iter().find(|tool| tool.id == node.tool_id) else {
            implemented = false;
            continue;
        };
        implemented &= tool.status == ImplementationStatus::Implemented;
        if tool.privacy_class == PrivacyClass::Cloud {
            privacy_class = PrivacyClass::Cloud;
        } else if tool.privacy_class == PrivacyClass::Network
            && privacy_class == PrivacyClass::Local
        {
            privacy_class = PrivacyClass::Network;
        }
        for input in &node.inputs {
            if matches!(input, pipeline::InputSource::External { .. }) {
                input_types.extend(
                    tool.inputs
                        .iter()
                        .map(|value| value.strip_suffix("[]").unwrap_or(value).to_owned()),
                );
            }
        }
    }
    for output_node in &pipeline.output_nodes {
        let Some(node) = by_node.get(output_node.as_str()) else {
            continue;
        };
        if let Some(tool) = tools.iter().find(|tool| tool.id == node.tool_id) {
            output_types.extend(tool.outputs.iter().cloned());
        }
    }
    if output_types.is_empty() {
        output_types.insert("text/plain".into());
    }
    let name = if pipeline.name.trim().is_empty() {
        humanize_id(&pipeline.id)
    } else {
        pipeline.name.clone()
    };
    let mut phrases = vec![name.clone(), "saved pipeline".into()];
    phrases.extend(pipeline.id.split('-').map(str::to_owned));
    ToolManifest {
        id: format!("arcade.pipeline.{}", pipeline.id),
        version: format!("{}.0.0", pipeline.version),
        api_version: arcade_contract::TOOL_API_VERSION.into(),
        name: name.clone(),
        description: format!("Run the saved {name} pipeline."),
        category: "Pipelines".into(),
        aliases: vec![pipeline.id.clone()],
        privacy_class,
        inputs: input_types.into_iter().collect(),
        outputs: output_types.into_iter().collect(),
        providers: Vec::new(),
        status: if implemented {
            ImplementationStatus::Implemented
        } else {
            ImplementationStatus::Partial
        },
        platforms: Default::default(),
        permissions: serde_json::json!({}),
        execution: serde_json::json!({
            "runtime": "pipeline",
            "pipelineId": pipeline.id,
        }),
        phrases,
        related_tools: related_tools.into_iter().collect(),
        ui: None,
    }
}

fn load_catalog() -> Result<Catalog, CoreError> {
    let catalog: Catalog = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../catalog/tools.json"
    )))?;
    catalog.validate()?;
    Ok(catalog)
}

fn input_compatible(actual: &str, declared: &str) -> bool {
    let declared = declared.strip_suffix("[]").unwrap_or(declared);
    actual == declared
        || declared == "file/any" && actual.starts_with("file/")
        || declared == "file/media" && matches!(actual, "file/video" | "file/audio")
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_contract::{ResultStatus, ToolValue};

    #[test]
    fn catalog_loads_and_is_unique() {
        let runtime = Arcade::in_memory().unwrap();
        let tools = runtime.list_tools();
        let ids = tools
            .iter()
            .map(|tool| &tool.id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), tools.len());
        assert!(tools.len() > 100);
    }

    #[test]
    fn real_tool_runs_via_registry() {
        let runtime = Arcade::in_memory().unwrap();
        let result = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.text.case".into(),
                inputs: vec![ToolValue::text("hello world", "text/plain")],
                options: serde_json::json!({"mode":"upper"}),
            })
            .unwrap();
        assert_eq!(result.status, ResultStatus::Success);
        assert_eq!(result.outputs[0].value, "HELLO WORLD");
    }

    #[test]
    fn developer_and_converter_tools_run_through_registry() {
        let runtime = Arcade::in_memory().unwrap();
        let calc = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.convert.calculator".into(),
                inputs: vec![ToolValue::text("928 * 41", "text/plain")],
                options: serde_json::Value::Null,
            })
            .unwrap();
        assert_eq!(calc.status, ResultStatus::Success);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&calc.outputs[0].value).unwrap()["value"],
            38048.0
        );

        let regex = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.developer.regex".into(),
                inputs: vec![ToolValue::text("Order 481 and 29", "text/plain")],
                options: serde_json::json!({"pattern":"\\d+","flags":"g"}),
            })
            .unwrap();
        assert_eq!(regex.status, ResultStatus::Success);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&regex.outputs[0].value).unwrap()["matchCount"],
            2
        );

        let units = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.convert.units".into(),
                inputs: vec![ToolValue::text("32 F to C", "text/plain")],
                options: serde_json::Value::Null,
            })
            .unwrap();
        assert_eq!(units.status, ResultStatus::Success);
        let converted: serde_json::Value = serde_json::from_str(&units.outputs[0].value).unwrap();
        assert!(converted["value"].as_f64().unwrap().abs() < 1e-9);
    }
}
