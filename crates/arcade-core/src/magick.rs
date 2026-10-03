//! A verified ImageMagick 7 (`magick`) provider shared by image and PDF
//! tools. Callers stage inputs under fixed names in a private folder and run
//! with that folder as the working directory, so user paths never reach
//! ImageMagick's command line (which interprets `[frame]`, `format:` and
//! `@file` syntax).

use crate::{
    process::{self, ProcessSpec},
    provider::find_system_executable,
    tool_kit::check_cancelled,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

pub(crate) struct Magick {
    executable: PathBuf,
}

/// Bold sans fonts tried, in order, for overlay text.
const BOLD_FONTS: [&str; 5] = [
    "DejaVu-Sans-Bold",
    "Noto-Sans-Bold",
    "Liberation-Sans-Bold",
    "Arial-Bold",
    "Helvetica-Bold",
];

impl Magick {
    pub(crate) fn discover() -> Result<Self, String> {
        let executable = find_system_executable("magick")
            .filter(|path| {
                process::run(
                    &ProcessSpec {
                        executable: path.clone(),
                        args: vec!["-version".into()],
                        current_dir: None,
                        timeout: Duration::from_secs(10),
                        output_limit: 64 * 1024,
                    },
                    &AtomicBool::new(false),
                )
                .is_ok_and(|output| {
                    String::from_utf8_lossy(&output.stdout).contains("ImageMagick 7")
                })
            })
            .ok_or("This tool needs ImageMagick 7 (the `magick` command)")?;
        Ok(Self { executable })
    }

    pub(crate) fn run(
        &self,
        dir: &Path,
        args: Vec<OsString>,
        cancelled: &AtomicBool,
    ) -> Result<String, String> {
        check_cancelled(cancelled)?;
        // Resource limits go after a subcommand such as `identify`.
        let mut args = args.into_iter().peekable();
        let mut full: Vec<OsString> = Vec::new();
        if args
            .peek()
            .is_some_and(|first| first == "identify" || first == "montage")
        {
            full.extend(args.next());
        }
        // `-list` must come first and reads no images.
        if args.peek().is_none_or(|first| first != "-list") {
            full.extend(["-limit", "memory", "2GiB", "-limit", "disk", "8GiB"].map(OsString::from));
        }
        full.extend(args);
        let output = process::run(
            &ProcessSpec {
                executable: self.executable.clone(),
                args: full,
                current_dir: Some(dir.to_path_buf()),
                timeout: Duration::from_secs(30 * 60),
                output_limit: 1024 * 1024,
            },
            cancelled,
        )
        .map_err(|error| error.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "ImageMagick failed: {}",
                stderr
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("unknown error")
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The first installed font from [`BOLD_FONTS`].
    pub(crate) fn bold_font(&self, dir: &Path, cancelled: &AtomicBool) -> Option<&'static str> {
        let list = self.run(dir, args(["-list", "font"]), cancelled).ok()?;
        BOLD_FONTS.into_iter().find(|font| {
            list.lines()
                .any(|line| line.trim().strip_prefix("Font: ") == Some(font))
        })
    }

    /// Width and height of the first frame of a staged image.
    pub(crate) fn size(
        &self,
        dir: &Path,
        name: &str,
        cancelled: &AtomicBool,
    ) -> Result<(u32, u32), String> {
        let text = self.run(
            dir,
            vec![
                "identify".into(),
                "-format".into(),
                "%w %h".into(),
                format!("{name}[0]").into(),
            ],
            cancelled,
        )?;
        let mut parts = text.split_whitespace().map(|part| part.parse::<u32>());
        match (parts.next(), parts.next()) {
            (Some(Ok(width)), Some(Ok(height))) if width > 0 && height > 0 => Ok((width, height)),
            _ => Err("Could not read the image size".into()),
        }
    }
}

pub(crate) fn args<const N: usize>(items: [&str; N]) -> Vec<OsString> {
    items.into_iter().map(OsString::from).collect()
}

/// Text for `-annotate`: ImageMagick expands `%` escapes and reads a file for
/// a leading `@`, so both are neutralised.
pub(crate) fn literal_text(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Type the watermark text".into());
    }
    if text.chars().count() > 200 {
        return Err("Keep the watermark under 200 characters".into());
    }
    let escaped = text.replace('\\', "\\\\").replace('%', "%%");
    Ok(if escaped.starts_with('@') {
        format!("\\{escaped}")
    } else {
        escaped
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_text_cannot_read_files_or_expand_escapes() {
        assert_eq!(literal_text("@/etc/passwd").unwrap(), "\\@/etc/passwd");
        assert_eq!(literal_text("100% mine").unwrap(), "100%% mine");
        assert_eq!(literal_text(r"a\b").unwrap(), r"a\\b");
        assert!(literal_text("  ").is_err());
    }
}
