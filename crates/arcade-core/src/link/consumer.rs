//! Cached peer discovery and outbound requests. Call these from workers.

use std::sync::atomic::{AtomicBool, Ordering};

use arcade_contract::{ToolManifest, ToolValue};
use arcade_link::manifest::surface;
use arcade_link::{
    Action, Client, Content, ErrorCode, InvokeRequest, InvokeResult, LinkError, Locations,
    Manifest, PeerInfo, Registry, SharedRegistry, content, ids, offers,
};
use serde::Serialize;
use serde_json::json;

use super::{LinkSettings, link_type_of_output};
use crate::Arcade;

pub const DEVICE_LIMIT: u64 = 16 * 1024 * 1024;

pub fn me() -> PeerInfo {
    PeerInfo {
        id: ids::BOX.into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// Initialized by the desktop's background Link setup, or on a CLI job worker.
pub fn registry(runtime: &Arcade) -> &SharedRegistry {
    runtime
        .link_registry
        .get_or_init(|| SharedRegistry::load(&Locations::discover()))
}

pub fn peer_action(runtime: &Arcade, app: &str, action: &str) -> Option<(Manifest, Action)> {
    if !LinkSettings::load(runtime).uses(app) {
        return None;
    }
    registry(runtime).with(|r| {
        let manifest = r.get(app)?;
        if !manifest.settings.link_enabled {
            return None;
        }
        let action = manifest.action(action)?;
        (action.available && action.on_this_platform()).then(|| (manifest.clone(), action.clone()))
    })
}

pub fn check_request(
    settings: &LinkSettings,
    registry: &Registry,
    app: &str,
    request: &InvokeRequest,
) -> Result<(Manifest, Action), LinkError> {
    if !settings.uses(app) {
        return Err(LinkError::denied(arcade_link::error::reason::DISABLED));
    }
    let manifest = registry
        .get(app)
        .ok_or_else(|| LinkError::new(ErrorCode::NotInstalled, "peer is missing"))?;
    if !manifest.settings.link_enabled {
        return Err(LinkError::denied(arcade_link::error::reason::DISABLED));
    }
    let action = arcade_link::client::find_action(manifest, request)
        .ok_or_else(|| LinkError::unavailable("action is missing"))?;
    if !action.available || !action.on_this_platform() {
        return Err(LinkError::unavailable(
            action
                .reason
                .clone()
                .unwrap_or_else(|| "unsupported on this platform".into()),
        ));
    }
    if request.version.is_some_and(|v| v != action.version) {
        return Err(LinkError::new(
            ErrorCode::VersionMismatch,
            "action version changed",
        ));
    }
    if request
        .inputs
        .iter()
        .any(|c| !arcade_link::content::accepts_content(&action.accepts, c))
    {
        return Err(LinkError::unsupported("input is not accepted"));
    }
    let size = request
        .inputs
        .iter()
        .map(Content::byte_size)
        .fold(0u64, u64::saturating_add);
    let limit = if app == ids::CLIPBOARD && request.action == "clipboard.add" {
        Some(action.max_bytes.unwrap_or(DEVICE_LIMIT).min(DEVICE_LIMIT))
    } else {
        action.max_bytes
    };
    if let Some(limit) = limit {
        if size > limit {
            return Err(LinkError::too_large(limit));
        }
    }
    Ok((manifest.clone(), action.clone()))
}

pub fn invoke(
    runtime: &Arcade,
    app: &str,
    mut request: InvokeRequest,
    cancelled: &AtomicBool,
) -> Result<InvokeResult, LinkError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(LinkError::cancelled());
    }
    // Refresh on an explicit user request too, in case watching is unavailable.
    let registry = registry(runtime);
    registry.refresh();
    let (manifest, _) = check_request(
        &LinkSettings::load(runtime),
        &registry.snapshot(),
        app,
        &request,
    )?;
    let locations = Locations::discover();
    // Keep large text out of the wire; the guard owns its files until completion.
    let mut handoff = None;
    for input in &mut request.inputs {
        if input
            .text
            .as_ref()
            .is_some_and(|t| t.len() > arcade_link::content::INLINE_TEXT_LIMIT)
        {
            let transfer =
                handoff.get_or_insert_with(|| arcade_link::Handoff::create(&locations, ids::BOX));
            let transfer = transfer
                .as_ref()
                .map_err(|e| LinkError::internal(e.to_string()))?;
            *input = transfer
                .text(&input.kind, input.text.as_deref().unwrap())
                .map_err(|e| LinkError::internal(e.to_string()))?;
        }
    }
    let action = arcade_link::client::find_action(&manifest, &request)
        .ok_or_else(|| LinkError::unavailable("action is missing"))?;
    let client = Client::connect(&locations, app, &me());
    let mut client = match client {
        Ok(client) => client,
        Err(_) if !action.interactive && manifest.launch.invoke.is_some() => {
            return arcade_link::oneshot::run(
                &manifest.executable,
                manifest.launch.invoke.as_ref().unwrap(),
                &request,
                &mut |_| {},
                Some(cancelled),
            );
        }
        Err(_) => arcade_link::client::launch_and_connect(&locations, &manifest, &me())?,
    };
    invoke_resident(&mut client, &request, cancelled)
}

/// Wait only while this user-requested operation is active. Return after a
/// cancellation acknowledgement, even if a peer keeps its picker open. Box
/// can then release its private workspace instead of waiting indefinitely.
fn invoke_resident(
    client: &mut Client,
    request: &InvokeRequest,
    cancelled: &AtomicBool,
) -> Result<InvokeResult, LinkError> {
    use arcade_link::wire::{JobDone, method};
    use std::time::{Duration, Instant};
    let reply = client.call(
        method::INVOKE,
        serde_json::to_value(request).map_err(|e| LinkError::internal(e.to_string()))?,
    )?;
    let Some(job) = reply.get("job").and_then(serde_json::Value::as_str) else {
        return serde_json::from_value(reply).map_err(|e| LinkError::internal(e.to_string()));
    };
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            let _ = client.call(method::JOB_CANCEL, json!({"job":job}));
            return Err(if cancelled.load(Ordering::Acquire) {
                LinkError::cancelled()
            } else {
                LinkError::new(ErrorCode::Timeout, "peer job exceeded five minutes")
            });
        }
        match client.next_notification(Some(Duration::from_millis(100))) {
            Ok(message)
                if message.method.as_deref() == Some(method::JOB_DONE)
                    && message
                        .params()
                        .get("job")
                        .and_then(serde_json::Value::as_str)
                        == Some(job) =>
            {
                return serde_json::from_value::<JobDone>(message.params().clone())
                    .map_err(|e| LinkError::internal(e.to_string()))?
                    .into_result();
            }
            Ok(_) => {}
            Err(error) if error.code == ErrorCode::Timeout => {}
            Err(_) => {
                return Err(LinkError::new(
                    ErrorCode::NotRunning,
                    "peer stopped while working",
                ));
            }
        }
    }
}

/// SPEC §5.4: pipelines carry their ID in options, never in preset.
pub fn action_reference(tool: &ToolManifest, preset: Option<&str>) -> Content {
    let mut data = json!({"app":ids::BOX, "action":format!("box:{}", tool.id), "version":1, "title":tool.name});
    if let Some(id) = tool.id.strip_prefix("arcade.pipeline.") {
        data["action"] = json!("box.pipeline.run");
        data["options"] = json!({"pipeline":id});
    } else if let Some(preset) = preset.and_then(|id| tool.preset(id)) {
        data["preset"] = json!(preset.id);
        data["title"] = json!(preset.name);
    } else if tool.status != arcade_contract::ImplementationStatus::Implemented
        || super::link_accepts(tool).is_empty() && !tool.inputs.is_empty()
    {
        data["action"] = json!("box.open");
        data["options"] = json!({"tool":tool.id});
    }
    data["input"] = json!(if tool.inputs.is_empty() {
        "none"
    } else if tool.inputs.iter().any(|t| t.starts_with("file/")) {
        "file-selection"
    } else {
        "clipboard"
    });
    Content::structured("arcade-action", data)
}

pub fn output_content(runtime: &Arcade, output: &ToolValue) -> Result<Content, LinkError> {
    use arcade_contract::ValueKind;
    match output.kind {
        ValueKind::Artifact => {
            let path = runtime
                .grants()
                .resolve(&output.value)
                .map_err(|e| LinkError::unsupported(e.to_string()))?;
            Ok(Content::file(&path).with_owner(ids::BOX))
        }
        ValueKind::File => {
            Ok(Content::file(std::path::Path::new(&output.value)).with_owner(ids::BOX))
        }
        ValueKind::Url => Ok(Content::url(&output.value)),
        ValueKind::Text => {
            let kind = link_type_of_output(&output.mime);
            if kind.starts_with("structured/") {
                Ok(Content::structured(
                    kind.trim_start_matches("structured/"),
                    serde_json::from_str(&output.value)
                        .map_err(|e| LinkError::unsupported(e.to_string()))?,
                ))
            } else {
                Ok(Content::text(&kind, &output.value))
            }
        }
    }
}

pub fn recognize_image(
    runtime: &Arcade,
    tool: &ToolManifest,
    path: &std::path::Path,
    language: &str,
    cancelled: &AtomicBool,
) -> Result<arcade_contract::ToolResult, String> {
    if language != "eng" {
        return Err("Arcade Lens uses its own OCR language settings. Choose Tesseract for a specific language pack.".into());
    }
    let peer = peer_action(runtime, ids::LENS, "lens.recognize")
        .ok_or_else(|| "Arcade Lens OCR isn't available".to_string())?
        .0;
    let result = invoke(
        runtime,
        ids::LENS,
        InvokeRequest::new("lens.recognize", ids::BOX)
            .input(Content::file(path))
            .options(json!({"ocrOnly":true})),
        cancelled,
    )
    .map_err(|e| e.user_message("Arcade Lens"))?;
    let text = result
        .outputs
        .iter()
        .filter(|c| c.kind == "text/plain")
        .map(arcade_link::handoff::read_text)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .join("\n");
    Ok(arcade_contract::ToolResult {
        tool_id: tool.id.clone(),
        status: arcade_contract::ResultStatus::Success,
        outputs: vec![ToolValue::text(text, "text/plain")],
        message: Some("Recognized image text locally with Arcade Lens".into()),
        warnings: vec![],
        metadata: [
            ("providerId".into(), json!("ocr.lens")),
            ("providerSource".into(), json!("arcade-app")),
            ("providerPath".into(), json!(peer.executable)),
            ("providerVersion".into(), json!(peer.version)),
            (
                "ocrEngine".into(),
                result
                    .data
                    .unwrap_or_default()
                    .get("ocrEngine")
                    .cloned()
                    .unwrap_or_default(),
            ),
        ]
        .into_iter()
        .collect(),
    })
}

/// Copy Lens's temporary image before its handoff is cleaned, then use the
/// normal Box artifact/grant boundary. The caller's image remains read-only.
pub fn capture_region(
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<crate::SelectedFile, String> {
    let result = invoke(
        runtime,
        ids::LENS,
        InvokeRequest::new("lens.capture", ids::BOX),
        cancelled,
    )
    .map_err(|e| e.user_message("Arcade Lens"))?;
    let image = result
        .outputs
        .iter()
        .find(|c| c.kind == "file/image")
        .ok_or("Arcade Lens returned no region image")?;
    let path = std::path::Path::new(
        image
            .path
            .as_deref()
            .ok_or("Arcade Lens returned no image path")?,
    );
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
        return Err("Arcade Lens returned an invalid or oversized region image".into());
    }
    let copy = runtime
        .artifact_staging_root()
        .join(format!("lens-region-{}.png", uuid::Uuid::new_v4()));
    std::fs::copy(path, &copy).map_err(|e| e.to_string())?;
    match runtime.grants().grant(&copy) {
        Ok(selected) => Ok(selected),
        Err(error) => {
            let _ = std::fs::remove_file(copy);
            Err(error.to_string())
        }
    }
}

pub fn pick_clipboard(
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<arcade_contract::ToolResult, String> {
    let result = invoke(
        runtime,
        ids::CLIPBOARD,
        InvokeRequest::new("clipboard.pick", ids::BOX),
        cancelled,
    )
    .map_err(|e| e.user_message("Arcade Clipboard"))?;
    let mut outputs = Vec::new();
    for content in result.outputs {
        match arcade_link::content::family(&content.kind) {
            "text" => outputs.push(ToolValue::text(
                arcade_link::handoff::read_text(&content).map_err(|e| e.to_string())?,
                &content.kind,
            )),
            "file" => {
                for path in content.all_paths() {
                    let source = std::path::Path::new(path);
                    let owned;
                    let path = if source.starts_with(Locations::discover().handoff) {
                        owned = super::copy_handoff_input(runtime, source)
                            .map_err(|e| e.to_string())?;
                        owned.as_path()
                    } else {
                        source
                    };
                    outputs.push(
                        runtime
                            .grants()
                            .grant(path)
                            .map_err(|e| e.to_string())?
                            .as_tool_value(),
                    );
                }
            }
            _ => return Err("Arcade Clipboard returned an unsupported clip".into()),
        }
    }
    Ok(arcade_contract::ToolResult {
        tool_id: "arcade.system.clipboard-history".into(),
        status: arcade_contract::ResultStatus::Success,
        outputs,
        message: result.message,
        warnings: vec![],
        metadata: [("providerId".into(), json!("clipboard.pick"))]
            .into_iter()
            .collect(),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultOffer {
    pub key: String,
    pub app: String,
    pub title: String,
    pub enabled: bool,
    pub reason: Option<String>,
    pub preview: String,
    /// Takes the tool itself (Box's action reference), so a preset can be chosen.
    pub for_tool: bool,
    /// Sends the result off this computer: show the preview and ask first.
    pub confirm: bool,
}

/// How many other apps' actions a result shows at most (best first).
const RESULT_OFFERS: usize = 8;
/// Effects that send the result off this computer: the user confirms first (SPEC §1 rule 8).
const OUTBOUND: [&str; 2] = ["sends-to-device", "uploads-content"];

/// One thing another app can do with a result: Link's offer for it and the request that runs it.
struct Candidate {
    offer: offers::Offer,
    request: InvokeRequest,
    for_tool: bool,
    confirm: bool,
}

/// What other apps can do with a tool's results, from Link's shared `offers` (the installed cards, the catalog's
/// rules and the user's choices), so Box keeps no list of other apps' actions. Files travel as themselves (one
/// file) or as one batch of paths; text as its own type or as plain text. A second pass offers the tool itself
/// (`structured/arcade-action`, e.g. Wheel's "Add to Wheel").
fn result_candidates(
    runtime: &Arcade,
    locations: &Locations,
    outputs: &[ToolValue],
    tool: &ToolManifest,
    preset: Option<&str>,
) -> Result<Vec<Candidate>, LinkError> {
    let contents = outputs
        .iter()
        .map(|o| output_content(runtime, o))
        .collect::<Result<Vec<_>, _>>()?;
    let settings = LinkSettings::load(runtime);
    let registry = registry(runtime).snapshot();
    let cards: Vec<Manifest> = registry
        .apps()
        .iter()
        .filter(|m| m.id != ids::BOX && settings.uses(&m.id))
        .cloned()
        .collect();
    let rules = arcade_link::catalog::Catalog::load(locations).rules;
    let prefs = offers::Prefs::load(locations);
    // Results are saved where the user asked, so they are the user's files, not Box's hand-off files.
    let files: Vec<Content> = contents
        .iter()
        .filter(|c| c.kind.starts_with("file/"))
        .map(|c| Content {
            owner: None,
            ..c.clone()
        })
        .collect();
    let values: Vec<Content> = if files.is_empty() {
        contents.iter().filter_map(sendable_value).collect()
    } else {
        Vec::new()
    };
    let items: Vec<offers::Item> = if !files.is_empty() {
        files
            .iter()
            .map(|c| offers::Item {
                kind: c.kind.clone(),
                size: None,
                hints: c.hints.clone(),
            })
            .collect()
    } else if let Some(first) = values.first() {
        // Several values are offered as one: their shared type, else plain text.
        let kind = if values.iter().all(|v| v.kind == first.kind) {
            first.kind.clone()
        } else {
            "text/plain".into()
        };
        vec![offers::Item::new(&kind)]
    } else {
        Vec::new()
    };
    let mut out: Vec<Candidate> = Vec::new();
    let mut add =
        |offer: offers::Offer, inputs: &dyn Fn(&Action) -> Vec<Content>, for_tool: bool| {
            if out.iter().any(|c| c.offer.reference() == offer.reference()) {
                return;
            }
            let Some(action) = registry
                .get(&offer.app)
                .and_then(|m| m.actions.iter().find(|a| a.id == offer.action))
            else {
                return;
            };
            let mut request = InvokeRequest::new(&offer.action, ids::BOX);
            request.inputs = inputs(action);
            let confirm = action
                .effects
                .iter()
                .any(|e| OUTBOUND.contains(&e.as_str()));
            out.push(Candidate {
                offer,
                request,
                for_tool,
                confirm,
            });
        };
    if !items.is_empty() {
        for offer in offers::offers(&items, surface::PALETTE, &cards, &rules, &prefs)
            .into_iter()
            .take(RESULT_OFFERS)
        {
            if files.is_empty() {
                add(
                    offer,
                    &|a: &Action| values.iter().map(|v| as_accepted(v, &a.accepts)).collect(),
                    false,
                );
            } else {
                add(offer, &|a: &Action| file_inputs(&files, &a.accepts), false);
            }
        }
    }
    let reference = action_reference(tool, preset);
    for offer in offers::offers(
        &[offers::Item::new(&reference.kind)],
        surface::PALETTE,
        &cards,
        &rules,
        &prefs,
    ) {
        add(offer, &|_: &Action| vec![reference.clone()], true);
    }
    Ok(out)
}

/// A non-file result as another app takes it: folders as references, text as itself, structured values as their
/// JSON text. Nothing else travels.
fn sendable_value(content: &Content) -> Option<Content> {
    match content.kind.as_str() {
        "folder/reference" => Some(Content {
            owner: None,
            ..content.clone()
        }),
        kind if kind.starts_with("text/") => Some(content.clone()),
        kind if kind.starts_with("structured/") => content
            .data
            .as_ref()
            .map(|data| Content::plain(data.to_string())),
        _ => None,
    }
}

/// `value` typed as `accepts` names it, or as plain text when the action takes plain text but not its type.
fn as_accepted(value: &Content, accepts: &[String]) -> Content {
    if accepts.iter().any(|a| content::content_matches(a, value)) {
        return Content {
            kind: content::type_for(accepts, &value.kind),
            ..value.clone()
        };
    }
    match &value.text {
        Some(text) if value.kind.starts_with("text/") => Content::plain(text.clone()),
        _ => value.clone(),
    }
}

/// One file as itself when the action takes it; otherwise (or for several) one batch of their paths.
fn file_inputs(files: &[Content], accepts: &[String]) -> Vec<Content> {
    if let [one] = files {
        if accepts.iter().any(|a| content::content_matches(a, one)) {
            return vec![Content {
                kind: content::type_for(accepts, &one.kind),
                ..one.clone()
            }];
        }
    }
    let paths: Vec<_> = files
        .iter()
        .flat_map(|c| c.all_paths())
        .map(std::path::Path::new)
        .collect();
    let mut batch = Content::files(&paths);
    batch.kind = content::type_for(accepts, &batch.kind);
    vec![batch]
}

/// The requests behind a result's offers: `(<app>/<action>, app, request)`.
pub fn result_requests(
    runtime: &Arcade,
    locations: &Locations,
    outputs: &[ToolValue],
    tool: &ToolManifest,
    preset: Option<&str>,
) -> Result<Vec<(String, String, InvokeRequest)>, LinkError> {
    Ok(
        result_candidates(runtime, locations, outputs, tool, preset)?
            .into_iter()
            .filter(|c| c.offer.enabled)
            .map(|c| (c.offer.reference(), c.offer.app, c.request))
            .collect(),
    )
}

pub fn result_offers(
    runtime: &Arcade,
    locations: &Locations,
    outputs: &[ToolValue],
    tool: &ToolManifest,
    preset: Option<&str>,
) -> Result<Vec<ResultOffer>, LinkError> {
    let settings = LinkSettings::load(runtime);
    let registry = registry(runtime).snapshot();
    let mut offers = Vec::new();
    for Candidate {
        offer,
        request,
        for_tool,
        confirm,
    } in result_candidates(runtime, locations, outputs, tool, preset)?
    {
        let app_name = arcade_link::manifest::app_name(&offer.app);
        // Never a broken entry: greyed out with Link's reason (an install it needs, a limit), or with Box's own
        // check of the payload (so an oversized device send explains its limit).
        let (enabled, reason) = if !offer.enabled {
            let reason = offer.reason.clone().or_else(|| {
                offer.install.as_ref().map(|need| {
                    format!(
                        "Needs {}",
                        need.split_once(':').map_or(need.as_str(), |x| x.1)
                    )
                })
            });
            (false, reason)
        } else {
            match check_request(&settings, &registry, &offer.app, &request) {
                Err(e) if e.code == ErrorCode::UnsupportedInput => continue,
                Err(e) => (false, Some(e.user_message(app_name))),
                Ok(_) => (true, None),
            }
        };
        let preview = request
            .inputs
            .iter()
            .map(|c| {
                if let Some(d) = &c.data {
                    d.get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Result")
                        .to_string()
                } else if !c.all_paths().is_empty() {
                    c.all_paths()
                        .iter()
                        .map(|p| {
                            std::path::Path::new(p)
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned()
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    c.text.as_deref().unwrap_or("").chars().take(160).collect()
                }
            })
            .collect::<Vec<_>>()
            .join(" · ");
        offers.push(ResultOffer {
            key: offer.reference(),
            title: offer.title.clone(),
            app: offer.app.clone(),
            enabled,
            reason,
            preview,
            for_tool,
            confirm,
        });
    }
    Ok(offers)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn peers() -> (tempfile::TempDir, Locations, Arcade) {
        let dir = tempfile::tempdir().unwrap();
        let loc = Locations::under(dir.path());
        let runtime = Arcade::in_memory().unwrap();
        runtime
            .link_registry
            .set(SharedRegistry::load(&loc))
            .ok()
            .unwrap();
        (dir, loc, runtime)
    }
    fn write_peer(loc: &Locations, enabled: bool, available: bool) {
        let mut manifest = Manifest::new(
            ids::CLIPBOARD,
            "1",
            std::env::current_exe().unwrap().to_str().unwrap(),
        );
        manifest.settings.link_enabled = enabled;
        let mut action = Action::new("clipboard.add", "Send to my devices", "send")
            .accepts(&["text/*", "file/any[]"])
            .effects(&["sends-to-device"]);
        action.available = available;
        action.reason = (!available).then(|| "Private mode".into());
        action.max_bytes = Some(DEVICE_LIMIT);
        manifest.actions.push(action);
        arcade_link::manifest::write_manifest(loc, &manifest).unwrap();
    }
    fn write_shelf(loc: &Locations, enabled: bool, available: bool) {
        let mut manifest = Manifest::new(
            ids::SHELF,
            "1",
            std::env::current_exe().unwrap().to_str().unwrap(),
        );
        manifest.settings.link_enabled = enabled;
        let mut action = Action::new("shelf.add", "Add to Shelf", "add").accepts(&[
            "file/*",
            "file/*[]",
            "folder/reference",
            "text/plain",
            "text/url",
            "text/rich",
        ]);
        action.available = available;
        action.reason = (!available).then(|| "Shelf is busy".into());
        manifest.actions.push(action);
        arcade_link::manifest::write_manifest(loc, &manifest).unwrap();
    }
    fn write_card(loc: &Locations, app: &str, actions: Vec<Action>) {
        let mut manifest =
            Manifest::new(app, "1", std::env::current_exe().unwrap().to_str().unwrap());
        manifest.actions = actions;
        arcade_link::manifest::write_manifest(loc, &manifest).unwrap();
    }
    #[test]
    fn resident_cancel_returns_after_ack_without_waiting_for_peer_done() {
        use arcade_link::server::{Handler, InvokeContext, Reply};
        use std::sync::{Arc, Mutex};
        struct Picker {
            cancel: Arc<AtomicBool>,
            acknowledged: Arc<AtomicBool>,
            pending: Mutex<Vec<arcade_link::Job>>,
        }
        impl Handler for Picker {
            fn describe(&self) -> Vec<Action> {
                vec![Action::new("lens.capture", "Capture", "capture").interactive(true)]
            }
            fn invoke(&self, _: InvokeRequest, ctx: &InvokeContext) -> Result<Reply, LinkError> {
                let job = ctx.start_job();
                let ticket = job.ticket();
                let observed = self.acknowledged.clone();
                job.on_cancel(move || {
                    observed.store(true, Ordering::Release);
                });
                self.pending.lock().unwrap().push(job);
                self.cancel.store(true, Ordering::Release);
                Ok(Reply::Job(ticket))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let locations = Locations::under(dir.path());
        let cancel = Arc::new(AtomicBool::new(false));
        let acknowledged = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(Picker {
            cancel: cancel.clone(),
            acknowledged: acknowledged.clone(),
            pending: Mutex::new(vec![]),
        });
        let server = arcade_link::Server::start(
            arcade_link::ServerConfig {
                locations: locations.clone(),
                app: PeerInfo {
                    id: ids::LENS.into(),
                    version: "1".into(),
                },
            },
            handler.clone(),
        )
        .unwrap();
        let mut client = Client::connect(&locations, ids::LENS, &me()).unwrap();
        let started = std::time::Instant::now();
        let error = invoke_resident(
            &mut client,
            &InvokeRequest::new("lens.capture", ids::BOX),
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert!(acknowledged.load(Ordering::Acquire));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        drop(client);
        handler.pending.lock().unwrap().clear();
        server.stop();
    }

    #[test]
    fn action_references_keep_preset_and_pipeline_options_separate() {
        let runtime = Arcade::in_memory().unwrap();
        let mut tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == "arcade.image.convert")
            .unwrap();
        let content = action_reference(&tool, Some("webp"));
        assert_eq!(content.data.as_ref().unwrap()["preset"], "webp");
        tool.id = "arcade.pipeline.image-small".into();
        let content = action_reference(&tool, None);
        let data = content.data.unwrap();
        assert_eq!(data["action"], "box.pipeline.run");
        assert_eq!(data["options"], json!({"pipeline":"image-small"}));
        assert!(data.get("preset").is_none());
    }

    #[test]
    fn peers_missing_disabled_unavailable_and_oversized_are_handled() {
        let (_dir, loc, runtime) = peers();
        let tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == "arcade.text.case")
            .unwrap();
        let output = ToolValue::text("hello", "text/plain");
        assert!(
            result_offers(&runtime, &loc, std::slice::from_ref(&output), &tool, None)
                .unwrap()
                .is_empty()
        );
        write_peer(&loc, true, true);
        registry(&runtime).refresh();
        let offers =
            result_offers(&runtime, &loc, std::slice::from_ref(&output), &tool, None).unwrap();
        assert_eq!(offers.len(), 1);
        assert!(offers[0].enabled);
        let oversized = ToolValue::text("x".repeat(DEVICE_LIMIT as usize + 1), "text/plain");
        let offer = result_offers(&runtime, &loc, &[oversized], &tool, None)
            .unwrap()
            .remove(0);
        assert!(!offer.enabled);
        assert_eq!(
            offer.reason.as_deref(),
            Some("Too large to send to your devices (limit 16 MB).")
        );
        LinkSettings {
            enabled: true,
            disabled_peers: vec![ids::CLIPBOARD.into()],
        }
        .save(&runtime)
        .unwrap();
        assert!(
            result_offers(&runtime, &loc, std::slice::from_ref(&output), &tool, None)
                .unwrap()
                .is_empty()
        );
        LinkSettings::default().save(&runtime).unwrap();
        write_peer(&loc, true, false);
        registry(&runtime).refresh();
        assert!(
            result_offers(&runtime, &loc, std::slice::from_ref(&output), &tool, None)
                .unwrap()
                .is_empty()
        );
        write_peer(&loc, false, true);
        registry(&runtime).refresh();
        assert!(
            result_offers(&runtime, &loc, &[output], &tool, None)
                .unwrap()
                .is_empty()
        );
        let request = InvokeRequest::new("clipboard.add", ids::BOX).input(Content::plain("hello"));
        assert_eq!(
            check_request(
                &LinkSettings::default(),
                &registry(&runtime).snapshot(),
                ids::CLIPBOARD,
                &request
            )
            .unwrap_err()
            .code,
            ErrorCode::Denied
        );
    }

    #[test]
    fn result_files_travel_by_path_with_their_own_type() {
        let (dir, loc, runtime) = peers();
        write_peer(&loc, true, true);
        write_card(
            &loc,
            ids::LENS,
            vec![Action::new("lens.pin", "Pin to screen", "pin").accepts(&["file/image"])],
        );
        registry(&runtime).refresh();
        let path = dir.path().join("result.png");
        image::RgbImage::new(3, 2).save(&path).unwrap();
        let selected = runtime.grants().grant(&path).unwrap();
        let tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == "arcade.image.convert")
            .unwrap();
        let requests = result_requests(
            &runtime,
            &loc,
            &[selected.as_tool_value()],
            &tool,
            Some("webp"),
        )
        .unwrap();
        let sent = &requests
            .iter()
            .find(|(key, _, _)| key == "arcade.clipboard/clipboard.add")
            .unwrap()
            .2
            .inputs[0];
        // `file/any[]` takes one file too (SPEC §5.1): it travels as itself, with its own type.
        assert_eq!(sent.kind, "file/image");
        assert!(sent.owner.is_none());
        // Grants hold canonical paths (macOS: /var is /private/var).
        let path = path.canonicalize().unwrap();
        assert_eq!(sent.all_paths(), vec![path.to_str().unwrap()]);
        assert_eq!(
            requests
                .iter()
                .find(|(key, _, _)| key == "arcade.lens/lens.pin")
                .unwrap()
                .2
                .inputs[0]
                .kind,
            "file/image"
        );
        assert!(path.is_file());
    }

    #[test]
    fn shelf_offer_follows_the_peer_and_sends_references() {
        let (dir, loc, runtime) = peers();
        let first = dir.path().join("result.png");
        image::RgbImage::new(3, 2).save(&first).unwrap();
        let second = dir.path().join("notes.pdf");
        std::fs::write(&second, b"%PDF-1.4").unwrap();
        let outputs = [
            runtime.grants().grant(&first).unwrap().as_tool_value(),
            runtime.grants().grant(&second).unwrap().as_tool_value(),
        ];
        let tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == "arcade.image.convert")
            .unwrap();
        let shelf = |outputs: &[ToolValue]| {
            result_offers(&runtime, &loc, outputs, &tool, None)
                .unwrap()
                .into_iter()
                .find(|o| o.key == "arcade.shelf/shelf.add")
        };
        // Missing Shelf: no row.
        assert!(shelf(&outputs).is_none());

        write_shelf(&loc, true, true);
        registry(&runtime).refresh();
        let offer = shelf(&outputs).unwrap();
        assert_eq!(offer.app, ids::SHELF);
        assert_eq!(offer.title, "Add to Shelf");
        assert!(offer.enabled);
        assert_eq!(offer.preview, "result.png, notes.pdf");

        // Files go as one reference batch: originals stay where they are.
        let requests = result_requests(&runtime, &loc, &outputs, &tool, None).unwrap();
        let (_, app, request) = requests
            .iter()
            .find(|(key, ..)| key == "arcade.shelf/shelf.add")
            .unwrap();
        assert_eq!(app, ids::SHELF);
        assert_eq!(request.action, "shelf.add");
        assert_eq!(request.inputs.len(), 1);
        let batch = &request.inputs[0];
        assert_eq!(batch.kind, "file/any[]");
        assert!(batch.owner.is_none());
        let expected = [
            first.canonicalize().unwrap(),
            second.canonicalize().unwrap(),
        ];
        assert_eq!(
            batch.all_paths(),
            expected
                .iter()
                .map(|p| p.to_str().unwrap())
                .collect::<Vec<_>>()
        );
        assert!(first.is_file() && second.is_file());

        // Text Shelf stores natively passes through; other text, and structured
        // values (as their JSON), become plain text.
        let text_tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == "arcade.text.case")
            .unwrap();
        let texts = [
            ToolValue::text("hello", "text/plain"),
            ToolValue::text("<b>hi</b>", "text/html"),
            ToolValue::text("{\"a\":1}", "structured/json"),
        ];
        let requests = result_requests(&runtime, &loc, &texts, &text_tool, None).unwrap();
        let inputs = &requests
            .iter()
            .find(|(key, ..)| key == "arcade.shelf/shelf.add")
            .unwrap()
            .2
            .inputs;
        assert_eq!(
            inputs
                .iter()
                .map(|c| (c.kind.as_str(), c.text.as_deref().unwrap()))
                .collect::<Vec<_>>(),
            vec![
                ("text/plain", "hello"),
                ("text/plain", "<b>hi</b>"),
                ("text/plain", "{\"a\":1}")
            ]
        );

        // Turned off in Box, disabled in Shelf, or unavailable: no row.
        LinkSettings {
            enabled: true,
            disabled_peers: vec![ids::SHELF.into()],
        }
        .save(&runtime)
        .unwrap();
        assert!(shelf(&outputs).is_none());
        LinkSettings::default().save(&runtime).unwrap();
        assert!(shelf(&outputs).is_some());
        write_shelf(&loc, false, true);
        registry(&runtime).refresh();
        assert!(shelf(&outputs).is_none());
        write_shelf(&loc, true, false);
        registry(&runtime).refresh();
        assert!(shelf(&outputs).is_none());
    }

    #[test]
    fn result_offers_come_from_the_cards_not_a_list_of_apps() {
        let (dir, loc, runtime) = peers();
        let path = dir.path().join("result.png");
        image::RgbImage::new(3, 2).save(&path).unwrap();
        let outputs = [runtime.grants().grant(&path).unwrap().as_tool_value()];
        let tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == "arcade.image.convert")
            .unwrap();
        write_peer(&loc, true, true);
        // An app Box has never heard of is offered from its card alone.
        let mut ocr = Action::new("notes.keep", "Keep in Notes", "keep").accepts(&["file/image"]);
        ocr.rank = [("file/image".to_string(), 90u8)].into_iter().collect();
        let mut needs = Action::new("notes.read", "Read the text", "read").accepts(&["file/image"]);
        needs.available = false;
        needs.reason = Some("Tesseract isn't installed".into());
        needs.requires = vec!["engine:tesseract".into()];
        write_card(&loc, "arcade.notes", vec![ocr, needs]);
        write_card(
            &loc,
            ids::WHEEL,
            vec![
                Action::new("wheel.add_action", "Add to Wheel", "add")
                    .accepts(&["structured/arcade-action"]),
            ],
        );
        // Box's own card never offers Box's tools back to it.
        write_card(
            &loc,
            ids::BOX,
            vec![
                Action::new("box:arcade.image.resize", "Resize", "resize").accepts(&["file/image"]),
            ],
        );
        registry(&runtime).refresh();
        let offers = result_offers(&runtime, &loc, &outputs, &tool, Some("webp")).unwrap();
        let keys: Vec<&str> = offers.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "arcade.notes/notes.keep",
                "arcade.clipboard/clipboard.add",
                "arcade.notes/notes.read",
                "arcade.wheel/wheel.add_action"
            ]
        );
        let notes = &offers[0];
        assert!(notes.enabled && !notes.confirm && !notes.for_tool);
        assert_eq!(notes.title, "Keep in Notes");
        // Sending off this computer asks first.
        assert!(offers[1].confirm);
        // An action that needs an engine shows why instead of disappearing.
        assert!(!offers[2].enabled);
        assert_eq!(
            offers[2].reason.as_deref(),
            Some("Tesseract isn't installed")
        );
        assert!(
            !result_requests(&runtime, &loc, &outputs, &tool, Some("webp"))
                .unwrap()
                .iter()
                .any(|(key, ..)| key == "arcade.notes/notes.read")
        );
        // Wheel takes the tool itself, with the chosen preset.
        let wheel = &offers[3];
        assert!(wheel.for_tool && wheel.enabled);
        let requests = result_requests(&runtime, &loc, &outputs, &tool, Some("webp")).unwrap();
        let (_, app, request) = requests
            .iter()
            .find(|(key, ..)| key == "arcade.wheel/wheel.add_action")
            .unwrap();
        assert_eq!(app, ids::WHEEL);
        assert_eq!(request.inputs[0].data.as_ref().unwrap()["preset"], "webp");
        // The user's hidden actions and the catalog's renames apply.
        arcade_link::offers::Prefs {
            hidden: vec!["arcade.notes/notes.keep".into()],
            ..Default::default()
        }
        .save(&loc)
        .unwrap();
        std::fs::write(
            loc.catalog(),
            r#"{"schema":1,"apps":[],"rules":{"rename":{"arcade.clipboard/clipboard.add":"Send to phone"}}}"#,
        )
        .unwrap();
        let offers = result_offers(&runtime, &loc, &outputs, &tool, None).unwrap();
        assert!(!offers.iter().any(|o| o.key == "arcade.notes/notes.keep"));
        assert_eq!(offers[0].title, "Send to phone");
    }
}
