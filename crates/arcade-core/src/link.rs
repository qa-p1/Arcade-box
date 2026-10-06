//! Arcade Link: Arcade Box's manifest, its actions and how Link requests run.
//!
//! Box is the ecosystem's transform engine. Every implemented tool is
//! exposed as `box:<tool-id>`, every preset as `box:<tool-id>#<preset>`.
//! The manifest is built from the embedded catalog and a cached provider
//! probe, so readers never trigger probes.
//!
//! A path that arrives over the Link counts as user-selected **for that job
//! only** (a delegated selection grant): it is canonicalized and checked like
//! a dialog selection by [`crate::grants::FileGrants::grant`], and the grant
//! is revoked when the job ends. Outputs follow Box's rule: new files in
//! Box's own artifact folder, never an overwrite.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use arcade_contract::{
    ImplementationStatus, PrivacyClass, ResultStatus, ToolManifest, ToolRequest, ToolResult,
    ToolValue, ValueKind,
};
use arcade_link::server::{Handler, InvokeContext, Reply};
use arcade_link::{Action, Content, InvokeRequest, InvokeResult, LinkError, Manifest, ids};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::Arcade;
use crate::pipeline::{InputSource, Pipeline};
use crate::provider;

pub mod consumer;

/// Storage keys for the Connected apps settings and the provider cache.
pub const SETTING_ENABLED: &str = "link_enabled";
pub const SETTING_DISABLED_PEERS: &str = "link_disabled_peers";
pub const SETTING_PROVIDER_CACHE: &str = "link_provider_cache";

/// "Connect with other Arcade apps" and the per-app toggles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSettings {
    pub enabled: bool,
    pub disabled_peers: Vec<String>,
}

impl Default for LinkSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            disabled_peers: Vec::new(),
        }
    }
}

impl LinkSettings {
    pub fn load(runtime: &Arcade) -> Self {
        let storage = runtime.storage();
        Self {
            enabled: storage
                .setting(SETTING_ENABLED)
                .ok()
                .flatten()
                .is_none_or(|v| v != "false"),
            disabled_peers: storage
                .setting(SETTING_DISABLED_PEERS)
                .ok()
                .flatten()
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default(),
        }
    }

    pub fn save(&self, runtime: &Arcade) -> Result<(), String> {
        let storage = runtime.storage();
        storage
            .set_setting(SETTING_ENABLED, if self.enabled { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        storage
            .set_setting(
                SETTING_DISABLED_PEERS,
                &serde_json::to_string(&self.disabled_peers).unwrap_or_else(|_| "[]".into()),
            )
            .map_err(|e| e.to_string())
    }

    /// Whether entries from `peer` may appear in Box.
    pub fn uses(&self, peer: &str) -> bool {
        self.enabled && !self.disabled_peers.iter().any(|p| p == peer)
    }
}

// ---- Provider cache ----------------------------------------------------------

/// Whether one catalog provider (`media.ffmpeg`, `image.vips`, …) is usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderState {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub type ProviderCache = BTreeMap<String, ProviderState>;

/// Built-in capabilities that need nothing installed.
fn builtin_provider(id: &str) -> bool {
    [
        "core.",
        "barcode.",
        "crypto.",
        "platform.",
        "network.",
        "currency.",
    ]
    .iter()
    .any(|prefix| id.starts_with(prefix))
        || matches!(
            id,
            "pdf.create" | "pdf.attachments" | "pdf.text" | "pdf.images"
        )
}

fn state(found: bool, missing: &str) -> ProviderState {
    ProviderState {
        available: found,
        reason: (!found).then(|| missing.to_string()),
    }
}

/// The external providers Box knows how to check.
const PROBED: &[&str] = &[
    "media.ffmpeg",
    "media.probe",
    "image.vips",
    "pdf.qpdf",
    "pdf.info",
    "pdf.render",
    "pdf.ghostscript",
    "ocr.tesseract",
    "pdf.ocrmypdf",
    "image.imagemagick",
    "image.model.background-removal",
    "image.model.upscale",
    "web.yt-dlp",
    "media.ytdlp",
    "network.curl",
];

/// Checks one provider; `None` if Box can't check it. Slow (runs the tool's
/// `--version`): never on a UI thread.
fn probe_one(id: &str) -> Option<ProviderState> {
    let compatible = |list: Vec<provider::ProviderInfo>| list.iter().any(|p| p.compatible);
    Some(match id {
        "media.ffmpeg" | "media.probe" => state(
            compatible(provider::discover_ffmpeg(None)),
            "FFmpeg isn't installed",
        ),
        "image.vips" => state(
            compatible(provider::discover_vips(None)),
            "libvips isn't installed",
        ),
        "pdf.qpdf" => state(
            compatible(provider::discover_qpdf(None)),
            "qpdf isn't installed",
        ),
        "pdf.info" | "pdf.render" => state(
            compatible(provider::discover_poppler()),
            "Poppler isn't installed",
        ),
        "pdf.ghostscript" => state(
            provider::find_system_executable("gs").is_some(),
            "Ghostscript isn't installed",
        ),
        "ocr.tesseract" => state(
            compatible(provider::discover_tesseract()),
            "Tesseract isn't installed",
        ),
        "pdf.ocrmypdf" => state(
            compatible(provider::discover_ocrmypdf()),
            "OCRmyPDF isn't installed",
        ),
        "image.imagemagick" => state(
            crate::magick::Magick::discover().is_ok(),
            "ImageMagick 7 isn't installed",
        ),
        "image.model.background-removal" => state(
            !provider::discover_background_removal_models().is_empty(),
            "the background removal model isn't installed",
        ),
        "image.model.upscale" => state(
            !provider::discover_upscale_models().is_empty(),
            "the upscaling model isn't installed",
        ),
        "web.yt-dlp" | "media.ytdlp" => state(
            compatible(provider::discover_ytdlp()),
            "yt-dlp isn't installed",
        ),
        "network.curl" => state(
            compatible(provider::discover_curl()),
            "curl isn't installed",
        ),
        _ => return None,
    })
}

/// Probes every provider Box knows how to check. Call it off the UI thread
/// and cache the result.
pub fn probe_providers() -> ProviderCache {
    let mut cache = ProviderCache::new();
    for id in PROBED {
        if let Some(s) = probe_one(id) {
            cache.insert((*id).to_string(), s);
        }
    }
    cache
}

/// Probes only the providers `tool` needs (one-shot mode has no cache).
fn probe_for(runtime: &Arcade, tool: &ToolManifest) -> ProviderCache {
    let mut cache: ProviderCache = tool
        .providers
        .iter()
        .filter(|id| !builtin_provider(id))
        .filter_map(|id| probe_one(id).map(|s| (id.clone(), s)))
        .collect();
    cache.extend(peer_provider_cache(runtime));
    cache
}

pub fn peer_provider_cache(runtime: &Arcade) -> ProviderCache {
    [
        ("ocr.lens", "lens.recognize"),
        ("screen.select.lens", "lens.capture"),
    ]
    .into_iter()
    .map(|(id, action)| {
        (
            id.into(),
            state(
                consumer::peer_action(runtime, ids::LENS, action).is_some(),
                "Arcade Lens isn't available",
            ),
        )
    })
    .collect()
}

pub fn refresh_peer_providers(runtime: &Arcade) -> bool {
    let mut cache = load_provider_cache(runtime).unwrap_or_default();
    cache.extend(peer_provider_cache(runtime));
    save_provider_cache(runtime, &cache)
}

pub fn load_provider_cache(runtime: &Arcade) -> Option<ProviderCache> {
    runtime
        .storage()
        .setting(SETTING_PROVIDER_CACHE)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
}

/// Saves `cache`; returns whether it differs from the saved one.
pub fn save_provider_cache(runtime: &Arcade, cache: &ProviderCache) -> bool {
    if load_provider_cache(runtime).as_ref() == Some(cache) {
        return false;
    }
    let _ = runtime.storage().set_setting(
        SETTING_PROVIDER_CACHE,
        &serde_json::to_string(cache).unwrap_or_default(),
    );
    true
}

/// Why `tool` can't run here, from the cache. Providers Box can't check are
/// treated as missing, so a peer never shows an entry that would fail.
fn missing_provider(tool: &ToolManifest, cache: Option<&ProviderCache>) -> Option<String> {
    for id in &tool.providers {
        if builtin_provider(id) {
            continue;
        }
        if id == "ocr.tesseract"
            && cache
                .and_then(|c| c.get("ocr.lens"))
                .is_some_and(|s| s.available)
        {
            continue;
        }
        match cache.and_then(|c| c.get(id)) {
            Some(s) if s.available => {}
            Some(s) => {
                return Some(
                    s.reason
                        .clone()
                        .unwrap_or_else(|| format!("{id} is missing")),
                );
            }
            None if cache.is_none() => {
                return Some("Arcade Box is still checking its engines".into());
            }
            None => return Some(format!("Arcade Box can't use {id} through other apps")),
        }
    }
    None
}

// ---- Types -------------------------------------------------------------------

/// The Link accept patterns for one of Box's input types. Box's own
/// screen and clipboard inputs aren't offered to peers (Lens and Clipboard
/// own those verbs).
fn link_accepts_one(box_type: &str) -> Vec<&'static str> {
    let array = box_type.ends_with("[]");
    let base = box_type.trim_end_matches("[]");
    let single: &[&'static str] = match base {
        "text/plain" | "text/markdown" | "network/host" | "network/ip" | "rows/csv" => {
            &["text/plain"]
        }
        "text/url" | "network/url" => &["text/url"],
        "folder/reference" => &["folder/reference"],
        "file/image" => &["file/image"],
        "file/video" => &["file/video"],
        "file/audio" => &["file/audio"],
        "file/pdf" => &["file/pdf"],
        "file/document" => &["file/document"],
        "file/spreadsheet" | "file/csv" | "file/tsv" => &["file/spreadsheet"],
        "file/json" => &["file/code"],
        "file/media" => &["file/video", "file/audio"],
        "file/any" | "file/subtitle" => &["file/any"],
        s if s.starts_with("structured/") => return vec![leak_structured(s, array)],
        _ => &[],
    };
    single
        .iter()
        .map(|t| if array { array_of(t) } else { *t })
        .collect()
}

fn array_of(t: &'static str) -> &'static str {
    match t {
        "file/image" => "file/image[]",
        "file/video" => "file/video[]",
        "file/audio" => "file/audio[]",
        "file/pdf" => "file/pdf[]",
        "file/document" => "file/document[]",
        "file/spreadsheet" => "file/spreadsheet[]",
        "file/code" => "file/code[]",
        "file/any" => "file/any[]",
        // Text and folders travel one at a time.
        other => other,
    }
}

fn leak_structured(s: &str, _array: bool) -> &'static str {
    match s {
        "structured/json" => "structured/json",
        "structured/yaml" => "structured/yaml",
        "structured/toml" => "structured/toml",
        "structured/xml" => "structured/xml",
        "structured/datetime" => "structured/datetime",
        "structured/quantity" => "structured/quantity",
        "structured/currency-amount" => "structured/currency-amount",
        "structured/time-zones" => "structured/time-zones",
        "structured/qr-payload" => "structured/qr-payload",
        "structured/regex-query" => "structured/regex-query",
        _ => "structured/*",
    }
}

/// The Link accept patterns for a tool.
pub fn link_accepts(tool: &ToolManifest) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in &tool.inputs {
        for a in link_accepts_one(t) {
            if !out.iter().any(|x| x == a) {
                out.push(a.to_string());
            }
        }
    }
    out
}

/// The Link type of one of Box's output types.
fn link_type_of_output(box_type: &str) -> String {
    let base = box_type.trim_end_matches("[]");
    match base {
        "text/markdown" | "rows/csv" | "network/host" | "network/ip" => "text/plain".into(),
        "network/url" => "text/url".into(),
        "file/csv" | "file/tsv" => "file/spreadsheet".into(),
        "file/json" => "file/code".into(),
        "file/media" | "file/subtitle" | "file/octet-stream" => "file/any".into(),
        other => other.to_string(),
    }
}

fn effects(tool: &ToolManifest) -> Vec<String> {
    let mut e = Vec::new();
    if tool
        .permissions
        .pointer("/filesystem/write")
        .and_then(Value::as_str)
        == Some("user-selected")
        || tool.outputs.iter().any(|o| o.starts_with("file/"))
    {
        e.push("writes-files".to_string());
    }
    if matches!(
        tool.permissions.pointer("/network").and_then(Value::as_str),
        Some("required" | "optional")
    ) {
        e.push("network".to_string());
    }
    if tool.privacy_class == PrivacyClass::Cloud {
        e.push("uploads-content".to_string());
    }
    e
}

fn privacy(tool: &ToolManifest) -> &'static str {
    match tool.privacy_class {
        PrivacyClass::Local => "local",
        PrivacyClass::Network => "network",
        PrivacyClass::Cloud => "cloud",
    }
}

fn platforms(tool: &ToolManifest) -> Vec<String> {
    let usable = |key: &str| {
        tool.platforms
            .get(key)
            .is_none_or(|s| !matches!(s.as_str(), "planned" | "unsupported"))
    };
    let mut p = Vec::new();
    if usable("x11") || usable("wayland") {
        p.push("linux".to_string());
    }
    if usable("windows") {
        p.push("windows".to_string());
    }
    if usable("macos") {
        p.push("macos".to_string());
    }
    p
}

fn verb(tool: &ToolManifest) -> String {
    tool.id
        .rsplit('.')
        .next()
        .and_then(|s| s.split('-').next())
        .unwrap_or("run")
        .to_string()
}

/// Every action Box offers peers, from the catalog and the provider cache.
pub fn actions(tools: &[ToolManifest], cache: Option<&ProviderCache>) -> Vec<Action> {
    let mut out = Vec::new();
    for tool in tools {
        if tool.status != ImplementationStatus::Implemented
            || tool.id.starts_with("arcade.pipeline.")
            || tool.execution.get("runtime").and_then(Value::as_str) == Some("wasm")
        {
            continue;
        }
        let accepts = link_accepts(tool);
        // Tools whose only inputs are Box's screen or clipboard sources stay in Box.
        if accepts.is_empty() && !tool.inputs.is_empty() {
            continue;
        }
        let mut base = Action::new(&format!("box:{}", tool.id), &tool.name, &verb(tool));
        base.accepts = accepts;
        base.produces = tool
            .outputs
            .iter()
            .map(|o| link_type_of_output(o))
            .collect();
        base.effects = effects(tool);
        base.privacy = privacy(tool).into();
        base.platforms = platforms(tool);
        base.group = Some(tool.category.clone());
        if let Some(reason) = missing_provider(tool, cache) {
            base.available = false;
            base.reason = Some(reason);
        }
        let featured = tool
            .link
            .as_ref()
            .map(|l| l.featured_for.clone())
            .unwrap_or_default();
        for preset in &tool.presets {
            let mut a = base.clone();
            a.id = format!("box:{}#{}", tool.id, preset.id);
            a.title = preset.name.clone();
            a.preset = Some(preset.id.clone());
            a.featured_for = featured.clone();
            out.push(a);
        }
        out.push(base);
    }
    out.push(open_action());
    out
}

/// "More in Arcade Box…": the Island, pre-filled with the input.
pub fn open_action() -> Action {
    Action::new("box.open", "More in Arcade Box…", "open")
        .accepts(&[
            "file/*",
            "file/*[]",
            "folder/reference",
            "text/*",
            "structured/*",
        ])
        .effects(&["opens-ui"])
        .interactive(true)
}

/// SPEC §5.4: a saved pipeline reference, independent of the node runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineOffer {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub accepts: Vec<String>,
    pub produces: Vec<String>,
    pub effects: Vec<String>,
    pub interactive: bool,
}

fn pipeline_offer(
    runtime: &Arcade,
    pipeline: &Pipeline,
    cache: Option<&ProviderCache>,
) -> Result<PipelineOffer, LinkError> {
    let order = pipeline
        .validate(runtime)
        .map_err(|e| LinkError::unavailable(e.to_string()))?;
    let first = order
        .first()
        .ok_or_else(|| LinkError::unavailable("the pipeline has no stages"))?;
    let tools = runtime.list_tools();
    let first_tool = tools.iter().find(|t| t.id == first.tool_id).unwrap();
    let mut offer = PipelineOffer {
        id: pipeline.id.clone(),
        name: pipeline.name.clone(),
        version: pipeline.version,
        accepts: if first
            .inputs
            .iter()
            .any(|i| matches!(i, InputSource::External { .. }))
        {
            link_accepts(first_tool)
        } else {
            Vec::new()
        },
        produces: Vec::new(),
        effects: Vec::new(),
        interactive: false,
    };
    for node in order {
        let tool = tools.iter().find(|t| t.id == node.tool_id).unwrap();
        if tool.status != ImplementationStatus::Implemented
            || tool.execution.get("runtime").and_then(Value::as_str) == Some("wasm")
            || tool.id.starts_with("arcade.pipeline.")
            || (!tool.inputs.is_empty() && link_accepts(tool).is_empty())
        {
            return Err(LinkError::unavailable(format!(
                "{} can't run through other apps",
                tool.name
            )));
        }
        if let Some(reason) = missing_provider(tool, cache) {
            return Err(LinkError::unavailable(reason));
        }
        for effect in effects(tool) {
            if !offer.effects.contains(&effect) {
                offer.effects.push(effect);
            }
        }
        if pipeline.output_nodes.contains(&node.id) {
            for output in &tool.outputs {
                let kind = link_type_of_output(output);
                if !offer.produces.contains(&kind) {
                    offer.produces.push(kind);
                }
            }
        }
    }
    Ok(offer)
}

/// Only runnable pipelines are offered; callers never get a broken entry.
pub fn pipelines(
    runtime: &Arcade,
    cache: Option<&ProviderCache>,
) -> Result<Vec<PipelineOffer>, LinkError> {
    Ok(runtime
        .list_pipelines()
        .map_err(|e| LinkError::internal(e.to_string()))?
        .iter()
        .filter_map(|p| pipeline_offer(runtime, p, cache).ok())
        .collect())
}

pub fn pipeline_actions(runtime: &Arcade, cache: Option<&ProviderCache>) -> Vec<Action> {
    let offers = pipelines(runtime, cache).unwrap_or_default();
    let list =
        Action::new("box.pipelines", "Saved pipelines", "list").produces(&["structured/pipelines"]);
    let mut run = Action::new("box.pipeline.run", "Run a saved pipeline", "run");
    for offer in &offers {
        for (target, values) in [
            (&mut run.accepts, &offer.accepts),
            (&mut run.produces, &offer.produces),
            (&mut run.effects, &offer.effects),
        ] {
            for value in values {
                if !target.contains(value) {
                    target.push(value.clone());
                }
            }
        }
        run.interactive |= offer.interactive;
    }
    if offers.is_empty() {
        run.available = false;
        run.reason = Some("No saved pipelines are available".into());
    }
    vec![list, run]
}

/// Resolve the stable pipeline action to its existing saved-tool executor.
pub fn resolve_pipeline<'a>(
    runtime: &Arcade,
    tools: &'a [ToolManifest],
    request: &InvokeRequest,
    cache: Option<&ProviderCache>,
) -> Result<&'a ToolManifest, LinkError> {
    let id = request
        .options
        .get("pipeline")
        .and_then(Value::as_str)
        .ok_or_else(|| LinkError::unsupported("Choose a pipeline with options.pipeline"))?;
    let pipeline = runtime
        .list_pipelines()
        .map_err(|e| LinkError::internal(e.to_string()))?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| LinkError::unavailable(format!("pipeline {id} does not exist")))?;
    pipeline_offer(runtime, &pipeline, cache)?;
    tools
        .iter()
        .find(|t| t.id == format!("arcade.pipeline.{id}"))
        .ok_or_else(|| LinkError::unavailable(format!("pipeline {id} does not exist")))
}

pub fn pipelines_result(runtime: &Arcade) -> Result<InvokeResult, LinkError> {
    let offers = pipelines(runtime, load_provider_cache(runtime).as_ref())?;
    Ok(InvokeResult {
        outputs: vec![Content::structured("pipelines", json!(offers))],
        ..Default::default()
    })
}

/// Box's manifest. `executable` is the desktop app, which also serves
/// one-shot requests (`--arcade-invoke`) without starting its UI.
pub fn manifest(
    settings: &LinkSettings,
    executable: &str,
    version: &str,
    shortcut: Option<&str>,
    tools: &[ToolManifest],
    cache: Option<&ProviderCache>,
) -> Manifest {
    let mut m = Manifest::new(ids::BOX, version, executable);
    m.launch.background = vec!["--background".into()];
    m.launch.invoke = Some(vec![arcade_link::oneshot::FLAG.into()]);
    if let Some(s) = shortcut {
        m.shortcuts.push(arcade_link::manifest::Shortcut {
            id: "island".into(),
            accelerator: s.into(),
        });
    }
    m.settings.link_enabled = settings.enabled;
    m.actions = if settings.enabled {
        actions(tools, cache)
    } else {
        Vec::new()
    };
    m
}

// ---- Running requests ----------------------------------------------------------

/// The tool and preset an action ID names (`box:<tool>` + optional preset).
pub fn resolve_action<'a>(
    tools: &'a [ToolManifest],
    request: &InvokeRequest,
) -> Result<(&'a ToolManifest, Map<String, Value>), LinkError> {
    let id = request
        .action
        .strip_prefix("box:")
        .unwrap_or(&request.action);
    let (tool_id, inline_preset) = match id.split_once('#') {
        Some((t, p)) => (t, Some(p)),
        None => (id, None),
    };
    let tool = tools
        .iter()
        .find(|t| t.id == tool_id && t.status == ImplementationStatus::Implemented)
        .ok_or_else(|| LinkError::unavailable(format!("Arcade Box has no tool {tool_id}")))?;
    let mut options = Map::new();
    if let Some(p) = request.preset.as_deref().or(inline_preset) {
        let preset = tool
            .preset(p)
            .ok_or_else(|| LinkError::unavailable(format!("{} has no preset {p}", tool.name)))?;
        options.extend(preset.options.clone());
    }
    // Callers may adjust options the tool's form exposes, nothing else.
    if let Value::Object(extra) = &request.options {
        let known: Vec<&str> = tool
            .ui
            .as_ref()
            .map(|ui| ui.controls.iter().map(|c| c.key.as_str()).collect())
            .unwrap_or_default();
        for (k, v) in extra {
            if known.contains(&k.as_str()) {
                options.insert(k.clone(), v.clone());
            }
        }
    }
    Ok((tool, options))
}

/// A tool request built from a Link request, with the delegated grants it
/// holds. Call [`Prepared::release`] when the job ends.
pub struct Prepared {
    pub request: ToolRequest,
    file_grants: Vec<String>,
    folder_grants: Vec<String>,
    /// Box-owned copies of handoff inputs, removed when the job ends.
    copies: Vec<std::path::PathBuf>,
}

impl Prepared {
    /// Ends the delegated grants (the selection was for this job only).
    pub fn release(&self, runtime: &Arcade) {
        for token in &self.file_grants {
            runtime.grants().revoke(token);
        }
        for token in &self.folder_grants {
            runtime.grants().revoke_input_directory(token);
        }
        for copy in &self.copies {
            let _ = std::fs::remove_file(copy);
        }
    }
}

fn text_mime(tool: &ToolManifest, link_type: &str) -> String {
    let declared: Vec<&str> = tool
        .inputs
        .iter()
        .map(|t| t.trim_end_matches("[]"))
        .collect();
    let wanted = if link_type.starts_with("structured/") && declared.contains(&link_type) {
        link_type.to_string()
    } else if link_type == "text/url" {
        declared
            .iter()
            .find(|t| matches!(**t, "text/url" | "network/url"))
            .map(|t| t.to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    if !wanted.is_empty() {
        return wanted;
    }
    declared
        .iter()
        .find(|t| {
            ["text/", "network/", "structured/", "rows/"]
                .iter()
                .any(|p| t.starts_with(p))
        })
        .map(|t| t.to_string())
        .unwrap_or_else(|| "text/plain".into())
}

/// Converts a Link request into a tool request. File and folder paths become
/// delegated, job-scoped grants; text arrives inline or via a handoff file.
pub fn prepare(
    runtime: &Arcade,
    tool: &ToolManifest,
    options: Map<String, Value>,
    inputs: &[Content],
) -> Result<Prepared, LinkError> {
    let accepts = link_accepts(tool);
    let mut prepared = Prepared {
        request: ToolRequest {
            tool_id: tool.id.clone(),
            inputs: Vec::new(),
            options: Value::Object(options),
        },
        file_grants: Vec::new(),
        folder_grants: Vec::new(),
        copies: Vec::new(),
    };
    let handoff_root = arcade_link::Locations::discover().handoff;
    for input in inputs {
        if !arcade_link::content::accepts_content(&accepts, input) {
            prepared.release(runtime);
            return Err(LinkError::unsupported(format!(
                "{} doesn't take {}",
                tool.name, input.kind
            )));
        }
        let fail = |prepared: &Prepared, e: String| {
            prepared.release(runtime);
            LinkError::unsupported(e)
        };
        match arcade_link::content::family(&input.kind) {
            "file" => {
                for path in input.all_paths() {
                    // Handoff files belong to the caller, who deletes their
                    // folder when the job ends; tools that write beside their
                    // input must not write there. Work on a Box-owned copy.
                    let source = std::path::Path::new(path);
                    let path_buf;
                    let path = if source.starts_with(&handoff_root) {
                        match copy_handoff_input(runtime, source) {
                            Ok(copy) => {
                                prepared.copies.push(copy.clone());
                                path_buf = copy;
                                path_buf.as_path()
                            }
                            Err(e) => return Err(fail(&prepared, format!("{path}: {e}"))),
                        }
                    } else {
                        source
                    };
                    match runtime.grants().grant(path) {
                        Ok(selected) => {
                            prepared.file_grants.push(selected.token.clone());
                            prepared.request.inputs.push(selected.as_tool_value());
                        }
                        Err(e) => {
                            return Err(fail(&prepared, format!("{}: {e}", path.display())));
                        }
                    }
                }
            }
            "folder" => {
                let path = input.path.as_deref().unwrap_or_default();
                match runtime
                    .grants()
                    .grant_input_directory(std::path::Path::new(path))
                {
                    Ok(folder) => {
                        prepared.folder_grants.push(folder.token.clone());
                        prepared.request.inputs.push(ToolValue {
                            kind: ValueKind::Artifact,
                            value: folder.token,
                            mime: "folder/reference".into(),
                        });
                    }
                    Err(e) => return Err(fail(&prepared, format!("{path}: {e}"))),
                }
            }
            _ => {
                let text = match &input.data {
                    Some(data) if input.text.is_none() => data.to_string(),
                    _ => arcade_link::handoff::read_text(input)
                        .map_err(|e| fail(&prepared, e.to_string()))?,
                };
                prepared
                    .request
                    .inputs
                    .push(ToolValue::text(text, text_mime(tool, &input.kind)));
            }
        }
    }
    Ok(prepared)
}

/// Copies a handoff file into `<artifacts>/link-inputs/<random>/<name>`.
fn copy_handoff_input(
    runtime: &Arcade,
    source: &std::path::Path,
) -> std::io::Result<std::path::PathBuf> {
    let dir = runtime
        .artifact_staging_root()
        .join("link-inputs")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir)?;
    let name = source
        .file_name()
        .ok_or_else(|| std::io::Error::other("handoff file has no name"))?;
    let copy = dir.join(name);
    std::fs::copy(source, &copy)?;
    Ok(copy)
}

/// The Link view of a finished tool run.
pub fn result_to_link(runtime: &Arcade, result: &ToolResult) -> Result<InvokeResult, LinkError> {
    if result.status == ResultStatus::Error {
        let message = result
            .message
            .clone()
            .unwrap_or_else(|| "the tool failed".into());
        return Err(
            LinkError::new(arcade_link::ErrorCode::Internal, message.clone()).with_reason(message),
        );
    }
    let mut outputs = Vec::new();
    for output in &result.outputs {
        match output.kind {
            ValueKind::Artifact | ValueKind::File => {
                let path = if output.kind == ValueKind::Artifact {
                    runtime
                        .grants()
                        .resolve(&output.value)
                        .map_err(|e| LinkError::internal(e.to_string()))?
                } else {
                    output.value.clone().into()
                };
                let mut c = Content::file(&path);
                c.owner = Some(ids::BOX.into());
                outputs.push(c);
            }
            ValueKind::Url => outputs.push(Content::url(&output.value)),
            ValueKind::Text => {
                let kind = link_type_of_output(&output.mime);
                let mut c = Content::text(&kind, &output.value);
                if kind.starts_with("structured/") {
                    c.data = serde_json::from_str(&output.value).ok();
                }
                outputs.push(c);
            }
        }
    }
    let data = (!result.metadata.is_empty()).then(|| json!(result.metadata));
    Ok(InvokeResult {
        outputs,
        message: result.message.clone(),
        data,
    })
}

/// Runs a Link request synchronously (one-shot mode and short resident calls).
pub fn run_blocking(
    runtime: &Arcade,
    request: &InvokeRequest,
    cancelled: &AtomicBool,
) -> Result<InvokeResult, LinkError> {
    if !LinkSettings::load(runtime).enabled {
        return Err(LinkError::denied(arcade_link::error::reason::DISABLED));
    }
    let tools = runtime.list_tools();
    if request.action == "box.pipelines" {
        return pipelines_result(runtime);
    }
    let (tool, options) = if request.action == "box.pipeline.run" {
        let mut cache = load_provider_cache(runtime).unwrap_or_default();
        let id = request
            .options
            .get("pipeline")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(p) = runtime
            .list_pipelines()
            .map_err(|e| LinkError::internal(e.to_string()))?
            .iter()
            .find(|p| p.id == id)
        {
            for node in &p.nodes {
                if let Some(tool) = tools.iter().find(|t| t.id == node.tool_id) {
                    cache.extend(probe_for(runtime, tool));
                }
            }
        }
        (
            resolve_pipeline(runtime, &tools, request, Some(&cache))?,
            Map::new(),
        )
    } else {
        resolve_action(&tools, request)?
    };
    if let Some(reason) = missing_provider(tool, Some(&probe_for(runtime, tool))) {
        return Err(LinkError::unavailable(reason));
    }
    let prepared = prepare(runtime, tool, options, &request.inputs)?;
    let outcome = runtime.run_tool_with_cancel(prepared.request.clone(), cancelled);
    prepared.release(runtime);
    let result =
        outcome.map_err(|e| LinkError::internal(e.to_string()).with_reason(e.to_string()))?;
    result_to_link(runtime, &result)
}

/// One-shot mode (`--arcade-invoke`): runs tools without any UI.
pub struct OneshotHandler {
    pub runtime: Arc<Arcade>,
}

impl Handler for OneshotHandler {
    fn describe(&self) -> Vec<Action> {
        if !LinkSettings::load(&self.runtime).enabled {
            return Vec::new();
        }
        let mut all = actions(
            &self.runtime.list_tools(),
            load_provider_cache(&self.runtime).as_ref(),
        );
        all.extend(pipeline_actions(
            &self.runtime,
            load_provider_cache(&self.runtime).as_ref(),
        ));
        all
    }

    fn invoke(&self, request: InvokeRequest, ctx: &InvokeContext) -> Result<Reply, LinkError> {
        if request.action == "box.open" {
            return Err(LinkError::unavailable("Arcade Box isn't running"));
        }
        let job = ctx.start_job();
        let ticket = job.ticket();
        job.progress(None, "Running in Arcade Box");
        job.finish(run_blocking(
            &self.runtime,
            &request,
            &AtomicBool::new(false),
        ));
        Ok(Reply::Job(ticket))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> (tempfile::TempDir, Arcade) {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arcade::open(&dir.path().join("arcade.sqlite3")).unwrap();
        (dir, runtime)
    }

    #[test]
    fn presets_become_actions_and_screen_tools_stay_in_box() {
        let (_dir, runtime) = runtime();
        let tools = runtime.list_tools();
        let all = actions(&tools, Some(&ProviderCache::new()));
        let webp = all
            .iter()
            .find(|a| a.id == "box:arcade.image.convert#webp")
            .unwrap();
        assert_eq!(webp.title, "Convert to WebP");
        assert_eq!(webp.preset.as_deref(), Some("webp"));
        assert_eq!(webp.featured_for, vec!["file/image".to_string()]);
        assert!(webp.accepts.contains(&"file/image".to_string()));
        // vips isn't in the (empty) cache: shown as unavailable, with a reason.
        assert!(!webp.available && webp.reason.is_some());
        assert!(all.iter().any(|a| a.id == "box.open" && a.interactive));
        assert!(!all.iter().any(|a| a.id.starts_with("box:arcade.screen.")));
        let json = all
            .iter()
            .find(|a| a.id == "box:arcade.text.structured#format-json")
            .unwrap();
        assert!(json.available, "built-in tools need no provider");
    }

    #[test]
    fn text_presets_run_and_paths_are_granted_for_the_job_only() {
        let (dir, runtime) = runtime();
        let request = InvokeRequest::new("box:arcade.text.structured", "arcade.test")
            .preset(Some("format-json"))
            .input(Content::plain("{\"a\":1}"));
        let r = run_blocking(&runtime, &request, &AtomicBool::new(false)).unwrap();
        assert!(
            r.outputs[0]
                .text
                .as_deref()
                .unwrap()
                .contains("\n  \"a\": 1")
        );

        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "x").unwrap();
        let tools = runtime.list_tools();
        let tool = tools
            .iter()
            .find(|t| t.id == "arcade.developer.hash")
            .unwrap();
        if link_accepts(tool).iter().any(|a| a.starts_with("file/")) {
            let prepared = prepare(&runtime, tool, Map::new(), &[Content::file(&file)]).unwrap();
            let token = prepared.request.inputs[0].value.clone();
            assert!(runtime.grants().resolve(&token).is_ok());
            prepared.release(&runtime);
            assert!(
                runtime.grants().resolve(&token).is_err(),
                "the grant ends with the job"
            );
        }
        let wrong = InvokeRequest::new("box:arcade.image.convert", "arcade.test")
            .preset(Some("webp"))
            .input(Content::plain("not an image"));
        let tools = runtime.list_tools();
        let (tool, options) = resolve_action(&tools, &wrong).unwrap();
        assert_eq!(
            prepare(&runtime, tool, options, &wrong.inputs)
                .err()
                .unwrap()
                .code,
            arcade_link::ErrorCode::UnsupportedInput
        );
    }

    #[test]
    fn unknown_options_are_dropped() {
        let (_dir, runtime) = runtime();
        let tools = runtime.list_tools();
        let req = InvokeRequest::new("box:arcade.image.convert#png", "t")
            .options(json!({"quality": 50, "evil": "x"}));
        let (_, options) = resolve_action(&tools, &req).unwrap();
        assert_eq!(options.get("format"), Some(&json!("png")));
        assert_eq!(options.get("quality"), Some(&json!(50)));
        assert!(!options.contains_key("evil"));
    }

    #[test]
    fn delegated_image_grants_are_revoked_on_success_and_rejected_input() {
        let (dir, runtime) = runtime();
        let path = dir.path().join("outside.png");
        image::RgbImage::new(2, 2).save(&path).unwrap();
        let tools = runtime.list_tools();
        let tool = tools
            .iter()
            .find(|t| t.id == "arcade.image.convert")
            .unwrap();
        let prepared = prepare(&runtime, tool, Map::new(), &[Content::file(&path)]).unwrap();
        let token = prepared.request.inputs[0].value.clone();
        assert_eq!(runtime.grants().resolve(&token).unwrap(), path);
        prepared.release(&runtime);
        assert!(runtime.grants().resolve(&token).is_err());
        assert!(
            prepare(
                &runtime,
                tool,
                Map::new(),
                &[Content::file(&path), Content::plain("wrong")]
            )
            .is_err()
        );
    }

    #[test]
    fn saved_pipeline_contract_runs_and_reports_node_effects() {
        let (_dir, runtime) = runtime();
        runtime.save_pipeline(serde_json::from_value(json!({
            "id": "upper-clean", "name": "Upper and clean", "version": 1,
            "nodes": [
                {"id":"upper", "toolId":"arcade.text.case", "inputs":[{"kind":"external", "index":0}], "options":{"mode":"upper"}},
                {"id":"clean", "toolId":"arcade.text.clean", "inputs":[{"kind":"node", "nodeId":"upper", "outputIndex":0}], "options":{"trim":true}}
            ], "outputNodes":["clean"]
        })).unwrap()).unwrap();
        let offer = pipelines(&runtime, Some(&ProviderCache::new()))
            .unwrap()
            .remove(0);
        assert_eq!(
            json!(offer),
            json!({"id":"upper-clean", "name":"Upper and clean", "version":1,
            "accepts":["text/plain"], "produces":["text/plain"], "effects":[], "interactive":false})
        );
        let list = run_blocking(
            &runtime,
            &InvokeRequest::new("box.pipelines", "t"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(list.outputs[0].kind, "structured/pipelines");
        assert!(list.outputs[0].data.as_ref().unwrap().is_array());
        let request = InvokeRequest::new("box.pipeline.run", "t")
            .options(json!({"pipeline":"upper-clean"}))
            .input(Content::plain("  hello  "));
        let result = run_blocking(&runtime, &request, &AtomicBool::new(false)).unwrap();
        assert_eq!(result.outputs[0].text.as_deref(), Some("HELLO"));
        assert!(run_blocking(&runtime, &request, &AtomicBool::new(true)).is_err());
        assert!(
            run_blocking(
                &runtime,
                &InvokeRequest::new("box.pipeline.run", "t").preset(Some("upper-clean")),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(
            run_blocking(
                &runtime,
                &InvokeRequest::new("box.pipeline.run", "t").options(json!({"pipeline":"missing"})),
                &AtomicBool::new(false)
            )
            .is_err()
        );

        runtime.save_pipeline(serde_json::from_value(json!({
            "id":"image-webp", "name":"WebP image", "version":1,
            "nodes":[{"id":"convert", "toolId":"arcade.image.convert", "inputs":[{"kind":"external", "index":0}], "options":{"format":"webp"}}],
            "outputNodes":["convert"]
        })).unwrap()).unwrap();
        assert_eq!(
            pipelines(&runtime, Some(&ProviderCache::new()))
                .unwrap()
                .len(),
            1,
            "missing providers aren't offered"
        );
        let mut cache = ProviderCache::new();
        cache.insert("image.vips".into(), state(true, ""));
        let offer = pipelines(&runtime, Some(&cache))
            .unwrap()
            .into_iter()
            .find(|p| p.id == "image-webp")
            .unwrap();
        assert_eq!(offer.effects, vec!["writes-files"]);
        let all = pipeline_actions(&runtime, Some(&cache));
        assert!(all[1].effects.contains(&"writes-files".into()));
        runtime.delete_pipeline("upper-clean").unwrap();
        runtime.delete_pipeline("image-webp").unwrap();
        assert!(!pipeline_actions(&runtime, Some(&cache))[1].available);
    }
}
