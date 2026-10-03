//! Webpage to PDF or full-page screenshot with headless Chromium, driven
//! over the DevTools protocol on a loopback WebSocket. Chromium runs with a
//! throwaway profile, a cleared environment, and is killed when the job ends.

use super::{stage_dir, url_input};
use crate::{
    Arcade,
    provider::find_system_executable,
    tool_kit::{
        check_cancelled, number_in, option_bool, option_str, output_name, publish_file, success,
    },
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};

const BROWSERS: [&str; 6] = [
    "chromium",
    "chromium-browser",
    "google-chrome-stable",
    "google-chrome",
    "brave",
    "microsoft-edge",
];
/// Chromium can't capture textures taller than this in one screenshot.
const MAX_CAPTURE_HEIGHT: f64 = 16_000.0;

pub(super) fn capture(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<ToolResult, String> {
    let url = url_input(request)?;
    let output = option_str(request, "output", "pdf");
    let width = number_in(
        request,
        "width",
        "Window width",
        Some(1366.0),
        320.0..=3840.0,
    )?
    .round();
    let stage = stage_dir(runtime.artifact_staging_root())?;
    let executable = BROWSERS
        .into_iter()
        .find_map(find_system_executable)
        .ok_or("Webpage capture needs Chromium or Google Chrome")?;
    let browser = Browser::launch(&executable, stage.path())?;
    progress(0.1);
    let mut session = browser.session(cancelled)?;
    session.call("Page.enable", json!({}))?;
    session.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width, "height": 900, "deviceScaleFactor": 1, "mobile": false}),
    )?;
    if option_bool(request, "dark", false) {
        session.call(
            "Emulation.setEmulatedMedia",
            json!({"features": [{"name": "prefers-color-scheme", "value": "dark"}]}),
        )?;
    }
    let navigation = session.call("Page.navigate", json!({"url": url}))?;
    if let Some(error) = navigation["errorText"].as_str() {
        return Err(format!("The page could not be opened: {error}"));
    }
    session.wait_for_event("Page.loadEventFired", Duration::from_secs(45))?;
    progress(0.5);
    // Scroll through once so lazy-loaded images appear, then settle.
    session.call(
        "Runtime.evaluate",
        json!({"expression": "(async () => { for (let y = 0; y < document.body.scrollHeight && y < 60000; y += innerHeight) { scrollTo(0, y); await new Promise(r => setTimeout(r, 120)); } scrollTo(0, 0); await new Promise(r => setTimeout(r, 600)); })()", "awaitPromise": true}),
    )?;
    check_cancelled(cancelled)?;
    let title = session.call(
        "Runtime.evaluate",
        json!({"expression": "document.title", "returnByValue": true}),
    )?["result"]["value"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    progress(0.7);
    let (bytes, extension, mut warnings) = match output {
        "pdf" => {
            let (paper_width, paper_height) = match option_str(request, "paper", "a4") {
                "letter" => (8.5, 11.0),
                _ => (8.27, 11.69),
            };
            let landscape = option_str(request, "orientation", "portrait") == "landscape";
            let result = session.call(
                "Page.printToPDF",
                json!({
                    "printBackground": option_bool(request, "background", true),
                    "landscape": landscape,
                    "paperWidth": paper_width,
                    "paperHeight": paper_height,
                    "marginTop": 0.4, "marginBottom": 0.4, "marginLeft": 0.4, "marginRight": 0.4,
                }),
            )?;
            (decode(&result)?, "pdf", Vec::new())
        }
        "png" | "jpg" => {
            let metrics = session.call("Page.getLayoutMetrics", json!({}))?;
            let content = &metrics["cssContentSize"];
            let page_width = content["width"].as_f64().unwrap_or(width).max(1.0);
            let page_height = content["height"].as_f64().unwrap_or(900.0).max(1.0);
            let mut warnings = Vec::new();
            let height = if page_height > MAX_CAPTURE_HEIGHT {
                warnings.push(format!("The page is {page_height:.0} px tall; the capture stops at {MAX_CAPTURE_HEIGHT:.0} px."));
                MAX_CAPTURE_HEIGHT
            } else {
                page_height
            };
            let mut params = json!({
                "format": if output == "png" { "png" } else { "jpeg" },
                "captureBeyondViewport": true,
                "clip": {"x": 0, "y": 0, "width": page_width, "height": height, "scale": 1},
            });
            if output == "jpg" {
                params["quality"] = json!(88);
            }
            let result = session.call("Page.captureScreenshot", params)?;
            (decode(&result)?, output, warnings)
        }
        other => return Err(format!("Unknown output: {other}")),
    };
    let _ = session.call("Browser.close", json!({}));
    drop(browser);
    progress(0.9);
    let staged = stage.path().join(format!("capture.{extension}"));
    std::fs::write(&staged, &bytes).map_err(|error| error.to_string())?;
    let stem = safe_title(&title);
    let name = output_name(request, &format!("{stem}.{extension}"))?;
    let saved = publish_file(request, runtime, &staged, &name, cancelled)?;
    warnings.push("Pages that need you to sign in, or that block automated browsers, may capture incorrectly.".into());
    Ok(success(
        manifest,
        vec![saved],
        Some(format!(
            "Captured {}",
            if title.is_empty() { "the page" } else { &title }
        )),
        warnings,
    ))
}

fn decode(result: &Value) -> Result<Vec<u8>, String> {
    STANDARD
        .decode(result["data"].as_str().ok_or("Chromium returned no data")?)
        .map_err(|error| format!("Chromium returned invalid data: {error}"))
}

fn safe_title(title: &str) -> String {
    let cleaned = title
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, ' ' | '-' | '_') {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>();
    let words = cleaned
        .split_whitespace()
        .take(10)
        .collect::<Vec<_>>()
        .join(" ");
    if words.is_empty() {
        "webpage".into()
    } else {
        words.chars().take(80).collect()
    }
}

/// A headless Chromium process that is killed (with its children) on drop.
struct Browser {
    child: Child,
    endpoint: String,
}

impl Browser {
    fn launch(executable: &Path, stage: &Path) -> Result<Self, String> {
        let profile: PathBuf = stage.join("profile");
        std::fs::create_dir_all(&profile).map_err(|error| error.to_string())?;
        let mut command = Command::new(executable);
        command
            .args([
                "--headless=new",
                "--disable-gpu",
                "--hide-scrollbars",
                "--mute-audio",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-sync",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-default-apps",
                "--password-store=basic",
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .env_clear()
            .env("HOME", stage)
            .env("LANG", "C.UTF-8")
            .current_dir(stage)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start Chromium: {error}"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or("Could not read Chromium's output")?;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(endpoint) = line
                    .split_once("DevTools listening on ")
                    .map(|(_, rest)| rest.trim().to_owned())
                {
                    let _ = sender.send(endpoint);
                }
            }
        });
        match receiver.recv_timeout(Duration::from_secs(20)) {
            Ok(endpoint) if endpoint.starts_with("ws://127.0.0.1:") => Ok(Self { child, endpoint }),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                Err("Chromium did not start its automation endpoint".into())
            }
        }
    }

    /// A DevTools session attached to a fresh tab.
    fn session<'a>(&self, cancelled: &'a AtomicBool) -> Result<Session<'a>, String> {
        let address: SocketAddr = self
            .endpoint
            .trim_start_matches("ws://")
            .split('/')
            .next()
            .and_then(|host| host.parse().ok())
            .ok_or("Chromium reported an invalid endpoint")?;
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .map_err(|error| error.to_string())?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(512 * 1024 * 1024))
            .max_frame_size(Some(512 * 1024 * 1024));
        let (socket, _) =
            tungstenite::client::client_with_config(self.endpoint.as_str(), stream, Some(config))
                .map_err(|error| format!("Could not connect to Chromium: {error}"))?;
        let mut session = Session {
            socket,
            next_id: 0,
            session_id: None,
            cancelled,
            events: Vec::new(),
        };
        let target = session.call("Target.createTarget", json!({"url": "about:blank"}))?;
        let target_id = target["targetId"]
            .as_str()
            .ok_or("Chromium did not open a tab")?
            .to_owned();
        let attached = session.call(
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
        )?;
        session.session_id = attached["sessionId"].as_str().map(str::to_owned);
        Ok(session)
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            // Chromium spawns helper processes in its own process group.
            libc::killpg(self.child.id() as i32, libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Session<'a> {
    socket: WebSocket<TcpStream>,
    next_id: u64,
    session_id: Option<String>,
    cancelled: &'a AtomicBool,
    events: Vec<String>,
}

impl Session<'_> {
    fn read(&mut self) -> Result<Option<Value>, String> {
        check_cancelled(self.cancelled)?;
        match self.socket.read() {
            Ok(Message::Text(text)) => Ok(serde_json::from_str(text.as_str()).ok()),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(format!("Lost the connection to Chromium: {error}")),
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = &self.session_id {
            message["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::text(message.to_string()))
            .map_err(|error| format!("Could not talk to Chromium: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(120);
        while Instant::now() < deadline {
            let Some(reply) = self.read()? else { continue };
            if reply["id"].as_u64() == Some(id) {
                if let Some(error) = reply.get("error") {
                    return Err(format!(
                        "Chromium could not {method}: {}",
                        error["message"].as_str().unwrap_or("unknown error")
                    ));
                }
                return Ok(reply["result"].clone());
            }
            if let Some(event) = reply["method"].as_str() {
                self.events.push(event.to_owned());
            }
        }
        Err(format!("Chromium did not answer {method} in time"))
    }

    fn wait_for_event(&mut self, name: &str, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.events.iter().any(|event| event == name) {
                return Ok(());
            }
            if let Some(message) = self.read()?
                && let Some(event) = message["method"].as_str()
            {
                self.events.push(event.to_owned());
            }
        }
        // Slow pages: capture whatever has loaded rather than failing.
        Ok(())
    }
}
