//! System-first provider discovery. Discovery never invokes a shell or accepts
//! an executable name from an untrusted tool request.

use crate::process::{self, ProcessSpec};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub capability: String,
    pub source: String,
    pub executable_path: PathBuf,
    pub version: String,
    pub compatible: bool,
    pub warning: Option<String>,
    pub capabilities: Vec<String>,
}

/// Local Arcade peers discovered from cached manifests; no provider process
/// is started during discovery.
pub fn discover_arcade(runtime: &crate::Arcade) -> Vec<ProviderInfo> {
    [
        ("ocr.lens", "lens.recognize"),
        ("screen.select.lens", "lens.capture"),
    ]
    .into_iter()
    .filter_map(|(capability, action)| {
        let (manifest, _) =
            crate::link::consumer::peer_action(runtime, arcade_link::ids::LENS, action)?;
        Some(ProviderInfo {
            capability: capability.into(),
            source: "arcade-app".into(),
            executable_path: manifest.executable.into(),
            version: manifest.version,
            compatible: true,
            warning: None,
            capabilities: vec![capability.replacen('.', ":", 1)],
        })
    })
    .collect()
}

/// A separately managed model package. Model weights have independent
/// provenance and license terms from the executable that runs them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAssetInfo {
    pub id: String,
    pub version: Option<String>,
    pub model_name: String,
    pub path: PathBuf,
    pub sha256: Option<String>,
    pub source: String,
    pub license: String,
    pub provenance: String,
    pub compatible_provider: String,
}

/// A verified executable/model pairing. `model_home_environment` is set only
/// for providers that need an explicit local cache root; provider processes
/// still start with a cleared environment.
#[derive(Debug, Clone)]
pub struct ImageModelProvider {
    pub provider: ProviderInfo,
    pub asset: ModelAssetInfo,
    pub model_home: Option<PathBuf>,
    pub model_home_environment: Option<OsString>,
}

impl ImageModelProvider {
    pub fn environment(&self) -> Vec<(OsString, OsString)> {
        match (&self.model_home_environment, &self.model_home) {
            (Some(key), Some(home)) => vec![(key.clone(), home.as_os_str().to_os_string())],
            _ => vec![],
        }
    }
}

pub fn discover_ffmpeg(user_binary: Option<&Path>) -> Vec<ProviderInfo> {
    let mut candidates = Vec::new();
    if let Some(path) = user_binary {
        candidates.push((path.to_path_buf(), "user-configured"));
    }
    for directory in system_search_directories() {
        candidates.push((directory.join(binary_name("ffmpeg")), "system"));
    }
    let mut seen = HashSet::new();
    let mut providers = Vec::new();
    for (candidate, source) in candidates {
        let Ok(path) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Some(info) = probe_ffmpeg_at(&path, source) {
            providers.push(info);
        }
    }
    providers.sort_by_key(|provider| {
        (
            !provider.compatible,
            provider.source != "user-configured",
            provider.source != "system",
        )
    });
    providers
}

pub fn discover_qpdf(user_binary: Option<&Path>) -> Vec<ProviderInfo> {
    let mut candidates = Vec::new();
    if let Some(path) = user_binary {
        candidates.push((path.to_path_buf(), "user-configured"));
    }
    for directory in system_search_directories() {
        candidates.push((directory.join(binary_name("qpdf")), "system"));
    }
    #[cfg(windows)]
    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push((
            PathBuf::from(program_files)
                .join("qpdf")
                .join("bin")
                .join("qpdf.exe"),
            "system",
        ));
    }
    let mut seen = HashSet::new();
    let mut providers = Vec::new();
    for (candidate, source) in candidates {
        let Ok(path) = fs::canonicalize(&candidate) else {
            continue;
        };
        if seen.insert(path.clone()) {
            if let Some(info) = probe_qpdf_at(&path, source) {
                providers.push(info);
            }
        }
    }
    providers.sort_by_key(|provider| {
        (
            !provider.compatible,
            provider.source != "user-configured",
            provider.source != "system",
        )
    });
    providers
}

pub fn discover_vips(user_binary: Option<&Path>) -> Vec<ProviderInfo> {
    let mut candidates = Vec::new();
    if let Some(path) = user_binary {
        candidates.push((path.to_path_buf(), "user-configured"));
    }
    for directory in system_search_directories() {
        candidates.push((directory.join(binary_name("vips")), "system"));
    }
    let mut seen = HashSet::new();
    let mut providers = Vec::new();
    for (candidate, source) in candidates {
        let Ok(path) = fs::canonicalize(&candidate) else {
            continue;
        };
        if seen.insert(path.clone()) {
            if let Some(info) = probe_vips_at(&path, source) {
                providers.push(info);
            }
        }
    }
    providers.sort_by_key(|provider| {
        (
            !provider.compatible,
            provider.source != "user-configured",
            provider.source != "system",
        )
    });
    providers
}

/// Discover a pre-existing rembg CLI and cached U²-Net model weights. Only
/// files that are already present are returned. Image execution selects the
/// CLI's local custom-model mode so a missing model can never trigger a fetch.
pub fn discover_background_removal_models() -> Vec<ImageModelProvider> {
    let Some(executable_path) = find_system_executable("rembg") else {
        return vec![];
    };
    if suspicious_path(&executable_path).is_some() {
        return vec![];
    }
    let Some(help) = command_output_with_stderr(&executable_path, &["i", "--help"], 256 * 1024)
    else {
        return vec![];
    };
    let lower = help.to_ascii_lowercase();
    if !lower.contains("usage:") || !lower.contains("-m") || !lower.contains("-x") {
        return vec![];
    }
    let version = command_output_with_stderr(&executable_path, &["--version"], 16 * 1024)
        .and_then(|output| output.lines().next().map(str::trim).map(str::to_owned))
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "rembg CLI (version unavailable)".into());
    let mut seen_paths = HashSet::new();
    let mut providers = Vec::new();
    for model_name in ["u2netp", "u2net"] {
        let filename = format!("{model_name}.onnx");
        let found = rembg_model_homes().into_iter().find_map(|(home, env_key)| {
            let canonical_home = fs::canonicalize(home).ok()?;
            if !safe_model_directory(&canonical_home) {
                return None;
            }
            let nested = canonical_home
                .join("models")
                .join(model_name)
                .join(&filename);
            let legacy = canonical_home.join(&filename);
            [nested, legacy].into_iter().find_map(|candidate| {
                let metadata = fs::symlink_metadata(&candidate).ok()?;
                if !metadata.is_file() || !(1_000_000..=2_000_000_000).contains(&metadata.len()) {
                    return None;
                }
                let canonical = fs::canonicalize(candidate).ok()?;
                if !canonical.starts_with(&canonical_home) || !seen_paths.insert(canonical.clone())
                {
                    return None;
                }
                Some((canonical_home.clone(), canonical, env_key.clone()))
            })
        });
        let Some((model_home, model_path, env_key)) = found else {
            continue;
        };
        providers.push(ImageModelProvider {
            provider: ProviderInfo {
                capability: "image.model.background-removal".into(),
                source: "system".into(),
                executable_path: executable_path.clone(),
                version: version.clone(),
                compatible: true,
                warning: None,
                capabilities: vec![
                    "image:background-removal".into(),
                    format!("image:model:{model_name}"),
                ],
            },
            asset: ModelAssetInfo {
                id: format!("arcade.model.rembg.{model_name}"),
                version: None,
                model_name: model_name.into(),
                path: model_path,
                sha256: None,
                source: "pre-existing rembg model cache".into(),
                license: "Unverified for this converted weight file; check the model source before redistribution".into(),
                provenance: "rembg model catalog (MIT tool code; weight license is separate): https://github.com/danielgatis/rembg".into(),
                compatible_provider: "image.model.background-removal".into(),
            },
            model_home: Some(model_home),
            model_home_environment: Some(env_key),
        });
    }
    providers
}

/// Discover a pre-existing Real-ESRGAN NCNN/Vulkan executable and local x4plus
/// model files. This path never downloads or bundles the model package.
pub fn discover_upscale_models() -> Vec<ImageModelProvider> {
    let Some(executable_path) = find_system_executable("realesrgan-ncnn-vulkan") else {
        return vec![];
    };
    if suspicious_path(&executable_path).is_some() {
        return vec![];
    }
    let Some(help) = command_output_with_stderr(&executable_path, &["-h"], 256 * 1024) else {
        return vec![];
    };
    let lower = help.to_ascii_lowercase();
    if !lower.contains("realesrgan")
        || !lower.contains("-m model-path")
        || !lower.contains("-s scale")
        || !lower.contains("realesrgan-x4plus")
    {
        return vec![];
    }
    let mut model_directories = Vec::new();
    if let Some(parent) = executable_path.parent() {
        model_directories.push(parent.join("models"));
        model_directories.push(parent.to_path_buf());
        if let Some(grandparent) = parent.parent() {
            model_directories.push(grandparent.join("models"));
        }
    }
    let mut seen = HashSet::new();
    model_directories
        .into_iter()
        .filter_map(|directory| fs::canonicalize(directory).ok())
        .filter(|directory| seen.insert(directory.clone()) && safe_model_directory(directory))
        .filter_map(|directory| {
            let model_name = "realesrgan-x4plus";
            let parameter_path = directory.join(format!("{model_name}.param"));
            let weights_path = directory.join(format!("{model_name}.bin"));
            let parameter_meta = fs::symlink_metadata(&parameter_path).ok()?;
            let weights_meta = fs::symlink_metadata(&weights_path).ok()?;
            if !parameter_meta.is_file()
                || !weights_meta.is_file()
                || parameter_meta.len() < 64
                || weights_meta.len() < 1_000_000
            {
                return None;
            }
            let parameter_path = fs::canonicalize(parameter_path).ok()?;
            let weights_path = fs::canonicalize(weights_path).ok()?;
            if !parameter_path.starts_with(&directory) || !weights_path.starts_with(&directory) {
                return None;
            }
            Some(ImageModelProvider {
                provider: ProviderInfo {
                    capability: "image.model.upscale".into(),
                    source: "system".into(),
                    executable_path: executable_path.clone(),
                    version: "Real-ESRGAN NCNN/Vulkan (version unavailable)".into(),
                    compatible: true,
                    warning: None,
                    capabilities: vec![
                        "image:upscale".into(),
                        format!("image:model:{model_name}"),
                        "image:upscale:scale:2".into(),
                        "image:upscale:scale:3".into(),
                        "image:upscale:scale:4".into(),
                    ],
                },
                asset: ModelAssetInfo {
                    id: format!("arcade.model.{model_name}"),
                    version: None,
                    model_name: model_name.into(),
                    path: directory.clone(),
                    sha256: None,
                    source: "pre-existing Real-ESRGAN model package".into(),
                    license: "Model-weight license not verified by the executable's source-code license; user-managed assets only".into(),
                    provenance: "Real-ESRGAN NCNN/Vulkan provider: https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan".into(),
                    compatible_provider: "image.model.upscale".into(),
                },
                model_home: Some(directory),
                model_home_environment: None,
            })
        })
        .collect()
}

fn rembg_model_homes() -> Vec<(PathBuf, OsString)> {
    let mut homes = Vec::new();
    if let Some(home) = env::var_os("U2NET_HOME") {
        homes.push((PathBuf::from(home), OsString::from("U2NET_HOME")));
    }
    if let Some(home) = env::var_os("REMBG_HOME") {
        homes.push((PathBuf::from(home), OsString::from("REMBG_HOME")));
    }
    if let Some(xdg) = env::var_os("XDG_DATA_HOME") {
        homes.push((
            PathBuf::from(xdg).join("rembg"),
            OsString::from("REMBG_HOME"),
        ));
    }
    if let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        homes.push((home.join(".rembg"), OsString::from("REMBG_HOME")));
        homes.push((home.join(".u2net"), OsString::from("U2NET_HOME")));
    }
    let mut seen = HashSet::new();
    homes.retain(|(home, _)| seen.insert(home.clone()));
    homes
}

fn safe_model_directory(path: &Path) -> bool {
    if !path.is_dir() || suspicious_path(path).is_some() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o002 != 0) {
            return false;
        }
    }
    true
}

/// Discover Poppler's command-line PDF readers/renderers independently. Each
/// executable is identity/version checked before a tool can invoke it.
pub fn discover_poppler() -> Vec<ProviderInfo> {
    [
        ("pdftoppm", "pdf.render"),
        ("pdftotext", "pdf.text"),
        ("pdfinfo", "pdf.info"),
        ("pdfimages", "pdf.images"),
        ("pdfdetach", "pdf.attachments"),
    ]
    .into_iter()
    .filter_map(|(stem, capability)| {
        discover_identified_command(capability, stem, &["-v"], "poppler")
    })
    .collect()
}

/// Discover a system Tesseract installation and the language data actually
/// available to it; language packs remain separately installed resources.
pub fn discover_tesseract() -> Vec<ProviderInfo> {
    let Some(mut provider) =
        discover_identified_command("ocr.tesseract", "tesseract", &["--version"], "tesseract")
    else {
        return vec![];
    };
    let Some(languages) =
        command_output_with_stderr(&provider.executable_path, &["--list-langs"], 256 * 1024)
    else {
        return vec![];
    };
    if !languages
        .to_ascii_lowercase()
        .contains("list of available languages")
    {
        return vec![];
    }
    provider.capabilities.push("ocr:recognize".into());
    for language in languages
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("List of available"))
    {
        if language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            provider
                .capabilities
                .push(format!("ocr:language:{language}"));
        }
    }
    if provider.capabilities.contains(&"ocr:language:eng".into()) {
        provider.compatible = true;
        provider.warning = None;
    } else {
        provider.compatible = false;
        provider.warning = Some("Tesseract has no English language data installed".into());
    }
    vec![provider]
}

/// Discover OCRmyPDF, which coordinates PDF rendering, OCR, and searchable
/// text-layer assembly through its mature local provider stack.
pub fn discover_ocrmypdf() -> Vec<ProviderInfo> {
    // `--version` prints a bare number, so identify the program by its usage text.
    discover_identified_command("pdf.ocrmypdf", "ocrmypdf", &["--help"], "ocrmypdf")
        .map(|mut provider| {
            if let Some(version) = command_output(&provider.executable_path, &["--version"], 1024) {
                provider.version = version.trim().to_owned();
            }
            provider.capabilities.push("pdf:searchable-ocr".into());
            vec![provider]
        })
        .unwrap_or_default()
}

/// Discover the optional img2pdf command used to embed selected raster images
/// into a PDF without decoding them into full in-memory pixel buffers.
pub fn discover_img2pdf() -> Vec<ProviderInfo> {
    discover_identified_command("pdf.create", "img2pdf", &["--version"], "img2pdf")
        .map(|mut provider| {
            provider.capabilities.push("pdf:images-to-pdf".into());
            vec![provider]
        })
        .unwrap_or_default()
}

/// Discover an installed LibreOffice headless executable. Arcade Box never
/// bundles or updates the user's office suite.
pub fn discover_libreoffice() -> Vec<ProviderInfo> {
    let mut candidates = Vec::new();
    if let Some(path) = find_system_executable("soffice") {
        candidates.push(path);
    }
    if let Some(path) = find_system_executable("libreoffice") {
        candidates.push(path);
    }
    #[cfg(windows)]
    if let Some(program_files) = env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("LibreOffice")
                .join("program")
                .join("soffice.exe"),
        );
    }
    #[cfg(target_os = "macos")]
    candidates.push(PathBuf::from(
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    ));
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter_map(|candidate| fs::canonicalize(candidate).ok())
        .filter(|path| seen.insert(path.clone()))
        .filter_map(|path| {
            let warning = suspicious_path(&path);
            let output = command_output_with_stderr(&path, &["--version"], 64 * 1024)?;
            let version = output.lines().next()?.trim();
            if !version.to_ascii_lowercase().contains("libreoffice") {
                return None;
            }
            let compatible = warning.is_none();
            Some(ProviderInfo {
                capability: "office.libreoffice".into(),
                source: "system".into(),
                executable_path: path,
                version: version.to_owned(),
                compatible,
                warning: warning.or_else(|| {
                    (!compatible).then(|| "LibreOffice provider failed validation".into())
                }),
                capabilities: if compatible {
                    vec!["document:render:pdf".into()]
                } else {
                    vec![]
                },
            })
        })
        .collect()
}

fn discover_identified_command(
    capability: &str,
    stem: &str,
    version_args: &[&str],
    expected_identity: &str,
) -> Option<ProviderInfo> {
    let executable_path = find_system_executable(stem)?;
    let warning = suspicious_path(&executable_path);
    let output = command_output_with_stderr(&executable_path, version_args, 64 * 1024)?;
    let lower = output.to_ascii_lowercase();
    if !lower.contains(expected_identity) || !lower.bytes().any(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let version = output.lines().next()?.trim().to_owned();
    let compatible = warning.is_none();
    Some(ProviderInfo {
        capability: capability.into(),
        source: "system".into(),
        executable_path,
        version,
        compatible,
        warning,
        capabilities: if compatible {
            vec![capability.replace('.', ":")]
        } else {
            vec![]
        },
    })
}

/// Discover a compatible system curl without trusting its basename alone.
/// HTTP is intentionally delegated to the user's system provider instead of
/// bundling a second TLS stack or runtime into Arcade Box.
pub fn discover_curl() -> Vec<ProviderInfo> {
    discover_versioned_command("network.http", "curl", &["--version"], &[], |output| {
        let first = output.lines().next().unwrap_or_default();
        first.starts_with("curl ") && output.contains("https") && output.contains("http")
    })
}

/// Discover yt-dlp and validate both its version response and command
/// interface before it is exposed to first-party download tools.
pub fn discover_ytdlp() -> Vec<ProviderInfo> {
    discover_versioned_command(
        "media.ytdlp",
        "yt-dlp",
        &["--version"],
        &["--help"],
        |output| output.lines().next().is_some_and(valid_ytdlp_version),
    )
}

/// Discover a local JavaScript runtime supported by the installed yt-dlp.
/// The returned capability distinguishes Deno from Node for argument setup.
pub fn discover_ytdlp_js_runtimes() -> Vec<ProviderInfo> {
    let mut providers = Vec::new();
    for (name, capability, expected) in [
        ("deno", "media.ytdlp-js.deno", "deno "),
        ("node", "media.ytdlp-js.node", "v"),
    ] {
        providers.extend(discover_versioned_command(
            capability,
            name,
            &["--version"],
            &[],
            |output| {
                output
                    .lines()
                    .next()
                    .is_some_and(|line| line.starts_with(expected))
            },
        ));
    }
    providers
}

/// Arcade Box's per-user data folder, which holds user-installed helper tools
/// (`tools/`) and model files (`models/`).
pub fn app_data_dir() -> Option<PathBuf> {
    const APP_ID: &str = "dev.arcadebox.app";
    #[cfg(target_os = "macos")]
    let base =
        env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(windows)]
    let base = env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    base.map(|base| base.join(APP_ID))
}

/// A model file the user placed under `<app data>/models/<relative>`.
pub fn user_model_file(relative: &str) -> Option<PathBuf> {
    let root = fs::canonicalize(app_data_dir()?.join("models")).ok()?;
    if !safe_model_directory(&root) {
        return None;
    }
    let path = fs::canonicalize(root.join(relative)).ok()?;
    (path.starts_with(&root) && path.is_file()).then_some(path)
}

/// Folders searched after PATH for user-installed helpers: `~/.local/bin`,
/// `<app data>/tools`, and each folder directly inside it (an unpacked
/// release such as `tools/piper/piper`). A desktop launcher's PATH often
/// lacks these.
fn user_tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(unix)]
    if let Some(home) = env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/bin"));
    }
    if let Some(tools) = app_data_dir().map(|dir| dir.join("tools")) {
        if let Ok(entries) = fs::read_dir(&tools) {
            let mut nested = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>();
            nested.sort();
            dirs.push(tools);
            dirs.extend(nested);
        }
    }
    dirs
}

/// Resolve a system executable candidate for platform adapters. Callers must
/// still perform a safe identity/capability probe before invoking it.
pub fn find_system_executable(stem: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(unix)]
    candidates.extend(
        [
            "/usr/bin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/opt/local/bin",
        ]
        .map(|dir| PathBuf::from(dir).join(binary_name(stem))),
    );
    #[cfg(windows)]
    if let Some(root) = env::var_os("SystemRoot") {
        candidates.push(PathBuf::from(root).join("System32").join(binary_name(stem)));
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|dir| dir.join(binary_name(stem))));
    }
    candidates.extend(
        user_tool_dirs()
            .into_iter()
            .map(|dir| dir.join(binary_name(stem))),
    );
    for candidate in candidates {
        if let Ok(path) = fs::canonicalize(candidate) {
            if is_executable_file(&path) && suspicious_path(&path).is_none() {
                return Some(path);
            }
        }
    }
    None
}

fn discover_versioned_command(
    capability: &str,
    stem: &str,
    version_args: &[&str],
    help_args: &[&str],
    validate: impl Fn(&str) -> bool,
) -> Vec<ProviderInfo> {
    let mut candidates = Vec::new();
    #[cfg(unix)]
    candidates.extend(
        [
            "/usr/bin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/opt/local/bin",
        ]
        .map(|dir| (PathBuf::from(dir).join(binary_name(stem)), "system")),
    );
    #[cfg(windows)]
    {
        if let Some(system_root) = env::var_os("SystemRoot") {
            candidates.push((
                PathBuf::from(system_root)
                    .join("System32")
                    .join(binary_name(stem)),
                "system",
            ));
        }
        if let Some(program_files) = env::var_os("ProgramFiles") {
            let root = PathBuf::from(program_files);
            candidates.push((
                root.join(stem).join("bin").join(binary_name(stem)),
                "system",
            ));
            candidates.push((
                root.join("Git")
                    .join("mingw64")
                    .join("bin")
                    .join(binary_name(stem)),
                "system",
            ));
        }
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|dir| (dir.join(binary_name(stem)), "path")));
    }
    candidates.extend(
        user_tool_dirs()
            .into_iter()
            .map(|dir| (dir.join(binary_name(stem)), "user")),
    );
    let mut seen = HashSet::new();
    let mut providers = Vec::new();
    for (candidate, source) in candidates {
        let Ok(path) = fs::canonicalize(candidate) else {
            continue;
        };
        if !seen.insert(path.clone()) || !is_executable_file(&path) {
            continue;
        }
        if let Some(warning) = suspicious_path(&path) {
            providers.push(ProviderInfo {
                capability: capability.into(),
                source: source.into(),
                executable_path: path,
                version: "unverified".into(),
                compatible: false,
                warning: Some(warning),
                capabilities: vec![],
            });
            continue;
        }
        let version = match command_output(&path, version_args, 64 * 1024) {
            Some(version) if validate(&version) => version,
            _ => continue,
        };
        let mut capabilities = Vec::new();
        if !help_args.is_empty() {
            let Some(help) = command_output(&path, help_args, 2 * 1024 * 1024) else {
                continue;
            };
            if stem == "yt-dlp" && (!help.contains("yt-dlp") || !help.contains("--output")) {
                continue;
            }
            capabilities.push("download:single".into());
            if help.contains("--yes-playlist") {
                capabilities.push("download:playlist".into());
            }
            if help.contains("--remux-video") {
                capabilities.push("download:remux".into());
            }
        }
        providers.push(ProviderInfo {
            capability: capability.into(),
            source: source.into(),
            executable_path: path,
            version: version.lines().next().unwrap_or_default().trim().to_owned(),
            compatible: true,
            warning: None,
            capabilities,
        });
    }
    providers.sort_by_key(|provider| {
        (
            !provider.compatible,
            provider.source != "user-configured",
            provider.source != "system",
        )
    });
    providers
}

fn valid_ytdlp_version(version: &str) -> bool {
    let version = version.trim();
    version.len() >= 8
        && version.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        && version.as_bytes()[4] == b'.'
        && version[5..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.' || byte == b'-')
}

pub fn probe_vips_at(path: &Path, source: &str) -> Option<ProviderInfo> {
    if !is_executable_file(path) {
        return None;
    }
    let header = fs::canonicalize(path.with_file_name(binary_name("vipsheader"))).ok()?;
    if !is_executable_file(&header) {
        return None;
    }
    if let Some(warning) = suspicious_path(path).or_else(|| suspicious_path(&header)) {
        return Some(ProviderInfo {
            capability: "image.vips".into(),
            source: source.into(),
            executable_path: path.to_path_buf(),
            version: "unverified".into(),
            compatible: false,
            warning: Some(warning),
            capabilities: vec![],
        });
    }
    let version = command_output(path, &["--version"], 64 * 1024)?;
    let version = version.trim().strip_prefix("vips-")?.to_owned();
    let header_version = command_output(&header, &["--version"], 64 * 1024)?;
    if header_version.trim() != format!("vips-{version}") {
        return None;
    }
    let (major, minor) = parse_major_minor(&version)?;
    let supported_version = major > 8 || major == 8 && minor >= 14;
    let thumbnail_help = command_output(path, &["thumbnail", "--help-operation"], 64 * 1024)?;
    let supports_resize = thumbnail_help.contains("--height") && thumbnail_help.contains("--crop");
    let savers = command_output(path, &["-l", "foreign"], 512 * 1024)?;
    let operations = command_output(path, &["-l"], 2 * 1024 * 1024)?;
    let mut capabilities = Vec::new();
    if supports_resize {
        capabilities.push("image:resize".into());
    }
    // Class names depend on the codec library libvips was built with (for
    // example `VipsForeignSaveSpngFile` with libspng on Debian and Ubuntu), so
    // match the stable operation nickname as well.
    for (format, operation, nickname) in [
        ("png", "VipsForeignSavePngFile", "(pngsave)"),
        ("jpeg", "VipsForeignSaveJpegFile", "(jpegsave)"),
        ("webp", "VipsForeignSaveWebpFile", "(webpsave)"),
        ("tiff", "VipsForeignSaveTiffFile", "(tiffsave)"),
    ] {
        if savers.contains(operation) || savers.contains(nickname) {
            capabilities.push(format!("image:save:{format}"));
        }
    }
    for (format, operation, nickname) in [
        ("heif", "VipsForeignLoadHeifFile", "(heifload)"),
        ("gif", "VipsForeignLoadGifFile", "(gifload)"),
        ("bmp", "VipsForeignLoadMagickFile", "(magickload)"),
    ] {
        if savers.contains(operation) || savers.contains(nickname) {
            capabilities.push(format!("image:load:{format}"));
        }
    }
    for (capability, operation) in [
        ("crop", "extract_area"),
        ("rot", "rot"),
        ("flip", "flip"),
        ("autorot", "autorot"),
        ("draw_rect", "draw_rect"),
        ("flatten", "flatten"),
        ("colourspace", "colourspace"),
        ("abs", "abs"),
        ("subtract", "subtract"),
        ("add", "add"),
        ("avg", "avg"),
        ("max", "max"),
    ] {
        if operations
            .lines()
            .any(|line| line.contains(&format!("({operation})")))
        {
            capabilities.push(format!("image:operation:{capability}"));
        }
    }
    let compatible = supported_version && supports_resize && capabilities.len() > 1;
    Some(ProviderInfo {
        capability: "image.vips".into(),
        source: source.into(),
        executable_path: path.to_path_buf(),
        version,
        compatible,
        warning: (!compatible).then(|| {
            "libvips 8.14+ with thumbnail and at least one supported image saver is required".into()
        }),
        capabilities: if compatible { capabilities } else { vec![] },
    })
}

pub fn probe_qpdf_at(path: &Path, source: &str) -> Option<ProviderInfo> {
    if !is_executable_file(path) {
        return None;
    }
    if let Some(warning) = suspicious_path(path) {
        return Some(ProviderInfo {
            capability: "pdf.merge".into(),
            source: source.into(),
            executable_path: path.to_path_buf(),
            version: "unverified".into(),
            compatible: false,
            warning: Some(warning),
            capabilities: vec![],
        });
    }
    let output = version_output(path)?;
    let version = output
        .lines()
        .next()?
        .trim()
        .strip_prefix("qpdf version ")?
        .split_whitespace()
        .next()?
        .to_string();
    let (major, minor) = parse_major_minor(&version)?;
    let minimum_supported = major > 11 || major == 11 && minor >= 9;
    // qpdf 12 renamed the help topics, so read the full reference.
    let optimize_help = command_output(path, &["--help=all"], 1024 * 1024)?;
    let supports_pages = optimize_help.contains("--pages") && optimize_help.contains("--file");
    let supports_image_optimization =
        optimize_help.contains("--optimize-images") && optimize_help.contains("--jpeg-quality");
    let compatible = minimum_supported && supports_pages;
    let mut capabilities = if compatible {
        vec![
            "pdf:merge".into(),
            "pdf:split".into(),
            "pdf:structural".into(),
        ]
    } else {
        vec![]
    };
    if compatible && supports_image_optimization {
        capabilities.push("pdf:optimize-images".into());
    }
    Some(ProviderInfo {
        capability: "pdf.merge".into(),
        source: source.into(),
        executable_path: path.to_path_buf(),
        version,
        compatible,
        warning: (!compatible)
            .then(|| "qpdf 11.9 or newer with page selection support is required".into()),
        capabilities,
    })
}

fn parse_major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

pub fn probe_ffmpeg_at(path: &Path, source: &str) -> Option<ProviderInfo> {
    if !is_executable_file(path) {
        return None;
    }
    let parent = path.parent()?;
    let ffprobe = fs::canonicalize(parent.join(binary_name("ffprobe"))).ok()?;
    if !is_executable_file(&ffprobe) {
        return None;
    }
    if let Some(warning) = suspicious_path(path).or_else(|| suspicious_path(&ffprobe)) {
        return Some(ProviderInfo {
            capability: "media.ffmpeg".into(),
            source: source.into(),
            executable_path: path.to_path_buf(),
            version: "unverified".into(),
            compatible: false,
            warning: Some(warning),
            capabilities: vec![],
        });
    }
    let ffmpeg_output = version_output(path)?;
    let ffprobe_output = version_output(&ffprobe)?;
    let first_line = ffmpeg_output.lines().next()?.trim();
    if !first_line.starts_with("ffmpeg version ") || !ffprobe_output.starts_with("ffprobe version ")
    {
        return None;
    }
    let version = first_line
        .strip_prefix("ffmpeg version ")?
        .split_whitespace()
        .next()?
        .to_string();
    let encoders = command_output(path, &["-hide_banner", "-encoders"], 2 * 1024 * 1024)?;
    let muxers = command_output(path, &["-hide_banner", "-muxers"], 1024 * 1024)?;
    let filters = command_output(path, &["-hide_banner", "-filters"], 2 * 1024 * 1024)?;
    let mut capabilities = vec!["probe:streams".into()];
    for (prefix, output, names) in [
        (
            "encoder:",
            &encoders,
            &[
                "libx264",
                "aac",
                "libvpx-vp9",
                "libopus",
                "libmp3lame",
                "pcm_s16le",
                "flac",
                "libvorbis",
                "libwebp_anim",
                "gif",
                "png",
                "srt",
                "webvtt",
                "ass",
                "mov_text",
                "libx265",
                "libsvtav1",
                "prores_ks",
                "mjpeg",
            ][..],
        ),
        (
            "mux:",
            &muxers,
            &[
                "mp4", "matroska", "webm", "gif", "webp", "image2", "mp3", "wav", "flac", "ogg",
                "ipod", "srt", "webvtt", "ass", "mov", "ac3", "eac3", "opus",
            ][..],
        ),
        (
            "filter:",
            &filters,
            &[
                "palettegen",
                "paletteuse",
                "loudnorm",
                "silencedetect",
                "silenceremove",
                "atempo",
                "rubberband",
                "subtitles",
                "scale",
                "crop",
                "transpose",
                "fps",
                "pad",
                "select",
                "anullsrc",
                "concat",
                "hflip",
                "vflip",
                "overlay",
                "afade",
                "acrossfade",
                "atrim",
                "asplit",
                "apad",
                "volume",
                "volumedetect",
                "astats",
                "ametadata",
                "asetnsamples",
                "afftdn",
                "arnndn",
                "alimiter",
                "highpass",
                "lowpass",
                "stereotools",
                "amix",
                "pan",
                "drawtext",
            ][..],
        ),
    ] {
        for name in names {
            if has_ffmpeg_entry(output, name) {
                capabilities.push(format!("{prefix}{name}"));
            }
        }
    }
    Some(ProviderInfo {
        capability: "media.ffmpeg".into(),
        source: source.into(),
        executable_path: path.to_path_buf(),
        version,
        compatible: true,
        warning: None,
        capabilities,
    })
}

fn binary_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.into()
    }
}

fn system_search_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        directories.extend(env::split_paths(&path));
    }
    #[cfg(unix)]
    directories.extend(
        [
            "/usr/bin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
            "/opt/local/bin",
        ]
        .map(PathBuf::from),
    );
    #[cfg(windows)]
    if let Some(program_files) = env::var_os("ProgramFiles") {
        directories.push(PathBuf::from(program_files).join("ffmpeg").join("bin"));
    }
    directories
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    true
}

fn suspicious_path(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return Some("provider path is not absolute".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(directory) = path.parent() {
            if let Ok(metadata) = fs::metadata(directory) {
                let mode = metadata.permissions().mode();
                // A sticky system temp directory still is not an appropriate
                // provenance for an implicitly trusted provider binary.
                if mode & 0o002 != 0 {
                    return Some("provider directory is world-writable".into());
                }
            }
        }
    }
    None
}

fn version_output(path: &Path) -> Option<String> {
    command_output(path, &["-version"], 64 * 1024)
}

fn command_output(path: &Path, args: &[&str], output_limit: usize) -> Option<String> {
    let output = process::run(
        &ProcessSpec {
            executable: path.to_path_buf(),
            args: args.iter().map(OsString::from).collect(),
            current_dir: None,
            timeout: Duration::from_secs(3),
            output_limit,
        },
        &AtomicBool::new(false),
    )
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn command_output_with_stderr(path: &Path, args: &[&str], output_limit: usize) -> Option<String> {
    let output = process::run(
        &ProcessSpec {
            executable: path.to_path_buf(),
            args: args.iter().map(OsString::from).collect(),
            current_dir: None,
            timeout: Duration::from_secs(3),
            output_limit,
        },
        &AtomicBool::new(false),
    )
    .ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    let stderr = String::from_utf8(output.stderr).ok()?;
    Some(format!("{stdout}{stderr}"))
}

fn has_ffmpeg_entry(output: &str, name: &str) -> bool {
    output.lines().any(|line| {
        let mut tokens = line.split_whitespace();
        let flags = tokens.next().unwrap_or_default();
        let entry = tokens.next().unwrap_or_default();
        !flags.is_empty() && flags.len() <= 7 && entry == name
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_binary_is_not_a_provider() {
        assert!(probe_ffmpeg_at(Path::new("/this/path/does/not/exist/ffmpeg"), "system").is_none());
    }

    #[test]
    fn detected_provider_reports_canonical_path() {
        let providers = discover_ffmpeg(None);
        for provider in providers {
            assert!(provider.executable_path.is_absolute());
            assert!(provider.version.len() > 0);
            if provider.compatible {
                assert!(provider.capabilities.contains(&"probe:streams".to_string()));
                // Capabilities are derived from the executable's actual
                // encoder, muxer, and filter listings rather than assumptions
                // based on the version string.
                let encoders = command_output(
                    &provider.executable_path,
                    &["-hide_banner", "-encoders"],
                    2 * 1024 * 1024,
                )
                .unwrap();
                let filters = command_output(
                    &provider.executable_path,
                    &["-hide_banner", "-filters"],
                    2 * 1024 * 1024,
                )
                .unwrap();
                assert_eq!(
                    provider
                        .capabilities
                        .contains(&"encoder:libx264".to_string()),
                    has_ffmpeg_entry(&encoders, "libx264")
                );
                assert_eq!(
                    provider.capabilities.contains(&"filter:atempo".to_string()),
                    has_ffmpeg_entry(&filters, "atempo")
                );
            }
        }
    }

    #[test]
    fn ffmpeg_entry_parser_supports_filters_and_provider_encoders() {
        assert!(has_ffmpeg_entry(
            " T.. atempo A->A Adjust audio tempo",
            "atempo"
        ));
        assert!(has_ffmpeg_entry(
            " V....D libwebp_anim WebP Animated Image",
            "libwebp_anim"
        ));
        assert!(!has_ffmpeg_entry(" .EV fake WebP muxer", "webp"));
    }

    #[test]
    fn ffmpeg_capability_parser_matches_entry_tokens() {
        assert!(has_ffmpeg_entry(" V....D libx264 H.264 encoder", "libx264"));
        assert!(!has_ffmpeg_entry(
            " V....D fake-libx264 Not the encoder",
            "libx264"
        ));
    }

    #[test]
    fn qpdf_version_floor_is_parsed_without_executing_a_binary() {
        assert_eq!(parse_major_minor("12.4.1"), Some((12, 4)));
        assert_eq!(parse_major_minor("11.9.0"), Some((11, 9)));
        assert_eq!(parse_major_minor("not-a-version"), None);
    }
}
