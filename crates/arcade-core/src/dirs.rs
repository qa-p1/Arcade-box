//! Box's own folders (STANDARD §10): `arcade/box` from Arcade Link's paths
//! module, rooted at `ARCADE_HOME` in isolated runs with the same layout as
//! Arcade Tools' isolation. The desktop app, its one-shot Link mode and the
//! terminal tool all share them, and all call [`prepare`] before opening the
//! database so that none of them starts an empty profile beside an
//! unmigrated one.

use arcade_link::accelerator::Platform;
use arcade_link::paths::{self, AppDirs, Migration, UserDirs};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub const APP_ID: &str = arcade_link::ids::BOX;
/// The folder name Box used before 0.4.0, also directly under `ARCADE_HOME`.
const OLD_FOLDER: &str = "dev.arcadebox.app";
/// The lock a running desktop app holds in its data folder.
pub const DESKTOP_LOCK: &str = "desktop.lock";

/// `ARCADE_HOME`, when set. A relative value is an error rather than a
/// reason to fall back to the real profile.
fn isolated_root() -> Result<Option<PathBuf>, String> {
    match env::var_os("ARCADE_HOME").filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(root) => {
            let root = PathBuf::from(root);
            if root.is_absolute() {
                Ok(Some(root))
            } else {
                Err(format!("ARCADE_HOME must be absolute: {}", root.display()))
            }
        }
    }
}

fn user_dirs() -> Result<UserDirs, String> {
    Ok(match isolated_root()? {
        Some(root) => UserDirs::under(&root),
        None => UserDirs::discover(),
    })
}

/// Box's config, data and cache folders.
pub fn app_dirs() -> Option<AppDirs> {
    let user = user_dirs().ok()?;
    Some(paths::app_dirs_in(&user, paths::current_os(), APP_ID))
}

/// The data folder: database, logs, models, user tools and keys.
pub fn data_dir() -> Option<PathBuf> {
    app_dirs().map(|dirs| dirs.data)
}

/// Every folder Box kept its data in before 0.4.0, existing or not.
fn old_folders(user: &UserDirs) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = paths::legacy_dirs_in(user, paths::current_os(), APP_ID)
        .into_iter()
        .map(|legacy| legacy.path)
        .collect();
    if let Ok(Some(root)) = isolated_root() {
        folders.push(root.join(OLD_FOLDER));
    }
    folders
}

/// Every folder Box's files can be in, new and old, for "remove my data".
pub fn all_folders() -> Vec<PathBuf> {
    let Ok(user) = user_dirs() else {
        return Vec::new();
    };
    let dirs = paths::app_dirs_in(&user, paths::current_os(), APP_ID);
    let mut folders = vec![dirs.data, dirs.config, dirs.cache];
    folders.extend(old_folders(&user));
    // WebView storage before 0.4.0, named after the old identifier.
    let library = user.home.join("Library");
    folders.extend(match paths::current_os() {
        Platform::Linux => vec![user.cache.join(OLD_FOLDER)],
        Platform::Windows => vec![user.local.join(OLD_FOLDER)],
        Platform::Macos => vec![
            library.join("Caches").join(OLD_FOLDER),
            library.join("WebKit").join(OLD_FOLDER),
        ],
    });
    folders.sort();
    folders.dedup();
    folders
}

/// Moves the folders of an earlier Box into `arcade/box` (Link's copy,
/// verify, then remove), then returns the data folder. Refuses while an
/// earlier Box is still running from an old folder, since moving the files
/// under it would lose what it writes next. Safe to call on every start.
pub fn prepare() -> Result<(PathBuf, Migration), String> {
    let user = user_dirs()?;
    for folder in old_folders(&user) {
        if held_by_running_app(&folder.join(DESKTOP_LOCK)) {
            return Err(format!(
                "an earlier Arcade Box is still running from {}; quit it, then start Arcade Box again",
                folder.display()
            ));
        }
    }
    let platform = paths::current_os();
    let dirs = paths::app_dirs_in(&user, platform, APP_ID);
    let mut report = paths::migrate_app_dirs_in(&user, platform, APP_ID);
    if let Ok(Some(root)) = isolated_root() {
        move_isolated_folder(&root.join(OLD_FOLDER), &dirs.data, &mut report);
    }
    // The old desktop lock means nothing in the new folder.
    let stale = dirs.data.join(DESKTOP_LOCK);
    if report.moved.iter().any(|(_, to)| to == &dirs.data) && !held_by_running_app(&stale) {
        let _ = fs::remove_file(stale);
    }
    Ok((dirs.data, report))
}

/// Isolated runs before 0.4.0 kept data at `$ARCADE_HOME/dev.arcadebox.app`,
/// outside the layout Link's table describes. A rename on the same disk moves
/// it whole or not at all.
fn move_isolated_folder(from: &Path, to: &Path, report: &mut Migration) {
    if !from.is_dir() {
        return;
    }
    if to.exists() {
        report
            .kept
            .push((from.into(), format!("{} already exists", to.display())));
        return;
    }
    let moved = to
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::rename(from, to));
    match moved {
        Ok(()) => report.moved.push((from.into(), to.into())),
        Err(error) => report.kept.push((from.into(), error.to_string())),
    }
}

/// Whether another process holds `lock` (the desktop app's lock file).
fn held_by_running_app(lock: &Path) -> bool {
    let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(lock) else {
        return false;
    };
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => {
            let _ = fs2::FileExt::unlock(&file);
            false
        }
        // EWOULDBLOCK on Unix, ERROR_LOCK_VIOLATION on Windows.
        Err(error) => error.raw_os_error() == fs2::lock_contended_error().raw_os_error(),
    }
}

/// One line per migrated or kept folder, for the app's log.
pub fn describe(report: &Migration) -> Vec<String> {
    let moved = report
        .moved
        .iter()
        .map(|(from, to)| format!("Arcade Box moved {} to {}", from.display(), to.display()));
    let kept = report
        .kept
        .iter()
        .map(|(from, reason)| format!("Arcade Box left {} in place: {reason}", from.display()));
    moved.chain(kept).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("arcade-box-dirs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn isolated_folder_moves_whole() {
        let root = temp("move");
        let from = root.join(OLD_FOLDER);
        fs::create_dir_all(from.join("logs")).unwrap();
        fs::write(from.join("arcade.sqlite3"), b"db").unwrap();
        let to = root.join(".local/share/arcade/box");
        let mut report = Migration::default();
        move_isolated_folder(&from, &to, &mut report);
        assert_eq!(report.moved, vec![(from.clone(), to.clone())]);
        assert!(!from.exists());
        assert_eq!(fs::read(to.join("arcade.sqlite3")).unwrap(), b"db");
        assert!(to.join("logs").is_dir());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn isolated_folder_never_overwrites() {
        let root = temp("keep");
        let from = root.join(OLD_FOLDER);
        let to = root.join(".local/share/arcade/box");
        fs::create_dir_all(&from).unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(from.join("arcade.sqlite3"), b"old").unwrap();
        fs::write(to.join("arcade.sqlite3"), b"new").unwrap();
        let mut report = Migration::default();
        move_isolated_folder(&from, &to, &mut report);
        assert!(report.moved.is_empty());
        assert_eq!(report.kept.len(), 1);
        assert_eq!(fs::read(from.join("arcade.sqlite3")).unwrap(), b"old");
        assert_eq!(fs::read(to.join("arcade.sqlite3")).unwrap(), b"new");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_held_lock_is_seen() {
        let root = temp("lock");
        let lock = root.join(DESKTOP_LOCK);
        assert!(!held_by_running_app(&lock), "a missing lock is not held");
        let file = fs::File::create(&lock).unwrap();
        assert!(!held_by_running_app(&lock));
        fs2::FileExt::lock_exclusive(&file).unwrap();
        // fs2 locks are per open file description, so a second open sees it.
        assert!(held_by_running_app(&lock));
        drop(file);
        assert!(!held_by_running_app(&lock));
        fs::remove_dir_all(root).ok();
    }
}
