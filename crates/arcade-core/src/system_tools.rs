//! System information and the process finder. Inspection is read-only; a
//! process is only ever terminated through an explicit, separate command.

use arcade_contract::{ResultStatus, ToolManifest, ToolRequest, ToolResult, ToolValue};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    sync::atomic::{AtomicBool, Ordering},
};
#[cfg(target_os = "windows")]
use std::{path::PathBuf, process::Command};
use sysinfo::{
    Disks, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind,
};

const MAX_PROCESSES_LISTED: usize = 200;

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("System inspection cancelled".into());
    }
    let output = match manifest.id.as_str() {
        "arcade.system.system-info" => system_information(),
        "arcade.system.process" => inspect_processes(optional_text_input(request)?),
        _ => {
            return Err(format!(
                "no system inspector registered for {}",
                manifest.id
            ));
        }
    }?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("System inspection cancelled".into());
    }
    let mime = match manifest.id.as_str() {
        "arcade.system.system-info" => "structured/system-info",
        "arcade.system.process" => "structured/process-list",
        _ => "structured/json",
    };
    let serialized = serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?;
    Ok(ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs: vec![ToolValue::text(serialized, mime)],
        message: None,
        warnings: vec![],
        metadata: BTreeMap::new(),
    })
}

/// Inspect process names and resource use without reading command lines or environments.
fn inspect_processes(query: Option<&str>) -> Result<Value, String> {
    let query = query.map(str::trim).filter(|query| !query.is_empty());
    if query.is_some_and(|query| query.len() > 256 || query.chars().any(char::is_control)) {
        return Err(
            "Process search must be under 256 characters and contain no control characters".into(),
        );
    }

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .with_exe(UpdateKind::OnlyIfNotSet),
    );
    let current_pid = std::process::id();
    let needle = query.map(str::to_lowercase);
    let mut processes = system
        .processes()
        .values()
        .filter_map(|process| {
            let pid = process.pid().as_u32();
            let name = process.name().to_string_lossy().into_owned();
            let executable = process
                .exe()
                .map(|path| path.to_string_lossy().into_owned());
            if let Some(needle) = needle.as_deref() {
                let pid_match = needle.parse::<u32>().ok() == Some(pid);
                let name_match = name.to_lowercase().contains(needle);
                let executable_match = executable
                    .as_deref()
                    .is_some_and(|path| path.to_lowercase().contains(needle));
                if !pid_match && !name_match && !executable_match {
                    return None;
                }
            }
            Some(json!({
                "pid": pid,
                "name": name,
                "executable": executable,
                "startTime": process.start_time(),
                "cpuPercent": process.cpu_usage(),
                "memoryBytes": process.memory(),
                "protected": pid <= 1 || pid == current_pid,
            }))
        })
        .collect::<Vec<_>>();
    processes.sort_by(|left, right| {
        let left_memory = left.get("memoryBytes").and_then(Value::as_u64).unwrap_or(0);
        let right_memory = right
            .get("memoryBytes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        right_memory.cmp(&left_memory).then_with(|| {
            left.get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .cmp(
                    right
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
        })
    });
    let total_matches = processes.len();
    processes.truncate(MAX_PROCESSES_LISTED);
    Ok(json!({
        "query": query,
        "totalMatches": total_matches,
        "listedCount": processes.len(),
        "truncated": total_matches > processes.len(),
        "processes": processes,
        "limitations": ["Process metrics are a current snapshot; CPU percentage is sampled during this query and may be near zero."],
    }))
}

/// Recheck process identity before requesting termination to guard against PID reuse.
pub fn terminate_process(
    pid: u32,
    expected_name: &str,
    expected_executable: Option<&str>,
    expected_start_time: u64,
) -> Result<(), String> {
    if pid <= 1 || pid == std::process::id() {
        return Err("This process is protected from termination".into());
    }
    if expected_name.is_empty() || expected_name.len() > 512 {
        return Err("The selected process identity is invalid".into());
    }
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    let process = system
        .process(pid)
        .ok_or("The selected process has already exited")?;
    let actual_executable = process
        .exe()
        .map(|path| path.to_string_lossy().into_owned());
    if process.name().to_string_lossy() != expected_name
        || process.start_time() != expected_start_time
        || actual_executable.as_deref() != expected_executable
    {
        return Err(
            "The process changed since it was selected; refresh the list before trying again"
                .into(),
        );
    }
    #[cfg(target_os = "windows")]
    {
        let system_root = env::var_os("WINDIR").ok_or("Windows system directory is unavailable")?;
        let taskkill = PathBuf::from(system_root).join("System32/taskkill.exe");
        let output = Command::new(taskkill)
            .arg("/PID")
            .arg(pid.as_u32().to_string())
            .output()
            .map_err(|error| format!("Could not request process termination: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(if detail.is_empty() {
                "The operating system could not terminate this process".into()
            } else {
                detail
            })
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        match process.kill_with(Signal::Term) {
            Some(true) => Ok(()),
            Some(false) => Err("The operating system could not terminate this process".into()),
            None => Err("This platform does not support requesting process termination".into()),
        }
    }
}

fn optional_text_input(request: &ToolRequest) -> Result<Option<&str>, String> {
    match request.inputs.as_slice() {
        [] => Ok(None),
        [input] if input.mime.starts_with("text/") => Ok(Some(input.value.as_str())),
        [_] => Err("This inspector accepts a text search phrase".into()),
        _ => Err("This inspector accepts at most one text input".into()),
    }
}

fn system_information() -> Result<Value, String> {
    let system = System::new_all();
    let cpu = system.cpus().first();
    let disks = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|disk| {
            json!({
                "name": disk.name().to_string_lossy(),
                "mountPoint": disk.mount_point().to_string_lossy(),
                "fileSystem": disk.file_system().to_string_lossy(),
                "totalBytes": disk.total_space(),
                "availableBytes": disk.available_space(),
            })
        })
        .collect::<Vec<_>>();
    let networks = Networks::new_with_refreshed_list();
    let interfaces = networks
        .iter()
        .map(|(name, data)| {
            json!({
                "name": name,
                "receivedBytes": data.total_received(),
                "transmittedBytes": data.total_transmitted(),
            })
        })
        .collect::<Vec<_>>();
    let logical_cores = system.cpus().len();
    Ok(json!({
        "operatingSystem": {
            "name": System::name(),
            "version": System::os_version(),
            "longVersion": System::long_os_version(),
            "kernel": System::kernel_version(),
            "family": env::consts::FAMILY,
            "architecture": env::consts::ARCH,
            "hostName": System::host_name(),
            "uptimeSeconds": System::uptime(),
        },
        "processor": {
            "model": cpu.map(|cpu| cpu.brand()).filter(|value| !value.is_empty()),
            "logicalCores": logical_cores,
            "physicalCores": System::physical_core_count(),
            "reportedFrequencyMhz": cpu.map(|cpu| cpu.frequency()),
        },
        "memory": {
            "totalBytes": system.total_memory(),
            "usedBytes": system.used_memory(),
            "availableBytes": system.available_memory(),
            "swapTotalBytes": system.total_swap(),
            "swapUsedBytes": system.used_swap(),
        },
        "storage": disks,
        "networkInterfaces": interfaces,
        "gpuInventory": null,
        "displayInventory": null,
        "limitations": [
            "GPU and display inventory need additional native platform adapters and are not reported by this provider."
        ],
    }))
}
