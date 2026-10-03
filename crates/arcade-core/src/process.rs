use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

pub type StdoutLineHandler = Arc<dyn Fn(&str) + Send + Sync + 'static>;

/// Only the provider broker constructs these specifications. Paths and
/// arguments remain separate all the way to the operating system.
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub timeout: Duration,
    pub output_limit: usize,
}

pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub elapsed: Duration,
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("provider executable path must be absolute")]
    RelativeExecutable,
    #[error("provider process failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider output exceeded the allowed capture size")]
    OutputLimit,
    #[error("provider input exceeded the allowed transfer size")]
    InputLimit,
    #[error("provider process timed out")]
    Timeout,
    #[error("provider process cancelled")]
    Cancelled,
    #[error("provider output collector failed")]
    Collector,
    #[error("could not contain provider process tree: {0}")]
    JobObject(#[source] std::io::Error),
}

pub fn run(spec: &ProcessSpec, cancelled: &AtomicBool) -> Result<ProcessOutput, ProcessError> {
    run_inner(spec, cancelled, None, &[], None)
}

/// Run a provider with a small, explicit set of environment overrides. The
/// inherited environment remains cleared; callers must pass only scoped
/// values required by that provider.
pub fn run_with_env(
    spec: &ProcessSpec,
    cancelled: &AtomicBool,
    environment: &[(OsString, OsString)],
) -> Result<ProcessOutput, ProcessError> {
    run_inner(spec, cancelled, None, environment, None)
}

/// Send a bounded request to a trusted, short-lived worker over stdin.
/// The process still receives no inherited stdin or ambient environment.
pub fn run_with_input(
    spec: &ProcessSpec,
    cancelled: &AtomicBool,
    input: &[u8],
) -> Result<ProcessOutput, ProcessError> {
    if input.len() > 8 * 1024 * 1024 {
        return Err(ProcessError::InputLimit);
    }
    run_inner(spec, cancelled, None, &[], Some(input))
}

/// Observe bounded text lines from stdout while retaining the same process
/// lifetime, cancellation, and capture limits as `run`. Providers can use
/// this for structured progress without putting log parsing in the UI.
pub fn run_with_stdout_lines(
    spec: &ProcessSpec,
    cancelled: &AtomicBool,
    on_line: StdoutLineHandler,
) -> Result<ProcessOutput, ProcessError> {
    run_inner(spec, cancelled, Some(on_line), &[], None)
}

fn run_inner(
    spec: &ProcessSpec,
    cancelled: &AtomicBool,
    on_line: Option<StdoutLineHandler>,
    environment: &[(OsString, OsString)],
    input: Option<&[u8]>,
) -> Result<ProcessOutput, ProcessError> {
    if !spec.executable.is_absolute() {
        return Err(ProcessError::RelativeExecutable);
    }
    let mut command = Command::new(&spec.executable);
    command
        .args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    command.env_clear().env("LANG", "C");
    command.envs(environment.iter().cloned());
    if let Some(path) = trusted_provider_path(&spec.executable) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    if let Some(directory) = &spec.current_dir {
        command.current_dir(directory);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Keep the provider suspended until it is assigned to a Job Object,
        // closing the window in which it could create an untracked child.
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    #[cfg(windows)]
    let windows_job = WindowsJobObject::create().map_err(ProcessError::JobObject)?;
    let started = Instant::now();
    let mut child = command.spawn()?;
    #[cfg(windows)]
    {
        if let Err(error) = windows_job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessError::JobObject(error));
        }
        if let Err(error) = resume_suspended_primary_thread(child.id()) {
            let _ = windows_job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessError::JobObject(error));
        }
    }
    let stdout = child.stdout.take().ok_or(ProcessError::Collector)?;
    let stderr = child.stderr.take().ok_or(ProcessError::Collector)?;
    let stdin_writer = if let Some(input) = input {
        let mut stdin = child.stdin.take().ok_or(ProcessError::Collector)?;
        let bytes = input.to_vec();
        Some(thread::spawn(move || stdin.write_all(&bytes)))
    } else {
        None
    };
    let limit = spec.output_limit;
    let stdout_reader = thread::spawn(move || collect_bounded_with_lines(stdout, limit, on_line));
    let stderr_reader = thread::spawn(move || collect_bounded(stderr, limit));
    let status = loop {
        if cancelled.load(Ordering::Relaxed) {
            #[cfg(windows)]
            terminate_tree(&mut child, &windows_job);
            #[cfg(not(windows))]
            terminate_tree(&mut child);
            let _ = child.wait();
            #[cfg(windows)]
            drop(windows_job);
            join_reader(stdout_reader)?;
            join_reader(stderr_reader)?;
            join_writer(stdin_writer)?;
            return Err(ProcessError::Cancelled);
        }
        if started.elapsed() > spec.timeout {
            #[cfg(windows)]
            terminate_tree(&mut child, &windows_job);
            #[cfg(not(windows))]
            terminate_tree(&mut child);
            let _ = child.wait();
            #[cfg(windows)]
            drop(windows_job);
            join_reader(stdout_reader)?;
            join_reader(stderr_reader)?;
            join_writer(stdin_writer)?;
            return Err(ProcessError::Timeout);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                #[cfg(windows)]
                terminate_tree(&mut child, &windows_job);
                #[cfg(not(windows))]
                terminate_tree(&mut child);
                let _ = child.wait();
                #[cfg(windows)]
                drop(windows_job);
                join_reader(stdout_reader)?;
                join_reader(stderr_reader)?;
                join_writer(stdin_writer)?;
                return Err(ProcessError::Io(error));
            }
        }
        thread::sleep(Duration::from_millis(25));
    };
    #[cfg(windows)]
    {
        // A provider must not leave helpers holding inherited stdout/stderr
        // handles after its own process exits. Kill any remaining descendants
        // before joining the pipe readers; closing the job handle also applies
        // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE as a final safeguard.
        let _ = windows_job.terminate();
        drop(windows_job);
    }
    #[cfg(unix)]
    {
        // A provider may exit while one of its helpers still holds a pipe.
        // The child ran in a dedicated process group, so end remaining
        // helpers before waiting for the output collectors to reach EOF.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    join_writer(stdin_writer)?;
    if stdout.1 || stderr.1 {
        return Err(ProcessError::OutputLimit);
    }
    Ok(ProcessOutput {
        status,
        stdout: stdout.0,
        stderr: stderr.0,
        elapsed: started.elapsed(),
    })
}

fn join_writer(
    handle: Option<thread::JoinHandle<std::io::Result<()>>>,
) -> Result<(), ProcessError> {
    if let Some(handle) = handle {
        match handle.join().map_err(|_| ProcessError::Collector)? {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(error) => return Err(ProcessError::Io(error)),
        }
    }
    Ok(())
}

/// A discovered provider is invoked by its verified absolute path. Its own
/// child process lookup is limited to the provider directory and OS system
/// directories; an untrusted entry earlier in the user's PATH is not passed
/// on to a media worker.
fn trusted_provider_path(executable: &std::path::Path) -> Option<OsString> {
    let mut directories = Vec::new();
    if let Some(parent) = executable.parent() {
        directories.push(parent.to_path_buf());
    }
    #[cfg(unix)]
    directories.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(std::path::PathBuf::from));
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        directories.push(std::path::PathBuf::from(root).join("System32"));
    }
    std::env::join_paths(directories).ok()
}

fn collect_bounded_with_lines(
    mut stream: impl Read,
    limit: usize,
    on_line: Option<StdoutLineHandler>,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut data = Vec::new();
    let mut line = Vec::new();
    let mut overflow = false;
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(data.len());
        data.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            overflow = true;
        }
        if let Some(callback) = &on_line {
            for &byte in &buffer[..count] {
                if byte == b'\n' {
                    callback(&String::from_utf8_lossy(&line));
                    line.clear();
                } else if line.len() < 4096 {
                    line.push(byte);
                }
            }
        }
    }
    Ok((data, overflow))
}

#[cfg(not(windows))]
fn terminate_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The child was started in its own process group. Killing the group
        // also stops helper processes spawned by providers such as yt-dlp.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

#[cfg(windows)]
fn terminate_tree(child: &mut std::process::Child, job: &WindowsJobObject) {
    let _ = job.terminate();
    let _ = child.kill();
}

#[cfg(windows)]
struct WindowsJobObject {
    handle: WindowsHandle,
}

#[cfg(windows)]
impl WindowsJobObject {
    fn create() -> std::io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        let raw_handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw_handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let handle = WindowsHandle(raw_handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    fn assign(&self, child: &std::process::Child) -> std::io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

        let assigned = unsafe { AssignProcessToJobObject(self.handle.0, child.as_raw_handle()) };
        if assigned == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn terminate(&self) -> std::io::Result<()> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        let terminated = unsafe { TerminateJobObject(self.handle.0, 1) };
        if terminated == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
struct WindowsHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for WindowsHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}

/// Find and resume the one initial thread of the suspended child process.
/// It cannot create children while suspended, so assigning its process to the
/// Job Object before this point covers the full provider process tree.
#[cfg(windows)]
fn resume_suspended_primary_thread(process_id: u32) -> std::io::Result<()> {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let _snapshot = WindowsHandle(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut found_thread = None;
    let mut has_entry = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while has_entry {
        if entry.th32OwnerProcessID == process_id {
            found_thread = Some(entry.th32ThreadID);
            break;
        }
        has_entry = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    let thread_id = found_thread.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not find the suspended provider thread",
        )
    })?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    if thread.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let _thread = WindowsHandle(thread);
    if unsafe { ResumeThread(thread) } == u32::MAX {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn collect_bounded(mut stream: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut data = Vec::new();
    let mut overflow = false;
    let mut buffer = [0u8; 8192];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(data.len());
        data.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            overflow = true;
        }
    }
    Ok((data, overflow))
}

fn join_reader(
    handle: thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
) -> Result<(Vec<u8>, bool), ProcessError> {
    handle
        .join()
        .map_err(|_| ProcessError::Collector)?
        .map_err(ProcessError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_executable_is_rejected_before_spawn() {
        let spec = ProcessSpec {
            executable: "ffmpeg".into(),
            args: vec![],
            current_dir: None,
            timeout: Duration::from_secs(1),
            output_limit: 1024,
        };
        assert!(matches!(
            run(&spec, &AtomicBool::new(false)),
            Err(ProcessError::RelativeExecutable)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn exited_provider_does_not_leave_pipe_holding_descendants() {
        let spec = ProcessSpec {
            executable: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 5 & exit 0".into()],
            current_dir: None,
            timeout: Duration::from_secs(2),
            output_limit: 1024,
        };
        let started = Instant::now();
        let result = run(&spec, &AtomicBool::new(false)).unwrap();
        assert!(result.status.success());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn trusted_worker_receives_only_explicit_bounded_stdin() {
        let spec = ProcessSpec {
            executable: "/bin/cat".into(),
            args: vec![],
            current_dir: None,
            timeout: Duration::from_secs(2),
            output_limit: 1024,
        };
        let output = run_with_input(&spec, &AtomicBool::new(false), b"hello worker").unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello worker");
        assert!(matches!(
            run_with_input(
                &spec,
                &AtomicBool::new(false),
                &vec![0; 8 * 1024 * 1024 + 1]
            ),
            Err(ProcessError::InputLimit)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn timeout_terminates_provider_descendants_before_pipe_join() {
        let system_root = std::env::var_os("SystemRoot").expect("Windows SystemRoot");
        let spec = ProcessSpec {
            executable: PathBuf::from(system_root).join("System32").join("cmd.exe"),
            args: vec![
                "/C".into(),
                r#"start "" /B /WAIT cmd.exe /C "ping -n 20 127.0.0.1 >NUL""#.into(),
            ],
            current_dir: None,
            timeout: Duration::from_millis(300),
            output_limit: 1024,
        };
        let started = Instant::now();
        assert!(matches!(
            run(&spec, &AtomicBool::new(false)),
            Err(ProcessError::Timeout)
        ));
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "a descendant retained the provider's output pipe after timeout"
        );
    }
}
