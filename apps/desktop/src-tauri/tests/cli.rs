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
fn an_unknown_argument_prints_usage_and_exits_2() {
    let home = profile();
    for args in [
        &["--no-such-option"][..],
        &["--show", "extra"],
        &["--install", "--channel"],
    ] {
        let out = run(home.path(), args).expect("arcade-desktop ran");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("arcade-desktop --status"),
            "{args:?}: no usage on stderr"
        );
    }
    assert_eq!(
        files(home.path()),
        ["run"],
        "a refused argument created files"
    );
}

/// RT-4: a fatal signal leaves one report in Box's crash folder, which nothing created before the crash.
#[cfg(target_os = "linux")]
#[test]
fn a_fatal_signal_leaves_a_crash_report() {
    use std::os::unix::process::ExitStatusExt;
    let home = profile();
    let crashes = home.path().join(".local/share/arcade/box/crashes");
    let mut child = Command::new(env!("CARGO_BIN_EXE_arcade-desktop"))
        .arg("--arcade-invoke")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.path())
        .env("ARCADE_HOME", home.path().join(".local/share/arcade"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // The handler is installed first thing in main; once Box blocks reading its request from stdin it is in place.
    let reading = |pid: u32| {
        std::fs::read_to_string(format!("/proc/{pid}/syscall")).is_ok_and(|s| {
            let mut fields = s.split_whitespace();
            let (call, fd) = (fields.next(), fields.next());
            // read(2) is 0 on x86_64 and 63 on aarch64; the first argument is fd 0.
            matches!(call, Some("0" | "63")) && fd == Some("0x0")
        })
    };
    let start = Instant::now();
    while !reading(child.id()) {
        assert!(
            child.try_wait().unwrap().is_none(),
            "--arcade-invoke exited early"
        );
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "--arcade-invoke never read stdin"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !crashes.exists(),
        "the crash folder exists before any crash"
    );
    unsafe { libc::kill(child.id() as i32, libc::SIGSEGV) };
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGSEGV), "{status:?}");
    let reports: Vec<_> = std::fs::read_dir(&crashes)
        .expect("no crash folder")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "crash"))
        .collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let text = std::fs::read_to_string(&reports[0]).unwrap();
    for needle in [
        "app: Arcade Box",
        concat!("version: ", env!("CARGO_PKG_VERSION")),
        "signal: 11",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in\n{text}");
    }
}
