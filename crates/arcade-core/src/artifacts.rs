//! Atomic, non-destructive publication of staged file results.

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Create or open Arcade Box's persistent private artifact directory. The
/// fixed child is resolved through a capability for its parent, must not be a
/// symlink, and is locked to the current user's permissions on Unix.
pub fn prepare_private_artifact_root(parent_path: &Path) -> io::Result<PathBuf> {
    use cap_std::{
        ambient_authority,
        fs::{Dir, DirBuilder, DirBuilderExt},
    };

    let parent = fs::canonicalize(parent_path)?;
    let parent_meta = fs::metadata(&parent)?;
    if !parent_meta.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "application data parent is not a directory",
        ));
    }
    let parent_dir = Dir::open_ambient_dir(&parent, ambient_authority())?;
    let name = Path::new("job-artifacts");
    match parent_dir.symlink_metadata(name) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "job-artifacts must be a real directory, not a link",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut builder = DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match parent_dir.create_dir_with(name, &builder) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    }
    let artifact_dir = parent_dir.open_dir(name)?;
    let metadata = artifact_dir.dir_metadata()?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "job-artifacts is not a directory",
        ));
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        use cap_std::fs::PermissionsExt;
        let current_uid = unsafe { libc::geteuid() };
        if metadata.uid() != current_uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "job-artifacts is not owned by the current user",
            ));
        }
        artifact_dir.set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))?;
    }
    let canonical = fs::canonicalize(parent.join(name))?;
    let expected_path = parent.join(name);
    if canonical != expected_path || fs::symlink_metadata(&canonical)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "job-artifacts changed while opening it",
        ));
    }
    let path_meta = fs::metadata(&canonical)?;
    let handle_meta = artifact_dir.dir_metadata()?;
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt as CapMetadataExt;
        use std::os::unix::fs::MetadataExt;
        if path_meta.dev() != handle_meta.dev() || path_meta.ino() != handle_meta.ino() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "job-artifacts path no longer names the opened directory",
            ));
        }
    }
    Ok(canonical)
}

pub fn validate_portable_filename(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 240 || name != name.trim() {
        return Err(
            "Choose a filename between 1 and 240 characters without surrounding spaces".into(),
        );
    }
    if name.chars().any(|ch| {
        ch.is_control() || matches!(ch, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
    }) || name.ends_with('.')
    {
        return Err("Filename contains characters that are unsafe on desktop platforms".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0'
    {
        return Err("Filename is reserved by Windows".into());
    }
    Ok(())
}

/// Publish a completed staged file without replacing any existing file. A
/// same-filesystem hard link gives an atomic create-if-absent operation; the
/// copy fallback also uses `create_new` and removes a partial file on error.
pub fn publish_without_overwrite(
    staged: &Path,
    output_directory: &Path,
    requested_name: &str,
    cancelled: &AtomicBool,
) -> io::Result<PathBuf> {
    validate_portable_filename(requested_name)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let name_path = Path::new(requested_name);
    let stem = name_path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = name_path.extension().map(|value| value.to_string_lossy());
    for number in 1..=10_000 {
        if cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Job cancelled"));
        }
        let candidate = if number == 1 {
            requested_name.to_string()
        } else if let Some(extension) = &extension {
            format!("{stem}-{number}.{extension}")
        } else {
            format!("{stem}-{number}")
        };
        let destination = output_directory.join(candidate);
        match fs::hard_link(staged, &destination) {
            Ok(()) => return Ok(destination),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Unsupported
                        | io::ErrorKind::PermissionDenied
                        | io::ErrorKind::CrossesDevices
                ) =>
            {
                let mut source = fs::File::open(staged)?;
                let mut target = match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)
                {
                    Ok(file) => file,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                };
                let copied = (|| -> io::Result<()> {
                    let mut buffer = [0u8; 1024 * 1024];
                    loop {
                        if cancelled.load(Ordering::Relaxed) {
                            return Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "Job cancelled",
                            ));
                        }
                        let count = source.read(&mut buffer)?;
                        if count == 0 {
                            break;
                        }
                        target.write_all(&buffer[..count])?;
                    }
                    target.flush()
                })();
                if let Err(error) = copied {
                    drop(target);
                    let _ = fs::remove_file(&destination);
                    return Err(error);
                }
                return Ok(destination);
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Too many files with this name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_unsafe_names_and_never_overwrites() {
        assert!(validate_portable_filename("../secret.pdf").is_err());
        assert!(validate_portable_filename("CON.pdf").is_err());
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("staged");
        fs::write(&source, b"new").unwrap();
        fs::write(dir.path().join("report.pdf"), b"old").unwrap();
        let output =
            publish_without_overwrite(&source, dir.path(), "report.pdf", &AtomicBool::new(false))
                .unwrap();
        assert_eq!(output.file_name().unwrap(), "report-2.pdf");
        assert_eq!(fs::read(dir.path().join("report.pdf")).unwrap(), b"old");
        assert_eq!(fs::read(output).unwrap(), b"new");
    }
}
