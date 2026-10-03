use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::Duration,
};

use arcade_contract::{
    ResultStatus, ToolRequest as CoreToolRequest, ToolResult as CoreToolResult, ToolValue,
    ValueKind,
};
use serde_json::Value;
use thiserror::Error;
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, HasSelf, Linker},
};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::{
    install::InstalledPlugin,
    manifest::{
        AccessScope, ManifestError, PluginPermissions, max_component_bytes, sha256_component,
    },
};

wasmtime::component::bindgen!({
    path: "../../sdk/wit/arcade-tool/1.0.0",
    world: "arcade-tool",
});

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_ITEMS: usize = 1024;
const MAX_SELECTED_READ_BYTES: u32 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct PluginHostConfig {
    pub memory_bytes: usize,
    pub fuel: u64,
    pub max_stack_bytes: usize,
    pub timeout: Duration,
}

impl Default for PluginHostConfig {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024,
            fuel: 25_000_000,
            max_stack_bytes: 1024 * 1024,
            timeout: Duration::from_secs(2),
        }
    }
}

/// A user-selected file opened by the trusted host and bound to one input
/// position. Components never receive or resolve its path.
pub struct GrantedInput {
    index: u32,
    reader: Arc<Mutex<Box<dyn ReadSeek + Send>>>,
}

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

impl GrantedInput {
    pub fn from_reader<R: Read + Seek + Send + 'static>(index: u32, reader: R) -> Self {
        Self {
            index,
            reader: Arc::new(Mutex::new(Box::new(reader))),
        }
    }
}

struct StoreData {
    limits: StoreLimits,
    permissions: PluginPermissions,
    inputs: BTreeMap<u32, Arc<Mutex<Box<dyn ReadSeek + Send>>>>,
    max_read_bytes: u32,
    wasi_ctx: WasiCtx,
    resource_table: ResourceTable,
}

impl WasiView for StoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

impl arcade::tool::capabilities::Host for StoreData {
    fn read_selected_input(
        &mut self,
        input_index: u32,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, String> {
        if self.permissions.filesystem.read != AccessScope::UserSelected {
            return Err("filesystem read permission was not granted".into());
        }
        if max_bytes > self.max_read_bytes {
            return Err(format!(
                "read request exceeds {} byte chunk limit",
                self.max_read_bytes
            ));
        }
        let reader = self
            .inputs
            .get(&input_index)
            .ok_or_else(|| "input is not an explicitly selected file".to_string())?;
        let mut reader = reader
            .lock()
            .map_err(|_| "selected input handle is unavailable".to_string())?;
        reader
            .seek(SeekFrom::Start(offset))
            .map_err(|_| "could not seek selected input".to_string())?;
        let mut bytes = vec![0; max_bytes as usize];
        let read = reader
            .read(&mut bytes)
            .map_err(|_| "could not read selected input".to_string())?;
        bytes.truncate(read);
        Ok(bytes)
    }
}

impl arcade::tool::types::Host for StoreData {}

pub struct PluginHost {
    engine: Engine,
    config: PluginHostConfig,
    /// Epoch interruption is engine-wide. Serialize calls on this engine so
    /// the per-call timeout cannot interrupt an unrelated plugin invocation.
    execution_gate: Mutex<()>,
}

impl PluginHost {
    pub fn new(config: PluginHostConfig) -> Result<Self, PluginHostError> {
        if config.memory_bytes == 0
            || config.fuel == 0
            || config.max_stack_bytes == 0
            || config.timeout.is_zero()
        {
            return Err(PluginHostError::InvalidLimits);
        }
        let mut wasm_config = Config::new();
        wasm_config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true)
            .max_wasm_stack(config.max_stack_bytes);
        let engine = Engine::new(&wasm_config)?;
        Ok(Self {
            engine,
            config,
            execution_gate: Mutex::new(()),
        })
    }

    pub fn execute(
        &self,
        plugin: &InstalledPlugin,
        request: &CoreToolRequest,
        granted_inputs: Vec<GrantedInput>,
    ) -> Result<CoreToolResult, PluginHostError> {
        let _guard = self
            .execution_gate
            .lock()
            .map_err(|_| PluginHostError::Poisoned)?;
        if request.tool_id != plugin.manifest.tool_manifest.id {
            return Err(PluginHostError::ToolIdMismatch);
        }
        let permissions = plugin.manifest.permissions()?;
        let requested = permissions.requested_grants();
        if !requested.is_subset(&plugin.grants) {
            return Err(PluginHostError::PermissionGrantRequired);
        }
        if granted_inputs
            .iter()
            .any(|input| !request_has_readable_file(&request.inputs, input.index as usize))
        {
            return Err(PluginHostError::InvalidGrantedInput);
        }
        let unique_indices = granted_inputs
            .iter()
            .map(|input| input.index)
            .collect::<BTreeSet<_>>();
        if unique_indices.len() != granted_inputs.len() {
            return Err(PluginHostError::InvalidGrantedInput);
        }
        if request.inputs.iter().any(|input| {
            !plugin
                .manifest
                .tool_manifest
                .inputs
                .iter()
                .any(|declared| declared.strip_suffix("[]").unwrap_or(declared) == input.mime)
        }) {
            return Err(PluginHostError::UndeclaredInputType);
        }
        let request = to_wit_request(request)?;
        let component_bytes = read_component_bounded(&plugin.component_path)?;
        if sha256_component(&component_bytes) != plugin.manifest.package.component_sha256 {
            return Err(PluginHostError::ComponentHashMismatch);
        }
        let component = Component::new(&self.engine, component_bytes)?;
        let mut linker = Linker::new(&self.engine);
        ArcadeTool::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
        // Rust's `wasm32-wasip2` target includes a small number of standard
        // WASI imports (for example `wasi:io/poll`). Supply the Preview 2
        // implementation with an empty context: no preopened directories,
        // inherited stdio, process arguments, environment, or socket address
        // permissions. File access is available only through the scoped Arcade
        // capability above.
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;

        // Resolve imports before execution. WASI Preview 2 imports receive only
        // the empty, deny-by-default context configured below. No filesystem
        // preopen, network address, process API, or inherited host handle is
        // granted to the component.
        linker.instantiate_pre(&component)?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.config.memory_bytes)
            .instances(64)
            .memories(1)
            .tables(16)
            .table_elements(10_000)
            .build();
        let input_handles = granted_inputs
            .into_iter()
            .map(|input| (input.index, input.reader))
            .collect::<BTreeMap<_, _>>();
        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder
            .allow_tcp(false)
            .allow_udp(false)
            .allow_ip_name_lookup(false)
            .socket_addr_check(|_, _| Box::pin(async { false }));
        let mut store = Store::new(
            &self.engine,
            StoreData {
                limits,
                permissions,
                inputs: input_handles,
                max_read_bytes: MAX_SELECTED_READ_BYTES,
                wasi_ctx: wasi_builder.build(),
                resource_table: ResourceTable::new(),
            },
        );
        store.limiter(|state| &mut state.limits);
        store.set_fuel(self.config.fuel)?;
        store.set_epoch_deadline(1);

        let timed_out = Arc::new(AtomicBool::new(false));
        let timer_timed_out = Arc::clone(&timed_out);
        let timer_engine = self.engine.clone();
        let (timer_done, timer_wait) = mpsc::sync_channel::<()>(0);
        let timeout = self.config.timeout;
        let timer = thread::spawn(move || {
            if matches!(
                timer_wait.recv_timeout(timeout),
                Err(RecvTimeoutError::Timeout)
            ) {
                timer_timed_out.store(true, Ordering::SeqCst);
                timer_engine.increment_epoch();
            }
        });

        let call_result = (|| {
            let bindings = ArcadeTool::instantiate(&mut store, &component, &linker)?;
            let response = bindings.call_run(&mut store, &request)?;
            Ok::<_, wasmtime::Error>(response)
        })();
        drop(timer_done);
        let _ = timer.join();

        let response = match call_result {
            Ok(response) => response,
            Err(_error) if timed_out.load(Ordering::SeqCst) => {
                return Err(PluginHostError::Timeout(self.config.timeout));
            }
            Err(error) => return Err(PluginHostError::WasmtimeError(error)),
        };
        let output = response.map_err(|error| PluginHostError::PluginReported {
            code: error.code,
            message: error.message,
        })?;
        let result = from_wit_result(plugin.manifest.tool_manifest.id.clone(), output)?;
        if result.outputs.iter().any(|value| {
            !plugin
                .manifest
                .tool_manifest
                .outputs
                .iter()
                .any(|declared| declared.strip_suffix("[]").unwrap_or(declared) == value.mime)
        }) {
            return Err(PluginHostError::UndeclaredOutputType);
        }
        Ok(result)
    }
}

fn request_has_readable_file(inputs: &[ToolValue], index: usize) -> bool {
    inputs
        .get(index)
        .is_some_and(|input| matches!(input.kind, ValueKind::File | ValueKind::Artifact))
}

fn read_component_bounded(path: &std::path::Path) -> Result<Vec<u8>, PluginHostError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PluginHostError::UnsafeComponentPath);
    }
    let max_bytes = max_component_bytes() as u64;
    if metadata.len() > max_bytes {
        return Err(PluginHostError::ComponentTooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(path)?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(PluginHostError::ComponentTooLarge);
    }
    Ok(bytes)
}

fn to_wit_request(
    request: &CoreToolRequest,
) -> Result<arcade::tool::types::ToolRequest, PluginHostError> {
    if request.inputs.len() > MAX_ITEMS {
        return Err(PluginHostError::RequestTooLarge);
    }
    let options_json = serde_json::to_string(&request.options)?;
    let mut total = request.tool_id.len().saturating_add(options_json.len());
    for value in &request.inputs {
        total = total
            .saturating_add(value.value.len())
            .saturating_add(value.mime.len());
        if total > MAX_REQUEST_BYTES {
            return Err(PluginHostError::RequestTooLarge);
        }
    }
    let inputs = request
        .inputs
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let kind = match value.kind {
                ValueKind::Text => arcade::tool::types::ValueKind::Text,
                ValueKind::File => arcade::tool::types::ValueKind::File,
                ValueKind::Url => arcade::tool::types::ValueKind::Url,
                ValueKind::Artifact => arcade::tool::types::ValueKind::Artifact,
            };
            arcade::tool::types::ToolValue {
                kind,
                value: match value.kind {
                    ValueKind::File | ValueKind::Artifact => format!("input:{index}"),
                    ValueKind::Text | ValueKind::Url => value.value.clone(),
                },
                mime: value.mime.clone(),
            }
        })
        .collect::<Vec<_>>();
    Ok(arcade::tool::types::ToolRequest {
        tool_id: request.tool_id.clone(),
        inputs,
        options_json,
    })
}

fn from_wit_result(
    tool_id: String,
    output: arcade::tool::types::ToolResult,
) -> Result<CoreToolResult, PluginHostError> {
    if output.outputs.len() > MAX_ITEMS || output.warnings.len() > MAX_ITEMS {
        return Err(PluginHostError::ResponseTooLarge);
    }
    let mut total = output.metadata_json.len();
    for warning in &output.warnings {
        total = total.saturating_add(warning.len());
    }
    let outputs = output
        .outputs
        .into_iter()
        .map(|value| {
            if matches!(
                value.kind,
                arcade::tool::types::ValueKind::File | arcade::tool::types::ValueKind::Artifact
            ) {
                return Err(PluginHostError::UnsupportedOutputKind);
            }
            total = total
                .saturating_add(value.value.len())
                .saturating_add(value.mime.len());
            let kind = match value.kind {
                arcade::tool::types::ValueKind::Text => ValueKind::Text,
                arcade::tool::types::ValueKind::File => ValueKind::File,
                arcade::tool::types::ValueKind::Url => ValueKind::Url,
                arcade::tool::types::ValueKind::Artifact => ValueKind::Artifact,
            };
            Ok(ToolValue {
                kind,
                value: value.value,
                mime: value.mime,
            })
        })
        .collect::<Result<Vec<_>, PluginHostError>>()?;
    if total > MAX_REQUEST_BYTES {
        return Err(PluginHostError::ResponseTooLarge);
    }
    let metadata: Value = serde_json::from_str(&output.metadata_json)?;
    let metadata = metadata
        .as_object()
        .ok_or(PluginHostError::InvalidMetadata)?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    Ok(CoreToolResult {
        tool_id,
        status: ResultStatus::Success,
        outputs,
        message: None,
        warnings: output.warnings,
        metadata,
    })
}

#[derive(Debug, Error)]
pub enum PluginHostError {
    #[error("invalid plugin execution limits")]
    InvalidLimits,
    #[error("plugin execution lock is unavailable")]
    Poisoned,
    #[error("request tool ID does not match installed plugin")]
    ToolIdMismatch,
    #[error("plugin permission grant is missing")]
    PermissionGrantRequired,
    #[error("host received a file grant that does not map to a file/artifact input")]
    InvalidGrantedInput,
    #[error("plugin request contains an input MIME type that is not declared in its manifest")]
    UndeclaredInputType,
    #[error("plugin returned an output MIME type that is not declared in its manifest")]
    UndeclaredOutputType,
    #[error("plugin returned a file/artifact output without a scoped output capability")]
    UnsupportedOutputKind,
    #[error("plugin input request exceeds the host limit")]
    RequestTooLarge,
    #[error("plugin output exceeds the host limit")]
    ResponseTooLarge,
    #[error("plugin returned metadata that is not a JSON object")]
    InvalidMetadata,
    #[error("plugin exceeded its {0:?} wall-clock deadline")]
    Timeout(Duration),
    #[error("plugin reported {code}: {message}")]
    PluginReported { code: String, message: String },
    #[error("plugin component hash differs from its installed manifest")]
    ComponentHashMismatch,
    #[error("plugin component path must be a regular, non-symlink file")]
    UnsafeComponentPath,
    #[error("plugin component exceeds the 64 MiB package limit")]
    ComponentTooLarge,
    #[error("component file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid plugin manifest: {0}")]
    Manifest(#[from] ManifestError),
    #[error("JSON conversion failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Wasmtime error: {0}")]
    WasmtimeError(#[from] wasmtime::Error),
}
