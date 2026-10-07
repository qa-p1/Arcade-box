//! Pure candidate construction. Environment and directory reads stay at the
//! discovery edge; Windows/macOS layouts are testable on a Linux runner.
use std::path::PathBuf;

#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) enum Platform {
    Linux,
    Windows,
    Macos,
}

#[derive(Default)]
pub(super) struct Roots {
    pub path: Vec<PathBuf>,
    pub program_files: Vec<PathBuf>,
    pub system_root: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub home: Option<PathBuf>,
    /// Existing program/package roots, enumerated by the platform edge.
    pub installed: Vec<(PathBuf, &'static str)>,
    pub managed: Vec<PathBuf>,
}

pub(super) fn candidates(
    platform: Platform,
    stem: &str,
    roots: &Roots,
) -> Vec<(PathBuf, &'static str)> {
    // Names are fixed by callers, never a path supplied by a peer or a form.
    if stem.is_empty() || stem.contains(['/', '\\', ':']) || stem == "." || stem == ".." {
        return vec![];
    }
    let mut dirs: Vec<(PathBuf, &'static str)> =
        roots.path.iter().cloned().map(|p| (p, "path")).collect();
    match platform {
        Platform::Linux => dirs.extend(
            [
                "/usr/bin",
                "/usr/local/bin",
                "/opt/homebrew/bin",
                "/opt/local/bin",
            ]
            .map(|p| (p.into(), "system")),
        ),
        Platform::Macos => {
            dirs.extend(
                [
                    "/opt/homebrew/bin",
                    "/usr/local/bin",
                    "/usr/bin",
                    "/opt/local/bin",
                ]
                .map(|p| (p.into(), "system")),
            );
            dirs.push((
                PathBuf::from("/Applications").join(format!("{stem}.app/Contents/MacOS")),
                "application",
            ));
        }
        Platform::Windows => {
            if let Some(root) = &roots.system_root {
                dirs.push((root.join("System32"), "system"));
            }
            for root in &roots.program_files {
                for name in [stem, application_folder(stem)] {
                    let folder = root.join(name);
                    dirs.push((folder.clone(), "system"));
                    dirs.push((folder.join("bin"), "system"));
                }
                dirs.push((root.join("Git/mingw64/bin"), "system"));
            }
            if let Some(local) = &roots.local_app_data {
                dirs.push((local.join("Microsoft/WinGet/Links"), "winget"));
            }
            if let Some(home) = &roots.home {
                dirs.push((home.join("scoop/shims"), "scoop"));
                dirs.push((home.join("scoop/apps").join(stem).join("current"), "scoop"));
                dirs.push((
                    home.join("scoop/apps").join(stem).join("current/bin"),
                    "scoop",
                ));
            }
        }
    }
    for (root, source) in &roots.installed {
        dirs.push((root.clone(), *source));
        for child in ["bin", "program", "Library/bin", "Contents/MacOS"] {
            dirs.push((root.join(child), *source));
        }
    }
    dirs.extend(roots.managed.iter().cloned().map(|p| (p, "user")));
    let names: Vec<String> = match platform {
        Platform::Windows if stem == "gs" => vec![
            "gswin64c.exe".into(),
            "gswin32c.exe".into(),
            "gs.exe".into(),
        ],
        Platform::Windows => vec![format!("{stem}.exe")],
        _ => vec![stem.into()],
    };
    let mut seen = std::collections::HashSet::new();
    dirs.into_iter()
        .flat_map(|(dir, source)| names.iter().map(move |name| (dir.join(name), source)))
        .filter(|(path, _)| seen.insert(path.clone()))
        .collect()
}

fn application_folder(stem: &str) -> &str {
    match stem {
        "ffprobe" => "ffmpeg",
        "vipsheader" => "vips",
        "tesseract" => "Tesseract-OCR",
        "soffice" | "libreoffice" => "LibreOffice/program",
        "magick" => "ImageMagick",
        "chrome" | "chromium" => "Google/Chrome/Application",
        _ => stem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_paths_include_both_program_files_winget_and_scoop() {
        let roots = Roots {
            path: vec!["C:/custom/bin".into()],
            program_files: vec!["C:/Program Files".into(), "C:/Program Files (x86)".into()],
            system_root: Some("C:/Windows".into()),
            local_app_data: Some("C:/Users/A/AppData/Local".into()),
            home: Some("C:/Users/A".into()),
            installed: vec![
                (
                    "C:/Users/A/AppData/Local/Microsoft/WinGet/Packages/Gyan.FFmpeg/ffmpeg-8"
                        .into(),
                    "winget",
                ),
                ("C:/Users/A/scoop/apps/ffmpeg/current".into(), "scoop"),
            ],
            ..Default::default()
        };
        let paths = candidates(Platform::Windows, "ffmpeg", &roots);
        assert_eq!(
            paths[0],
            (PathBuf::from("C:/custom/bin/ffmpeg.exe"), "path")
        );
        for path in [
            "C:/Program Files/ffmpeg/bin/ffmpeg.exe",
            "C:/Program Files (x86)/ffmpeg/bin/ffmpeg.exe",
            "C:/Windows/System32/ffmpeg.exe",
            "C:/Users/A/AppData/Local/Microsoft/WinGet/Links/ffmpeg.exe",
            "C:/Users/A/AppData/Local/Microsoft/WinGet/Packages/Gyan.FFmpeg/ffmpeg-8/bin/ffmpeg.exe",
            "C:/Users/A/scoop/shims/ffmpeg.exe",
            "C:/Users/A/scoop/apps/ffmpeg/current/bin/ffmpeg.exe",
        ] {
            assert!(
                paths.iter().any(|(p, _)| p == &PathBuf::from(path)),
                "missing {path}"
            );
        }
        assert!(
            candidates(Platform::Windows, "tesseract", &roots)
                .iter()
                .any(|(p, _)| p == &PathBuf::from("C:/Program Files/Tesseract-OCR/tesseract.exe"))
        );
        assert!(
            candidates(Platform::Windows, "gs", &roots)
                .iter()
                .any(|(p, _)| p.file_name().unwrap() == "gswin64c.exe")
        );
    }
    #[test]
    fn macos_checks_homebrew_and_installed_app_bundles() {
        let roots = Roots {
            installed: vec![("/Applications/LibreOffice.app".into(), "application")],
            ..Default::default()
        };
        let paths = candidates(Platform::Macos, "soffice", &roots);
        for path in [
            "/opt/homebrew/bin/soffice",
            "/usr/local/bin/soffice",
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        ] {
            assert!(paths.iter().any(|(p, _)| p == &PathBuf::from(path)));
        }
    }
    #[test]
    fn candidates_are_deduplicated_and_reject_path_names() {
        let roots = Roots {
            path: vec!["/usr/bin".into(), "/usr/bin".into()],
            managed: vec!["/usr/bin".into()],
            ..Default::default()
        };
        let paths = candidates(Platform::Linux, "vips", &roots);
        assert_eq!(
            paths
                .iter()
                .filter(|(p, _)| p == &PathBuf::from("/usr/bin/vips"))
                .count(),
            1
        );
        for name in ["", "../vips", "bin\\vips", "C:vips", ".."] {
            assert!(candidates(Platform::Windows, name, &roots).is_empty());
        }
    }
}
