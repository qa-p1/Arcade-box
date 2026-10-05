//! Cached peer discovery and outbound requests. Call these from workers.

use std::sync::atomic::{AtomicBool, Ordering};

use arcade_contract::{ToolManifest, ToolValue};
use arcade_link::{
    Action, CallOptions, Content, ErrorCode, InvokeRequest, InvokeResult, LinkError, Locations,
    Manifest, PeerInfo, Registry, SharedRegistry, ids,
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
    arcade_link::invoke_action(
        &locations,
        &me(),
        &manifest,
        &request,
        CallOptions {
            cancel: Some(cancelled),
            ..Default::default()
        },
    )
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultOffer {
    pub key: String,
    pub app: String,
    pub title: String,
    pub enabled: bool,
    pub reason: Option<String>,
    pub preview: String,
}

pub fn result_requests(
    runtime: &Arcade,
    outputs: &[ToolValue],
    tool: &ToolManifest,
    preset: Option<&str>,
) -> Result<Vec<(String, String, InvokeRequest)>, LinkError> {
    let contents = outputs
        .iter()
        .map(|o| output_content(runtime, o))
        .collect::<Result<Vec<_>, _>>()?;
    let files: Vec<_> = contents
        .iter()
        .filter(|c| c.kind.starts_with("file/"))
        .cloned()
        .collect();
    let mut requests = Vec::new();
    if !files.is_empty() {
        let mut request = InvokeRequest::new("look.preview", ids::BOX);
        request.inputs = files.clone();
        requests.push(("preview".into(), ids::LOOK.into(), request));
    }
    if !contents.is_empty() {
        let mut request = InvokeRequest::new("clipboard.add", ids::BOX);
        let mut paths = Vec::new();
        for file in &files {
            paths.extend(file.all_paths().into_iter().map(std::path::PathBuf::from));
        }
        if !paths.is_empty() {
            let mut batch = Content::files(&paths.iter().map(|p| p.as_path()).collect::<Vec<_>>());
            batch.kind = "file/any[]".into();
            request.inputs.push(batch);
        }
        request.inputs.extend(
            contents
                .iter()
                .filter(|c| !c.kind.starts_with("file/"))
                .map(|c| {
                    if let Some(data) = &c.data {
                        Content::plain(data.to_string())
                    } else {
                        c.clone()
                    }
                }),
        );
        requests.push(("send".into(), ids::CLIPBOARD.into(), request));
    }
    requests.push((
        "wheel".into(),
        ids::WHEEL.into(),
        InvokeRequest::new("wheel.add_action", ids::BOX).input(action_reference(tool, preset)),
    ));
    if files.len() == 1 && files[0].kind == "file/image" {
        requests.push((
            "pin".into(),
            ids::LENS.into(),
            InvokeRequest::new("lens.pin", ids::BOX).input(files[0].clone()),
        ));
    }
    Ok(requests)
}

pub fn result_offers(
    runtime: &Arcade,
    outputs: &[ToolValue],
    tool: &ToolManifest,
    preset: Option<&str>,
) -> Result<Vec<ResultOffer>, LinkError> {
    let settings = LinkSettings::load(runtime);
    let registry = registry(runtime).snapshot();
    let mut offers = Vec::new();
    for (key, app, request) in result_requests(runtime, outputs, tool, preset)? {
        // Missing, disabled and unsupported peers stay hidden. Oversized or
        // unavailable actions stay disabled, with the owner's standard reason.
        if !settings.uses(&app)
            || !registry.get(&app).is_some_and(|m| {
                m.settings.link_enabled
                    && m.action(&request.action)
                        .is_some_and(Action::on_this_platform)
            })
        {
            continue;
        }
        let outcome = check_request(&settings, &registry, &app, &request);
        if outcome
            .as_ref()
            .is_err_and(|e| e.code == ErrorCode::UnsupportedInput)
        {
            continue;
        }
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
            title: match key.as_str() {
                "preview" => "Preview",
                "send" => "Send to my devices ↗",
                "wheel" => "Add to Wheel",
                _ => "Pin",
            }
            .into(),
            key,
            app: app.clone(),
            enabled: outcome.is_ok(),
            reason: outcome
                .err()
                .map(|e| e.user_message(arcade_link::manifest::app_name(&app))),
            preview,
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
            .accepts(&["text/*", "file/any[]"]);
        action.available = available;
        action.reason = (!available).then(|| "Private mode".into());
        action.max_bytes = Some(DEVICE_LIMIT);
        manifest.actions.push(action);
        arcade_link::manifest::write_manifest(loc, &manifest).unwrap();
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
            result_offers(&runtime, std::slice::from_ref(&output), &tool, None)
                .unwrap()
                .is_empty()
        );
        write_peer(&loc, true, true);
        registry(&runtime).refresh();
        let offers = result_offers(&runtime, std::slice::from_ref(&output), &tool, None).unwrap();
        assert_eq!(offers.len(), 1);
        assert!(offers[0].enabled);
        let oversized = ToolValue::text("x".repeat(DEVICE_LIMIT as usize + 1), "text/plain");
        let offer = result_offers(&runtime, &[oversized], &tool, None)
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
            result_offers(&runtime, std::slice::from_ref(&output), &tool, None)
                .unwrap()
                .is_empty()
        );
        LinkSettings::default().save(&runtime).unwrap();
        write_peer(&loc, true, false);
        registry(&runtime).refresh();
        let offer = result_offers(&runtime, std::slice::from_ref(&output), &tool, None)
            .unwrap()
            .remove(0);
        assert!(!offer.enabled && offer.reason.as_ref().unwrap().contains("Private mode"));
        write_peer(&loc, false, true);
        registry(&runtime).refresh();
        assert!(
            result_offers(&runtime, &[output], &tool, None)
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
    fn result_file_payloads_keep_originals_and_share_as_an_array() {
        let (dir, _loc, runtime) = peers();
        let path = dir.path().join("result.png");
        image::RgbImage::new(3, 2).save(&path).unwrap();
        let selected = runtime.grants().grant(&path).unwrap();
        let tool = runtime
            .list_tools()
            .into_iter()
            .find(|t| t.id == "arcade.image.convert")
            .unwrap();
        let requests =
            result_requests(&runtime, &[selected.as_tool_value()], &tool, Some("webp")).unwrap();
        let sent = &requests
            .iter()
            .find(|(key, _, _)| key == "send")
            .unwrap()
            .2
            .inputs[0];
        assert_eq!(sent.kind, "file/any[]");
        assert_eq!(sent.all_paths(), vec![path.to_str().unwrap()]);
        assert_eq!(
            requests
                .iter()
                .find(|(key, _, _)| key == "pin")
                .unwrap()
                .2
                .action,
            "lens.pin"
        );
        assert!(path.is_file());
    }
}
