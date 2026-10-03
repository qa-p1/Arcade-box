use arcade_contract::{ToolValue, ValueKind};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions as CapOpenOptions},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};
use thiserror::Error;
use uuid::Uuid;

/// Opaque, in-memory references minted only after a trusted host-side selection.
/// The frontend receives a label and token, never authority to choose arbitrary paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedFile {
    pub token: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
}

/// Opaque renderer-safe handle to a user-approved output directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedDirectory {
    pub token: String,
    pub name: String,
}

#[derive(Debug)]
struct Grant {
    canonical_path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
    mime: String,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

struct OutputDirectoryGrant {
    canonical_path: PathBuf,
    directory: Dir,
    identity: DirectoryIdentity,
}

struct InputDirectoryGrant {
    canonical_path: PathBuf,
    directory: Dir,
    identity: DirectoryIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    created: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    size: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

#[derive(Default)]
pub struct FileGrants {
    grants: Mutex<HashMap<String, Grant>>,
    output_directories: Mutex<HashMap<String, OutputDirectoryGrant>>,
    input_directories: Mutex<HashMap<String, InputDirectoryGrant>>,
}

#[derive(Debug, Error)]
pub enum GrantError {
    #[error("file selection failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("only regular files can be selected")]
    NotRegularFile,
    #[error("file grant expired or is unknown")]
    UnknownGrant,
    #[error("output directory grant expired or is unknown")]
    UnknownOutputDirectory,
    #[error("selected file changed after selection; select it again")]
    FileChanged,
    #[error("selected output folder is not a regular directory")]
    NotDirectory,
    #[error("selected output folder changed after selection; select it again")]
    DirectoryChanged,
    #[error("output name is unsafe: {0}")]
    UnsafeOutputName(String),
    #[error("too many output folders are selected; close an output dialog and try again")]
    TooManyOutputDirectories,
    #[error("selected input folder changed after selection; select it again")]
    InputDirectoryChanged,
    #[error("selected input folder grant expired or is unknown")]
    UnknownInputDirectory,
    #[error("too many input folders are selected; close a folder dialog and try again")]
    TooManyInputDirectories,
}

impl FileGrants {
    pub fn grant(&self, path: &Path) -> Result<SelectedFile, GrantError> {
        let canonical_path = fs::canonicalize(path)?;
        let mut file = File::open(&canonical_path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(GrantError::NotRegularFile);
        }
        if !same_selected_file(&metadata, &fs::metadata(&canonical_path)?) {
            return Err(GrantError::FileChanged);
        }
        let mut header = [0u8; 8192];
        let count = file.read(&mut header)?;
        let mime = file_mime(&canonical_path, &header[..count]).to_string();
        let token = Uuid::new_v4().to_string();
        let name = canonical_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "selected file".into());
        self.grants
            .lock()
            .expect("file grant mutex poisoned")
            .insert(
                token.clone(),
                Grant {
                    canonical_path,
                    size: metadata.len(),
                    modified: metadata.modified().ok(),
                    mime: mime.clone(),
                    #[cfg(unix)]
                    device: std::os::unix::fs::MetadataExt::dev(&metadata),
                    #[cfg(unix)]
                    inode: std::os::unix::fs::MetadataExt::ino(&metadata),
                },
            );
        Ok(SelectedFile {
            token,
            name,
            size: metadata.len(),
            mime,
        })
    }

    pub fn resolve(&self, token: &str) -> Result<PathBuf, GrantError> {
        let grants = self.grants.lock().expect("file grant mutex poisoned");
        let grant = grants.get(token).ok_or(GrantError::UnknownGrant)?;
        let path = fs::canonicalize(&grant.canonical_path)?;
        let metadata = fs::metadata(&path)?;
        if path != grant.canonical_path || !matches_grant(grant, &metadata) {
            return Err(GrantError::FileChanged);
        }
        Ok(path)
    }

    /// Open the exact selected file for an in-process capability such as a
    /// WASM plugin. The returned handle is checked after opening, closing the
    /// path-swap gap between a path validation and a later file read.
    pub fn open_scoped(&self, token: &str) -> Result<File, GrantError> {
        let grants = self.grants.lock().expect("file grant mutex poisoned");
        let grant = grants.get(token).ok_or(GrantError::UnknownGrant)?;
        let path = fs::canonicalize(&grant.canonical_path)?;
        if path != grant.canonical_path {
            return Err(GrantError::FileChanged);
        }
        let file = File::open(&path)?;
        if !matches_grant(grant, &file.metadata()?) {
            return Err(GrantError::FileChanged);
        }
        Ok(file)
    }

    pub fn revoke(&self, token: &str) {
        self.grants
            .lock()
            .expect("file grant mutex poisoned")
            .remove(token);
    }

    /// Grant output into a directory selected by the user. Writes are made
    /// relative to a held capability directory, never by joining an
    /// untrusted filename to a frontend-provided path.
    pub fn grant_output_directory(&self, path: &Path) -> Result<SelectedDirectory, GrantError> {
        const MAX_OUTPUT_DIRECTORIES: usize = 64;
        let canonical_path = fs::canonicalize(path)?;
        let symlink_metadata = fs::symlink_metadata(&canonical_path)?;
        if symlink_metadata.file_type().is_symlink() {
            return Err(GrantError::NotDirectory);
        }
        let metadata = fs::metadata(&canonical_path)?;
        if !metadata.is_dir() {
            return Err(GrantError::NotDirectory);
        }
        let directory = Dir::open_ambient_dir(&canonical_path, ambient_authority())?;
        let identity = directory_identity(&metadata, &directory.dir_metadata()?);
        if !directory_matches_path(&canonical_path, &identity, &directory)? {
            return Err(GrantError::DirectoryChanged);
        }
        let display_name = canonical_path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Selected folder".into());
        let token = Uuid::new_v4().to_string();
        let mut directories = self
            .output_directories
            .lock()
            .expect("output directory grant mutex poisoned");
        if directories.len() >= MAX_OUTPUT_DIRECTORIES {
            return Err(GrantError::TooManyOutputDirectories);
        }
        directories.insert(
            token.clone(),
            OutputDirectoryGrant {
                canonical_path,
                directory,
                identity,
            },
        );
        Ok(SelectedDirectory {
            token,
            name: display_name,
        })
    }

    /// Grant read-only access to a user-selected directory. The capability
    /// handle stays in the host; consumers never receive a renderer path.
    pub fn grant_input_directory(&self, path: &Path) -> Result<SelectedDirectory, GrantError> {
        const MAX_INPUT_DIRECTORIES: usize = 32;
        let initial = fs::symlink_metadata(path)?;
        if initial.file_type().is_symlink() || !initial.is_dir() {
            return Err(GrantError::NotDirectory);
        }
        let canonical_path = fs::canonicalize(path)?;
        let symlink_metadata = fs::symlink_metadata(&canonical_path)?;
        if symlink_metadata.file_type().is_symlink() || !symlink_metadata.is_dir() {
            return Err(GrantError::NotDirectory);
        }
        let directory = Dir::open_ambient_dir(&canonical_path, ambient_authority())?;
        let identity =
            directory_identity(&fs::metadata(&canonical_path)?, &directory.dir_metadata()?);
        if !directory_matches_path(&canonical_path, &identity, &directory)? {
            return Err(GrantError::InputDirectoryChanged);
        }
        let name = canonical_path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Selected folder".into());
        let token = Uuid::new_v4().to_string();
        let mut directories = self
            .input_directories
            .lock()
            .expect("input directory mutex poisoned");
        if directories.len() >= MAX_INPUT_DIRECTORIES {
            return Err(GrantError::TooManyInputDirectories);
        }
        directories.insert(
            token.clone(),
            InputDirectoryGrant {
                canonical_path,
                directory,
                identity,
            },
        );
        Ok(SelectedDirectory { token, name })
    }

    pub fn revoke_input_directory(&self, token: &str) {
        self.input_directories
            .lock()
            .expect("input directory mutex poisoned")
            .remove(token);
    }

    /// Open a cloned capability handle after rechecking that the selected
    /// directory still names the same non-symlink directory.
    pub fn open_input_directory(&self, token: &str) -> Result<Dir, GrantError> {
        let directories = self
            .input_directories
            .lock()
            .expect("input directory mutex poisoned");
        let grant = directories
            .get(token)
            .ok_or(GrantError::UnknownInputDirectory)?;
        let directory = grant.directory.try_clone()?;
        if !directory_matches_path(&grant.canonical_path, &grant.identity, &directory)? {
            return Err(GrantError::InputDirectoryChanged);
        }
        Ok(directory)
    }

    pub fn revoke_output_directory(&self, token: &str) {
        self.output_directories
            .lock()
            .expect("output directory grant mutex poisoned")
            .remove(token);
    }

    /// Publish a staged output by bounded streaming copy into a user-selected
    /// directory. The requested name is treated as a basename and collisions
    /// receive a numbered suffix; an existing file is never replaced.
    pub fn publish_staged_output(
        &self,
        directory_token: &str,
        staged_path: &Path,
        requested_name: &str,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<SelectedFile, GrantError> {
        let symlink_metadata = fs::symlink_metadata(staged_path)?;
        if symlink_metadata.file_type().is_symlink() || !symlink_metadata.is_file() {
            return Err(GrantError::NotRegularFile);
        }
        let mut source = File::open(staged_path)?;
        let opened_metadata = source.metadata()?;
        if !opened_metadata.is_file()
            || !same_selected_file(&opened_metadata, &fs::metadata(staged_path)?)
        {
            return Err(GrantError::FileChanged);
        }
        self.publish_reader_to_directory(
            directory_token,
            &mut source,
            requested_name,
            cancelled,
            true,
        )
    }

    /// Copy an already granted artifact to a user-selected path for the Save
    /// As workflow. The source remains handle-verified throughout the copy.
    pub fn copy_granted_file_to_directory(
        &self,
        source_token: &str,
        directory_token: &str,
        requested_name: &str,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<SelectedFile, GrantError> {
        let mut source = self.open_scoped(source_token)?;
        self.publish_reader_to_directory(
            directory_token,
            &mut source,
            requested_name,
            cancelled,
            true,
        )
    }

    pub fn copy_granted_file_to_directory_exact(
        &self,
        source_token: &str,
        directory_token: &str,
        requested_name: &str,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<SelectedFile, GrantError> {
        let mut source = self.open_scoped(source_token)?;
        self.publish_reader_to_directory(
            directory_token,
            &mut source,
            requested_name,
            cancelled,
            false,
        )
    }

    fn publish_reader_to_directory(
        &self,
        directory_token: &str,
        source: &mut dyn Read,
        requested_name: &str,
        cancelled: &std::sync::atomic::AtomicBool,
        number_collisions: bool,
    ) -> Result<SelectedFile, GrantError> {
        crate::artifacts::validate_portable_filename(requested_name)
            .map_err(GrantError::UnsafeOutputName)?;
        let (canonical_path, directory, identity) = {
            let directories = self
                .output_directories
                .lock()
                .expect("output directory grant mutex poisoned");
            let grant = directories
                .get(directory_token)
                .ok_or(GrantError::UnknownOutputDirectory)?;
            (
                grant.canonical_path.clone(),
                grant.directory.try_clone()?,
                grant.identity.clone(),
            )
        };
        if !directory_matches_path(&canonical_path, &identity, &directory)? {
            return Err(GrantError::DirectoryChanged);
        }

        let requested_path = Path::new(requested_name);
        let stem = requested_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let extension = requested_path
            .extension()
            .map(|value| value.to_string_lossy().into_owned());
        let mut output_file = None;
        let mut selected_name = String::new();
        let maximum = if number_collisions { 10_000 } else { 1 };
        for number in 1..=maximum {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "Output copy cancelled",
                )
                .into());
            }
            selected_name = if number == 1 {
                requested_name.to_owned()
            } else if let Some(extension) = &extension {
                format!("{stem}-{number}.{extension}")
            } else {
                format!("{stem}-{number}")
            };
            crate::artifacts::validate_portable_filename(&selected_name)
                .map_err(GrantError::UnsafeOutputName)?;
            let mut options = CapOpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match directory.open_with(&selected_name, &options) {
                Ok(file) => {
                    output_file = Some(file);
                    break;
                }
                Err(error)
                    if number_collisions && error.kind() == std::io::ErrorKind::AlreadyExists =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let Some(mut output) = output_file else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Too many files with this name",
            )
            .into());
        };

        let copied = (|| -> std::io::Result<()> {
            let mut buffer = [0u8; 1024 * 1024];
            loop {
                if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Output copy cancelled",
                    ));
                }
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count])?;
            }
            output.flush()?;
            output.sync_all()
        })();
        if let Err(error) = copied {
            drop(output);
            let _ = directory.remove_file(&selected_name);
            return Err(error.into());
        }
        let output_identity = file_identity(&output.metadata()?);
        drop(output);
        if !directory_matches_path(&canonical_path, &identity, &directory)? {
            return Err(GrantError::DirectoryChanged);
        }
        let final_path = canonical_path.join(&selected_name);
        let selected = self.grant(&final_path)?;
        let reopened = self.open_scoped(&selected.token)?;
        if !output_identity.matches(&reopened.metadata()?) {
            self.revoke(&selected.token);
            return Err(GrantError::FileChanged);
        }
        if !directory_matches_path(&canonical_path, &identity, &directory)? {
            self.revoke(&selected.token);
            return Err(GrantError::DirectoryChanged);
        }
        Ok(selected)
    }

    pub fn verify_type(&self, token: &str, mime: &str) -> Result<(), GrantError> {
        if mime == "folder/reference" {
            self.open_input_directory(token)?;
            return Ok(());
        }
        self.resolve(token)?;
        let grants = self.grants.lock().expect("file grant mutex poisoned");
        let grant = grants.get(token).ok_or(GrantError::UnknownGrant)?;
        if grant.mime != mime {
            return Err(GrantError::FileChanged);
        }
        Ok(())
    }
}

fn same_selected_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    if left.len() != right.len() || left.modified().ok() != right.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if left.dev() != right.dev() || left.ino() != right.ino() {
            return false;
        }
    }
    true
}

fn matches_grant(grant: &Grant, metadata: &fs::Metadata) -> bool {
    if !metadata.is_file()
        || metadata.len() != grant.size
        || metadata.modified().ok() != grant.modified
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != grant.device || metadata.ino() != grant.inode {
            return false;
        }
    }
    true
}

fn directory_identity(
    path_metadata: &fs::Metadata,
    _handle_metadata: &cap_std::fs::Metadata,
) -> DirectoryIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        DirectoryIdentity {
            device: path_metadata.dev(),
            inode: path_metadata.ino(),
        }
    }
    #[cfg(windows)]
    {
        let _ = _handle_metadata;
        DirectoryIdentity {
            created: path_metadata.created().ok(),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = _handle_metadata;
        DirectoryIdentity {}
    }
}

fn directory_matches_path(
    path: &Path,
    identity: &DirectoryIdentity,
    directory: &Dir,
) -> Result<bool, GrantError> {
    let canonical = match fs::canonicalize(path) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };
    if canonical != path || fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Ok(false);
    }
    let path_metadata = fs::metadata(path)?;
    let handle_metadata = directory.dir_metadata()?;
    if !path_metadata.is_dir() || !handle_metadata.is_dir() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt as CapMetadataExt;
        use std::os::unix::fs::MetadataExt;
        Ok(path_metadata.dev() == identity.device
            && path_metadata.ino() == identity.inode
            && handle_metadata.dev() == identity.device
            && handle_metadata.ino() == identity.inode)
    }
    #[cfg(windows)]
    {
        Ok(path_metadata.created().ok() == identity.created
            && handle_metadata
                .created()
                .ok()
                .map(cap_std::time::SystemTime::into_std)
                == identity.created)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (identity, handle_metadata);
        Ok(true)
    }
}

fn file_identity(metadata: &cap_std::fs::Metadata) -> FileIdentity {
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        FileIdentity {
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .map(cap_std::time::SystemTime::into_std),
            created: metadata
                .created()
                .ok()
                .map(cap_std::time::SystemTime::into_std),
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
    #[cfg(not(unix))]
    {
        FileIdentity {
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .map(cap_std::time::SystemTime::into_std),
            created: metadata
                .created()
                .ok()
                .map(cap_std::time::SystemTime::into_std),
        }
    }
}

impl FileIdentity {
    fn matches(&self, metadata: &fs::Metadata) -> bool {
        if self.size != metadata.len()
            || self.modified != metadata.modified().ok()
            || self.created != metadata.created().ok()
        {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if self.device != metadata.dev() || self.inode != metadata.ino() {
                return false;
            }
        }
        true
    }
}

impl SelectedFile {
    pub fn as_tool_value(&self) -> ToolValue {
        ToolValue {
            kind: ValueKind::Artifact,
            value: self.token.clone(),
            mime: self.mime.clone(),
        }
    }
}

fn map_mime(mime: &str) -> &'static str {
    if mime.contains("spreadsheetml")
        || mime == "application/vnd.ms-excel"
        || mime == "application/vnd.oasis.opendocument.spreadsheet"
    {
        "file/spreadsheet"
    } else if mime.contains("wordprocessingml")
        || mime.contains("presentationml")
        || matches!(
            mime,
            "application/msword"
                | "application/vnd.ms-powerpoint"
                | "application/rtf"
                | "application/vnd.oasis.opendocument.text"
                | "application/vnd.oasis.opendocument.presentation"
        )
    {
        "file/document"
    } else if mime == "application/zip" || mime == "application/x-zip-compressed" {
        "file/archive"
    } else if mime == "application/pdf" {
        "file/pdf"
    } else if mime.starts_with("image/") {
        "file/image"
    } else if mime.starts_with("video/") {
        "file/video"
    } else if mime.starts_with("audio/") {
        "file/audio"
    } else {
        "file/octet-stream"
    }
}

/// Identify subtitle text formats using both their conventional extension and
/// a small content signature. Subtitle formats are plain text, so there is no
/// reliable magic number to trust on its own. The same classifier is used for
/// selected inputs and generated output grants.
fn file_mime(path: &Path, header: &[u8]) -> &'static str {
    if is_mhtml(header) {
        return "file/mhtml";
    }
    if is_svg(header) {
        return "file/svg";
    }
    if is_delimited_text(path, header) {
        return if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tsv"))
        {
            "file/tsv"
        } else {
            "file/csv"
        };
    }
    if is_html(header) {
        return "file/html";
    }
    if subtitle_content_matches(path, header) {
        return "file/subtitle";
    }
    let detected = infer::get(header)
        .map(|kind| map_mime(kind.mime_type()))
        .unwrap_or("file/octet-stream");
    // Office files are ZIP or OLE containers whose type marker may sit past
    // the sniffed header; trust the extension only for those containers.
    if matches!(detected, "file/archive" | "file/octet-stream") {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let container =
            header.starts_with(b"PK\x03\x04") || header.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]);
        match extension.as_str() {
            "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" if container => return "file/spreadsheet",
            "docx" | "doc" | "odt" | "pptx" | "ppt" | "odp" if container => return "file/document",
            _ => {}
        }
    }
    detected
}

fn is_svg(header: &[u8]) -> bool {
    let text = String::from_utf8_lossy(header);
    let mut remaining = text.trim_start_matches('\u{feff}').trim_start();
    if remaining.starts_with("<?xml") {
        let Some(end) = remaining.find("?>") else {
            return false;
        };
        remaining = remaining[end + 2..].trim_start();
    }
    const ZINT_SVG_DOCTYPE: &str = "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">";
    if remaining.starts_with("<!DOCTYPE") {
        let Some(end) = remaining.find('>') else {
            return false;
        };
        if &remaining[..=end] != ZINT_SVG_DOCTYPE {
            return false;
        }
        remaining = remaining[end + 1..].trim_start();
    }
    while remaining.starts_with("<!--") {
        let Some(end) = remaining.find("-->") else {
            return false;
        };
        remaining = remaining[end + 3..].trim_start();
    }
    let Some(tag) = remaining.strip_prefix("<svg") else {
        return false;
    };
    tag.chars()
        .next()
        .is_some_and(|character| character.is_whitespace() || character == '>')
}

fn is_delimited_text(path: &Path, header: &[u8]) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    // Spreadsheet apps in many locales export `.csv` with semicolons.
    let delimiters: &[u8] = if extension.eq_ignore_ascii_case("csv") {
        b",;"
    } else if extension.eq_ignore_ascii_case("tsv") {
        b"\t"
    } else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(header) else {
        return false;
    };
    if text.contains('\0') {
        return false;
    }
    delimiters.iter().any(|delimiter| {
        csv::ReaderBuilder::new()
            .delimiter(*delimiter)
            .has_headers(false)
            .flexible(false)
            .from_reader(text.as_bytes())
            .records()
            .take(3)
            .filter_map(Result::ok)
            .any(|record| record.len() > 1)
    })
}

fn is_html(header: &[u8]) -> bool {
    let text = String::from_utf8_lossy(header);
    let text = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase();
    text.starts_with("<!doctype html")
        || text.starts_with("<html")
        || text.contains("<html>")
        || text.contains("<html ")
}

fn is_mhtml(header: &[u8]) -> bool {
    let text = String::from_utf8_lossy(header).to_ascii_lowercase();
    let has_mime_version = text
        .lines()
        .any(|line| line.trim_start().starts_with("mime-version:"));
    let has_related = text.lines().any(|line| {
        line.trim_start().starts_with("content-type:") && line.contains("multipart/related")
    });
    has_mime_version && has_related
}

fn subtitle_content_matches(path: &Path, header: &[u8]) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    let extension = extension.to_ascii_lowercase();
    let text = String::from_utf8_lossy(header);
    let text = text.trim_start_matches('\u{feff}');
    match extension.as_str() {
        "srt" => has_subtitle_timing(text, true),
        "vtt" => text
            .lines()
            .next()
            .is_some_and(|line| line.trim().starts_with("WEBVTT")),
        "ass" | "ssa" => {
            let normalized = text.to_ascii_lowercase();
            (normalized.contains("[script info]") || normalized.contains("[events]"))
                && normalized.contains("dialogue:")
        }
        // `.sub` has several unrelated uses; recognize the common MicroDVD
        // subtitle form rather than assigning this type from its extension.
        "sub" => text.lines().any(is_microdvd_cue),
        _ => false,
    }
}

fn has_subtitle_timing(text: &str, require_comma: bool) -> bool {
    text.lines().any(|line| {
        let Some((start, end)) = line.split_once("-->") else {
            return false;
        };
        let start = start.trim();
        let end = end.split_whitespace().next().unwrap_or("").trim();
        valid_subtitle_time(start, require_comma) && valid_subtitle_time(end, require_comma)
    })
}

fn valid_subtitle_time(value: &str, require_comma: bool) -> bool {
    let Some((clock, fraction)) = value.rsplit_once([',', '.']) else {
        return false;
    };
    if fraction.len() != 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if require_comma && !value.contains(',') {
        return false;
    }
    let fields = clock.split(':').collect::<Vec<_>>();
    if fields.len() != 2 && fields.len() != 3 {
        return false;
    }
    if !fields
        .iter()
        .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return false;
    }
    let minute = fields[fields.len() - 2].parse::<u32>().unwrap_or(60);
    let second = fields[fields.len() - 1].parse::<u32>().unwrap_or(60);
    minute < 60 && second < 60
}

fn is_microdvd_cue(line: &str) -> bool {
    let line = line.trim_start();
    let Some(rest) = line.strip_prefix('{') else {
        return false;
    };
    let Some((start, rest)) = rest.split_once("}{") else {
        return false;
    };
    let Some((end, _caption)) = rest.split_once('}') else {
        return false;
    };
    !start.is_empty()
        && !end.is_empty()
        && start.bytes().all(|byte| byte.is_ascii_digit())
        && end.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grant_requires_real_file_and_is_revocable() {
        let grants = FileGrants::default();
        assert!(
            grants
                .grant(Path::new("/path/that/does/not/exist"))
                .is_err()
        );
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let selected = grants.grant(&file).unwrap();
        let mut handle = grants.open_scoped(&selected.token).unwrap();
        let mut header = [0u8; 9];
        handle.read_exact(&mut header).unwrap();
        assert_eq!(&header, b"[package]");
        assert_eq!(
            grants.resolve(&selected.token).unwrap(),
            fs::canonicalize(file).unwrap()
        );
        grants.revoke(&selected.token);
        assert!(matches!(
            grants.resolve(&selected.token),
            Err(GrantError::UnknownGrant)
        ));
    }

    #[test]
    fn subtitle_grants_require_extension_and_recognizable_content() {
        let directory = tempfile::tempdir().unwrap();
        let grants = FileGrants::default();
        let cases = [
            ("captions.srt", "1\n00:00:01,000 --> 00:00:02,250\nHello\n"),
            ("captions.vtt", "WEBVTT\n\n00:01.000 --> 00:02.000\nHello\n"),
            (
                "captions.ass",
                "[Script Info]\nTitle: Test\n[Events]\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hello\n",
            ),
            (
                "captions.ssa",
                "[Script Info]\n[Events]\nDialogue: Marked=0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hello\n",
            ),
            ("captions.sub", "{25}{50}Hello\n"),
        ];
        for (name, contents) in cases {
            let path = directory.path().join(name);
            fs::write(&path, contents).unwrap();
            let selected = grants.grant(&path).unwrap();
            assert_eq!(selected.mime, "file/subtitle", "{name}");
        }

        let misleading = directory.path().join("not-really-a-subtitle.srt");
        fs::write(&misleading, "ordinary text with no cue timing\n").unwrap();
        assert_eq!(grants.grant(&misleading).unwrap().mime, "file/octet-stream");

        let unrecognized_extension = directory.path().join("captions.txt");
        fs::write(
            &unrecognized_extension,
            "1\n00:00:01,000 --> 00:00:02,000\nHello\n",
        )
        .unwrap();
        assert_eq!(
            grants.grant(&unrecognized_extension).unwrap().mime,
            "file/octet-stream"
        );
    }

    #[test]
    fn html_grants_use_content_signatures_not_extensions() {
        let directory = tempfile::tempdir().unwrap();
        let grants = FileGrants::default();
        let html = directory.path().join("snapshot.data");
        fs::write(
            &html,
            "<!doctype html><html><body><h1>Saved</h1></body></html>",
        )
        .unwrap();
        assert_eq!(grants.grant(&html).unwrap().mime, "file/html");

        let mhtml = directory.path().join("snapshot.bin");
        fs::write(
            &mhtml,
            "MIME-Version: 1.0\nContent-Type: multipart/related; boundary=arcade\n\n--arcade\n",
        )
        .unwrap();
        assert_eq!(grants.grant(&mhtml).unwrap().mime, "file/mhtml");

        let misleading = directory.path().join("snapshot.html");
        fs::write(&misleading, "this is plain text, not an html document").unwrap();
        assert_eq!(grants.grant(&misleading).unwrap().mime, "file/octet-stream");
    }

    #[test]
    fn zint_generated_svg_grants_use_svg_mime_with_known_public_doctype() {
        let directory = tempfile::tempdir().unwrap();
        let svg = directory.path().join("generated.dat");
        fs::write(
            &svg,
            concat!(
                "<?xml version=\"1.0\" standalone=\"no\"?>\n",
                "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n",
                "<svg width=\"24\" height=\"24\" version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\"><g/></svg>"
            ),
        )
        .unwrap();
        let grants = FileGrants::default();
        assert_eq!(grants.grant(&svg).unwrap().mime, "file/svg");

        let unrecognized = directory.path().join("unrecognized.dat");
        fs::write(
            &unrecognized,
            "<!DOCTYPE svg SYSTEM \"https://example.invalid/evil.dtd\"><svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )
        .unwrap();
        assert_eq!(
            grants.grant(&unrecognized).unwrap().mime,
            "file/octet-stream"
        );
    }

    #[test]
    fn output_grant_streams_without_overwrite_and_rejects_paths() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("chosen");
        let staging = root.path().join("staging");
        fs::create_dir(&destination).unwrap();
        fs::create_dir(&staging).unwrap();
        let staged = staging.join("result.txt");
        fs::write(&staged, b"first result").unwrap();

        let grants = FileGrants::default();
        let folder = grants.grant_output_directory(&destination).unwrap();
        let first = grants
            .publish_staged_output(
                &folder.token,
                &staged,
                "result.txt",
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(first.name, "result.txt");
        assert_eq!(
            fs::read(destination.join(&first.name)).unwrap(),
            b"first result"
        );

        fs::write(&staged, b"second result").unwrap();
        let second = grants
            .publish_staged_output(
                &folder.token,
                &staged,
                "result.txt",
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(second.name, "result-2.txt");
        assert_eq!(
            fs::read(destination.join(&first.name)).unwrap(),
            b"first result"
        );
        assert_eq!(
            fs::read(destination.join(&second.name)).unwrap(),
            b"second result"
        );

        assert!(matches!(
            grants.publish_staged_output(
                &folder.token,
                &staged,
                "../escape.txt",
                &std::sync::atomic::AtomicBool::new(false),
            ),
            Err(GrantError::UnsafeOutputName(_))
        ));
        assert!(!root.path().join("escape.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn scoped_handle_rejects_replaced_file_even_with_same_size_and_time() {
        let directory = tempfile::tempdir().unwrap();
        let chosen = directory.path().join("chosen.txt");
        let replacement = directory.path().join("replacement.txt");
        fs::write(&chosen, b"one").unwrap();
        fs::write(&replacement, b"two").unwrap();
        let grants = FileGrants::default();
        let selected = grants.grant(&chosen).unwrap();
        let selected_time = fs::metadata(&chosen).unwrap().modified().unwrap();
        File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(selected_time)
            .unwrap();
        fs::rename(replacement, chosen).unwrap();
        assert!(matches!(
            grants.open_scoped(&selected.token),
            Err(GrantError::FileChanged)
        ));
    }
}
