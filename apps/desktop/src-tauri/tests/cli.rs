//! The command line in Link's ARCADE_APP.md (CLI-1..4), run against the built binary in an empty profile with no
//! display. `known_bug_*` tests pin findings not fixed yet (docs/v1/DECISIONS.md D3).
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn run(home: &Path, args: &[&str]) -> Option<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_arcade-desktop"))
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("ARCADE_HOME", home.join(".local/share/arcade"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if child.try_wait().unwrap().is_some() {
            return Some(child.wait_with_output().unwrap());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

fn profile() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("run")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            home.path().join("run"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    home
}

fn files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.push(path.strip_prefix(root).unwrap().display().to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn version_and_help() {
    let home = profile();
    let version = run(home.path(), &["--version"]).expect("--version hung");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("Arcade Box {}", env!("CARGO_PKG_VERSION"))
    );
    let help = run(home.path(), &["--help"]).expect("--help hung");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("arcade-desktop --status"));
}

#[test]
fn arcade_manifest_prints_json_and_creates_nothing() {
    let home = profile();
    let before = files(home.path());
    let out = run(home.path(), &["--arcade-manifest"]).expect("--arcade-manifest hung");
    assert!(out.status.success());
    let manifest: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(manifest["id"], "arcade.box");
    assert_eq!(
        before,
        files(home.path()),
        "--arcade-manifest created files"
    );
}

#[test]
fn status_prints_one_json_line_without_starting_box() {
    let home = profile();
    let out = run(home.path(), &["--status"]).expect("CLI-1: --status started Box");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        matches!(out.status.code(), Some(0 | 3)) && text.lines().count() == 1,
        "CLI-1: --status exited {:?} and printed {text:?}",
        out.status.code()
    );
    let status: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(status["app"], "arcade.box");
    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status["running"], false);
}

#[test]
#[should_panic(expected = "KNOWN BUG CLI-2")]
fn known_bug_an_unknown_argument_prints_usage_and_exits_2() {
    let home = profile();
    let out = run(home.path(), &["--no-such-option"]);
    let ok = out.as_ref().is_some_and(|out| {
        out.status.code() == Some(2)
            && String::from_utf8_lossy(&out.stderr).contains("arcade-desktop --status")
    });
    assert!(
        ok,
        "KNOWN BUG CLI-2: {:?}",
        out.map(|out| out.status.code())
    );
}
