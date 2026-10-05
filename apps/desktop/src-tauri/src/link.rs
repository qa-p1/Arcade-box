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

use arcade_core::Arcade;
use arcade_core::jobs::{JobManager, JobSnapshot, JobStatus};
use arcade_core::link::{self as core_link, LinkSettings, Prepared, ProviderCache};
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
    manifest.actions.extend(core_link::pipeline_actions(
        runtime,
        core_link::load_provider_cache(runtime).as_ref(),
    ));
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
        serde_json::json!({ "jobs": self.app.state::<JobManager>().list().iter().filter(|j| !terminal(j.status)).count() })
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
    let cache: ProviderCache = core_link::probe_providers();
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
