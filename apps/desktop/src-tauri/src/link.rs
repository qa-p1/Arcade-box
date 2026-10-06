//! Arcade Link: Box's presence among the other Arcade apps.
//!
//! The resident desktop process writes Box's manifest and listens on its
//! endpoint from a background thread after setup, so startup never waits.
//! Tool requests run through the same job manager as the Island (they show
//! up in Background jobs); `box.open` opens the Island pre-filled.
//! With "Connect with other Arcade apps" off, the manifest has no actions
//! and nothing listens.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use arcade_contract::ToolResult;
use arcade_core::Arcade;
use arcade_core::jobs::{JobManager, JobSnapshot, JobStatus};
use arcade_core::link::{self as core_link, LinkSettings, Prepared, ProviderCache, consumer};
use arcade_link::server::{Handler, InvokeContext, Job, Reply};
use arcade_link::{Action, InvokeRequest, InvokeResult, LinkError, Locations, Manifest, Presence};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// The shortcut Box uses now, for clash warnings in other apps.
pub fn effective_shortcut(runtime: &Arcade) -> String {
    runtime
        .storage()
        .setting("global_shortcut")
        .ok()
        .flatten()
        .unwrap_or_else(|| default_shortcut().into())
}

fn default_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+Shift+Space"
    } else {
        "Ctrl+Alt+Space"
    }
}

/// The data directory the desktop app uses (Tauri's `app_data_dir`), for
/// code that runs before Tauri starts (`--arcade-manifest`, one-shot mode).
pub fn data_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.data_dir().join("dev.arcadebox.app"))
}

fn build(runtime: &Arcade) -> Manifest {
    let mut manifest = core_link::manifest(
        &LinkSettings::load(runtime),
        &arcade_link::manifest::current_executable(),
        env!("CARGO_PKG_VERSION"),
        Some(&effective_shortcut(runtime)),
        &runtime.list_tools(),
        core_link::load_provider_cache(runtime).as_ref(),
    );
    if manifest.settings.link_enabled {
        manifest.actions.extend(core_link::pipeline_actions(
            runtime,
            core_link::load_provider_cache(runtime).as_ref(),
        ));
    }
    manifest
}

/// `--arcade-manifest`: the manifest from the saved settings. Doesn't create
/// Box's database if it doesn't exist yet.
pub fn print_manifest() {
    let db = data_dir().map(|d| d.join("arcade.sqlite3"));
    let m = match db
        .filter(|p| p.is_file())
        .and_then(|p| Arcade::open(&p).ok())
    {
        Some(runtime) => build(&runtime),
        None => core_link::manifest(
            &LinkSettings::default(),
            &arcade_link::manifest::current_executable(),
            env!("CARGO_PKG_VERSION"),
            Some(default_shortcut()),
            &[],
            None,
        ),
    };
    println!("{}", m.to_json());
}

/// `--arcade-invoke`: one request on stdin, no UI, tray, shortcut or listener.
pub fn serve_oneshot() -> i32 {
    let Some(dir) = data_dir() else {
        return 1;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return 1;
    }
    match Arcade::open(&dir.join("arcade.sqlite3")) {
        Ok(runtime) => arcade_link::oneshot::serve(&core_link::OneshotHandler {
            runtime: Arc::new(runtime),
        }),
        Err(error) => {
            eprintln!("Arcade Box: {error}");
            1
        }
    }
}

/// The Island, opened from another app with its input attached.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkOpen {
    source: String,
    tool: Option<String>,
    files: Vec<arcade_core::grants::SelectedFile>,
    text: Option<String>,
    options: serde_json::Map<String, serde_json::Value>,
}

struct LinkJob {
    job: Job,
    prepared: Prepared,
}

/// Link requests in flight, by Box job ID. Updates that arrive before a job
/// is registered (a very fast tool) wait in `early`.
#[derive(Default)]
struct Jobs {
    running: HashMap<String, LinkJob>,
    early: VecDeque<JobSnapshot>,
}

fn jobs() -> &'static Mutex<Jobs> {
    static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

fn terminal(status: JobStatus) -> bool {
    matches!(
        status,
        JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled | JobStatus::Interrupted
    )
}

fn finish(app: &AppHandle, link_job: LinkJob, snapshot: &JobSnapshot) {
    let runtime = app.state::<Arc<Arcade>>().inner().clone();
    link_job.prepared.release(&runtime);
    let outcome: Result<InvokeResult, LinkError> = match snapshot.status {
        JobStatus::Succeeded => match &snapshot.result {
            Some(result) => core_link::result_to_link(&runtime, result),
            None => Err(LinkError::internal("the job finished without a result")),
        },
        JobStatus::Cancelled => Err(LinkError::cancelled()),
        _ => {
            let message = snapshot
                .result
                .as_ref()
                .and_then(|r| r.message.clone())
                .or_else(|| snapshot.message.clone())
                .unwrap_or_else(|| "the job failed".into());
            Err(LinkError::internal(message.clone()).with_reason(message))
        }
    };
    link_job.job.finish(outcome);
}

/// Called for every job update (from the job manager's update handler).
pub fn job_update(app: &AppHandle, snapshot: &JobSnapshot) {
    let done = {
        let mut state = jobs().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(link_job) = state.running.get(&snapshot.id) {
            if terminal(snapshot.status) {
                state.running.remove(&snapshot.id)
            } else {
                link_job.job.progress(
                    snapshot.progress.map(|p| p as f32),
                    snapshot.message.as_deref().unwrap_or("Working"),
                );
                None
            }
        } else {
            if terminal(snapshot.status) {
                state.early.push_back(snapshot.clone());
                while state.early.len() > 32 {
                    state.early.pop_front();
                }
            }
            None
        }
    };
    if let Some(link_job) = done {
        finish(app, link_job, snapshot);
    }
}

struct BoxHandler {
    app: AppHandle,
}

impl BoxHandler {
    fn runtime(&self) -> Arc<Arcade> {
        self.app.state::<Arc<Arcade>>().inner().clone()
    }

    fn open(&self, request: InvokeRequest, peer: &str) -> Result<Reply, LinkError> {
        let runtime = self.runtime();
        let mut open = LinkOpen {
            source: peer.to_string(),
            tool: request
                .options
                .get("tool")
                .and_then(serde_json::Value::as_str)
                .map(String::from),
            files: Vec::new(),
            text: None,
            options: request
                .options
                .get("options")
                .and_then(serde_json::Value::as_object)
                .cloned()
                .unwrap_or_default(),
        };
        for input in &request.inputs {
            match arcade_link::content::family(&input.kind) {
                // The user picked these in another app: the same as choosing
                // them in Box's file dialog.
                "file" => {
                    for path in input.all_paths() {
                        let selected = runtime
                            .grants()
                            .grant(std::path::Path::new(path))
                            .map_err(|e| LinkError::unsupported(format!("{path}: {e}")))?;
                        open.files.push(selected);
                    }
                }
                "folder" => {}
                _ => {
                    open.text = arcade_link::handoff::read_text(input)
                        .ok()
                        .or_else(|| input.data.as_ref().map(|d| d.to_string()))
                }
            }
        }
        if let Some(window) = self.app.get_webview_window("island") {
            crate::show_island(&window);
        }
        let _ = self.app.emit("arcade://link-open", open);
        Ok(Reply::Done(InvokeResult::message("Opened in Arcade Box")))
    }
}

impl Handler for BoxHandler {
    fn describe(&self) -> Vec<Action> {
        build(&self.runtime()).actions
    }

    fn invoke(&self, request: InvokeRequest, ctx: &InvokeContext) -> Result<Reply, LinkError> {
        if request.action == "box.open" {
            return self.open(request, &ctx.peer().id);
        }
        let runtime = self.runtime();
        if request.action == "box.pipelines" {
            return core_link::pipelines_result(&runtime).map(Reply::Done);
        }
        let tools = runtime.list_tools();
        let cache = core_link::load_provider_cache(&runtime);
        let (tool, options) = if request.action == "box.pipeline.run" {
            (
                core_link::resolve_pipeline(&runtime, &tools, &request, cache.as_ref())?,
                Default::default(),
            )
        } else {
            core_link::resolve_action(&tools, &request)?
        };
        if let Some(action) = core_link::actions(std::slice::from_ref(tool), cache.as_ref())
            .into_iter()
            .find(|a| !a.available)
        {
            return Err(LinkError::unavailable(action.reason.unwrap_or_default()));
        }
        let prepared = core_link::prepare(&runtime, tool, options, &request.inputs)?;
        let manager = self.app.state::<JobManager>();
        let snapshot = match manager.submit(prepared.request.clone()) {
            Ok(s) => s,
            Err(e) => {
                prepared.release(&runtime);
                return Err(match e {
                    arcade_core::jobs::JobError::QueueFull => LinkError::busy(),
                    other => LinkError::internal(other.to_string()),
                });
            }
        };
        let job = ctx.start_job();
        job.progress(None, &format!("Running {}", tool.name));
        let ticket = job.ticket();
        let app = self.app.clone();
        let box_id = snapshot.id.clone();
        job.on_cancel(move || {
            let _ = app.state::<JobManager>().cancel(&box_id);
        });
        let early = {
            let mut state = jobs().lock().unwrap_or_else(|e| e.into_inner());
            let early = state
                .early
                .iter()
                .position(|s| s.id == snapshot.id)
                .and_then(|i| state.early.remove(i));
            if early.is_none() {
                state
                    .running
                    .insert(snapshot.id.clone(), LinkJob { job, prepared });
                return Ok(Reply::Job(ticket));
            }
            (early, LinkJob { job, prepared })
        };
        if let (Some(snapshot), link_job) = early {
            finish(&self.app, link_job, &snapshot);
        }
        Ok(Reply::Job(ticket))
    }

    fn status(&self) -> serde_json::Value {
        let mode = if std::env::args().any(|arg| arg == "--background") {
            "background"
        } else {
            "foreground"
        };
        serde_json::json!({ "mode": mode, "jobs": self.app.state::<JobManager>().list().iter().filter(|j| !terminal(j.status)).count() })
    }

    fn activate(&self) -> Result<(), LinkError> {
        if let Some(window) = self.app.get_webview_window("island") {
            crate::show_island(&window);
        }
        Ok(())
    }

    fn quit(&self) -> Result<(), LinkError> {
        self.app.exit(0);
        Ok(())
    }
}

static PRESENCE: OnceLock<Mutex<Option<Arc<Presence>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<Presence>>> {
    PRESENCE.get_or_init(|| Mutex::new(None))
}

/// Re-checks providers and rewrites the manifest if availability changed.
fn reprobe(runtime: &Arcade) {
    let mut cache: ProviderCache = core_link::probe_providers();
    cache.extend(core_link::peer_provider_cache(runtime));
    if core_link::save_provider_cache(runtime, &cache) {
        let p = slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(p) = p {
            p.update(build(runtime));
        }
    }
}

/// Starts Box's presence on a background thread, then re-checks providers
/// (they may have been installed since the last run).
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("box-link".into())
        .spawn(move || {
            let Some(runtime) = app.try_state::<Arc<Arcade>>().map(|r| r.inner().clone()) else {
                return;
            };
            let p = Presence::start(
                Locations::discover(),
                build(&runtime),
                Arc::new(BoxHandler { app: app.clone() }),
            );
            if let Some(e) = p.last_error() {
                eprintln!("Arcade Link: {e}");
            }
            *slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(p));
            let watched_app = app.clone();
            let watched_runtime = runtime.clone();
            consumer::registry(&runtime).watch(move |_| {
                if core_link::refresh_peer_providers(&watched_runtime) {
                    if let Some(p) = slot().lock().unwrap_or_else(|e| e.into_inner()).clone() {
                        p.update(build(&watched_runtime));
                    }
                }
                let _ = watched_app.emit("arcade://link-changed", ());
            });
            let _ = app.emit("arcade://link-changed", ());
            reprobe(&runtime);
        });
}

/// Rewrites the manifest after a change (shortcut, Link settings).
pub fn refresh(app: &AppHandle) {
    let Some(runtime) = app.try_state::<Arc<Arcade>>().map(|r| r.inner().clone()) else {
        return;
    };
    let p = slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(p) = p {
        std::thread::spawn(move || p.update(build(&runtime)));
    }
}

/// After the Engines page probed providers: update the cache and manifest.
pub fn providers_checked(app: &AppHandle) {
    let Some(runtime) = app.try_state::<Arc<Arcade>>().map(|r| r.inner().clone()) else {
        return;
    };
    std::thread::spawn(move || reprobe(&runtime));
}

/// Stops listening and removes the endpoint file (the manifest stays).
pub fn stop() {
    if let Some(p) = slot().lock().unwrap_or_else(|e| e.into_inner()).take() {
        p.stop();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedApp {
    id: String,
    name: String,
    state: String,
    version: Option<String>,
    enabled: bool,
    pitch: String,
    endpoint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedApps {
    settings: LinkSettings,
    apps: Vec<ConnectedApp>,
    registry_path: String,
    endpoint_state: String,
    last_error: Option<String>,
}

#[tauri::command(async)]
pub fn connected_apps(runtime: tauri::State<'_, Arc<Arcade>>) -> ConnectedApps {
    let locations = Locations::discover();
    let registry = consumer::registry(&runtime);
    registry.refresh();
    let snapshot = registry.snapshot();
    let settings = LinkSettings::load(&runtime);
    let apps = arcade_link::ids::APPS
        .into_iter()
        .chain(std::iter::once(arcade_link::ids::TOOLS))
        .filter(|id| *id != arcade_link::ids::BOX)
        .map(|id| {
            let (state, version) =
                match arcade_link::client::app_state(&locations, &snapshot, id, &consumer::me()) {
                    arcade_link::AppState::Running { version } => ("Running", Some(version)),
                    arcade_link::AppState::Installed { version } => ("Installed", Some(version)),
                    arcade_link::AppState::NotInstalled => ("Not installed", None),
                };
            ConnectedApp {
                id: id.into(),
                name: arcade_link::manifest::app_name(id).into(),
                state: state.into(),
                version,
                enabled: !settings.disabled_peers.iter().any(|p| p == id),
                pitch: arcade_link::manifest::app_pitch(id).into(),
                endpoint: locations.endpoint(id).display().to_string(),
            }
        })
        .collect();
    let p = slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    ConnectedApps {
        settings: settings.clone(),
        apps,
        registry_path: locations.registry.display().to_string(),
        endpoint_state: if !settings.enabled {
            "Disabled"
        } else if p.as_ref().is_some_and(|p| p.last_error().is_none()) {
            "Listening"
        } else {
            "Unavailable"
        }
        .into(),
        last_error: p.and_then(|p| p.last_error()).map(|e| e.to_string()),
    }
}

#[tauri::command(async)]
pub fn set_link_settings(
    settings: LinkSettings,
    app: AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    if settings.disabled_peers.iter().any(|id| {
        (!arcade_link::ids::APPS.contains(&id.as_str()) && id != arcade_link::ids::TOOLS)
            || id == arcade_link::ids::BOX
    }) {
        return Err("Unknown Arcade app".into());
    }
    settings.save(&runtime)?;
    core_link::refresh_peer_providers(&runtime);
    if let Some(p) = slot().lock().unwrap_or_else(|e| e.into_inner()).clone() {
        p.update(build(&runtime));
    }
    let _ = app.emit("arcade://link-changed", ());
    Ok(())
}

#[tauri::command(async)]
pub fn get_connected_app(
    id: String,
    app: AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    if !arcade_link::ids::APPS.contains(&id.as_str()) && id != arcade_link::ids::TOOLS {
        return Err("Unknown Arcade app".into());
    }
    if consumer::peer_action(&runtime, arcade_link::ids::TOOLS, "tools.install").is_some() {
        consumer::invoke(
            &runtime,
            arcade_link::ids::TOOLS,
            InvokeRequest::new("tools.install", arcade_link::ids::BOX)
                .options(serde_json::json!({"app":id})),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .map_err(|e| e.user_message("Arcade Tools"))?;
        Ok(())
    } else {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(arcade_link::manifest::releases_url(&id), None::<&str>)
            .map_err(|e| e.to_string())
    }
}

#[tauri::command(async)]
pub fn result_link_actions(
    outputs: Vec<arcade_contract::ToolValue>,
    tool_id: String,
    preset: Option<String>,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<consumer::ResultOffer>, String> {
    let tool = runtime
        .list_tools()
        .into_iter()
        .find(|t| t.id == tool_id)
        .ok_or("Unknown tool")?;
    consumer::result_offers(&runtime, &outputs, &tool, preset.as_deref())
        .map_err(|e| e.user_message("Arcade Box"))
}

#[tauri::command(async)]
pub fn invoke_result_link_action(
    request_id: String,
    key: String,
    outputs: Vec<arcade_contract::ToolValue>,
    tool_id: String,
    preset: Option<String>,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<InvokeResult, String> {
    let tool = runtime
        .list_tools()
        .into_iter()
        .find(|t| t.id == tool_id)
        .ok_or("Unknown tool")?;
    let (.., app, request) =
        consumer::result_requests(&runtime, &outputs, &tool, preset.as_deref())
            .map_err(|e| e.user_message("Arcade Box"))?
            .into_iter()
            .find(|(k, _, _)| k == &key)
            .ok_or("Unknown result action")?;
    let active = OutboundRequest::begin(request_id)?;
    let result = consumer::invoke(&runtime, &app, request, &active.cancelled);
    result.map_err(|e| e.user_message(arcade_link::manifest::app_name(&app)))
}

fn outbound_requests() -> &'static Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>> {
    static REQUESTS: OnceLock<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>> =
        OnceLock::new();
    REQUESTS.get_or_init(Default::default)
}

#[tauri::command(async)]
pub fn cancel_result_link_action(request_id: String) {
    if let Some(cancelled) = outbound_requests()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&request_id)
    {
        cancelled.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// A cancellation flag shared by peer jobs and their frontend invocation.
pub struct OutboundRequest {
    id: String,
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl OutboundRequest {
    pub fn begin(id: String) -> Result<Self, String> {
        if id.is_empty() || id.len() > 80 {
            return Err("Invalid action request".into());
        }
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut requests = outbound_requests()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if requests.len() >= 32 || requests.contains_key(&id) {
            return Err("Another action is already using this request".into());
        }
        requests.insert(id.clone(), cancelled.clone());
        Ok(Self { id, cancelled })
    }
}

impl Drop for OutboundRequest {
    fn drop(&mut self) {
        outbound_requests()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlapState {
    lens_capture: bool,
    lens_actions: bool,
    clipboard_pick: bool,
}

#[tauri::command(async)]
pub fn overlap_state(runtime: tauri::State<'_, Arc<Arcade>>) -> OverlapState {
    OverlapState {
        lens_capture: consumer::peer_action(&runtime, arcade_link::ids::LENS, "lens.capture")
            .is_some(),
        lens_actions: consumer::peer_action(
            &runtime,
            arcade_link::ids::LENS,
            "lens.capture_and_act",
        )
        .is_some(),
        clipboard_pick: consumer::peer_action(
            &runtime,
            arcade_link::ids::CLIPBOARD,
            "clipboard.pick",
        )
        .is_some(),
    }
}

#[tauri::command(async)]
pub fn shortcut_owner(
    accelerator: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Option<String> {
    consumer::registry(&runtime).with(|r| r.shortcut_owner(arcade_link::ids::BOX, &accelerator))
}

#[tauri::command(async)]
pub fn pick_peer_clipboard(
    request_id: String,
    app: AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<ToolResult, String> {
    let request = OutboundRequest::begin(request_id)?;
    let window = app.get_webview_window("island");
    if let Some(window) = &window {
        let _ = window.hide();
    }
    let result = consumer::pick_clipboard(&runtime, &request.cancelled);
    if let Some(window) = window {
        let _ = window.show();
        let _ = window.set_focus();
    }
    result
}
