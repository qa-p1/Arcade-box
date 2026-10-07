use arcade_contract::{ToolRequest, ToolValue};
use arcade_core::{
    Arcade, PluginInstallApproval, PluginPermissionGrant,
    pipeline::Pipeline,
    provider::{discover_ffmpeg, discover_qpdf, discover_vips},
};
use clap::{Parser, Subcommand};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    sync::atomic::AtomicBool,
};

#[derive(Parser)]
#[command(
    name = "arcadebox",
    version,
    about = "Arcade Box's shared tool runtime from the terminal"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List known tools and their implementation status.
    Tools {
        #[arg(long)]
        all: bool,
        /// Print a stable JSON description (with presets) for other apps.
        #[arg(long)]
        json: bool,
    },
    /// Search the tool catalog.
    Search { query: String },
    /// Run an implemented tool. Use '-' to read text from stdin.
    Run {
        /// The tool to run (not needed with --stdin-json).
        tool_id: Option<String>,
        input: Option<String>,
        /// Start from one of the tool's presets; --set overrides its values.
        #[arg(long)]
        preset: Option<String>,
        /// Read an Arcade Link invoke request (JSON) from stdin and print the
        /// result as JSON.
        #[arg(long)]
        stdin_json: bool,
        #[arg(long)]
        file: Vec<PathBuf>,
        /// Set a tool option, e.g. `--set mode=upper` or `--set count=3`.
        /// Values that parse as JSON (numbers, booleans) keep their type.
        #[arg(long = "set", value_name = "KEY=VALUE")]
        set: Vec<String>,
        #[arg(long)]
        output_name: Option<String>,
        /// Additional tool options as a JSON object.
        #[arg(long)]
        options_json: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Manage or run saved pipelines.
    Pipeline {
        #[command(subcommand)]
        command: PipelineCommand,
    },
    /// Install and manage local sandboxed plugins.
    Plugins {
        #[command(subcommand)]
        command: PluginCommand,
    },
    /// Show discovered system engines.
    Providers,
}

#[derive(Subcommand)]
enum PipelineCommand {
    /// List saved workflows.
    List,
    /// Save a versioned pipeline definition from JSON.
    Save { definition: PathBuf },
    /// Run a saved pipeline with text or selected files.
    Run {
        id: String,
        input: Option<String>,
        #[arg(long)]
        file: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Approve the listed network/device/command effects for this definition.
        #[arg(long)]
        confirm_effects: bool,
    },
    /// Run a one-off pipeline definition from JSON.
    RunFile {
        definition: PathBuf,
        input: String,
        #[arg(long)]
        confirm_effects: bool,
    },
    /// Delete a saved pipeline.
    Delete { id: String },
}

#[derive(Subcommand)]
enum PluginCommand {
    /// List installed local plugins.
    List,
    /// Validate a package and show its manifest and requested permissions.
    Preview { package: PathBuf },
    /// Install an unpacked package; each permission needs an explicit flag.
    Install {
        package: PathBuf,
        #[arg(long = "grant")]
        grants: Vec<String>,
        #[arg(long)]
        acknowledge_escalation: bool,
    },
    /// Execute a local package once without installing it persistently.
    Dev {
        package: PathBuf,
        /// Serialized ToolRequest JSON, including toolId, inputs, and options.
        #[arg(long)]
        request_json: String,
        #[arg(long = "grant")]
        grants: Vec<String>,
    },
    /// Uninstall an installed plugin by tool ID.
    Uninstall { plugin_id: String },
}

fn main() {
    if let Err(error) = run() {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|io_error| io_error.kind() == io::ErrorKind::BrokenPipe)
        {
            return;
        }
        eprintln!("Arcade Box: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Arcade Link one-shot mode: one request on stdin, no other output.
    if std::env::args().nth(1).as_deref() == Some(arcade_link::oneshot::FLAG) {
        let runtime = std::sync::Arc::new(open_runtime()?);
        std::process::exit(arcade_link::oneshot::serve(
            &arcade_core::link::OneshotHandler { runtime },
        ));
    }
    let cli = Cli::parse();
    if let Command::Tools { all, json: true } = cli.command {
        return print_tools_json(all);
    }
    if matches!(cli.command, Command::Providers) {
        for provider in discover_ffmpeg(None)
            .into_iter()
            .chain(discover_qpdf(None))
            .chain(discover_vips(None))
        {
            writeln!(
                io::stdout().lock(),
                "{}\t{}\t{}\t{}\t{}",
                provider.capability,
                provider.source,
                provider.version,
                if provider.compatible {
                    "compatible"
                } else {
                    "warning"
                },
                provider.executable_path.display()
            )?;
            if let Some(warning) = provider.warning {
                eprintln!("  {warning}");
            }
        }
        return Ok(());
    }
    let runtime = open_runtime()?;
    match cli.command {
        Command::Tools { all, .. } => {
            for tool in runtime
                .list_tools()
                .iter()
                .filter(|tool| all || tool.status != arcade_contract::ImplementationStatus::Planned)
            {
                writeln!(
                    io::stdout().lock(),
                    "{}\t{:?}\t{}",
                    tool.id,
                    tool.status,
                    tool.name
                )?;
            }
        }
        Command::Search { query } => {
            for tool in runtime.search_tools(&query).into_iter().take(20) {
                writeln!(
                    io::stdout().lock(),
                    "{}\t{:?}\t{}",
                    tool.id,
                    tool.status,
                    tool.name
                )?;
            }
        }
        Command::Run {
            stdin_json: true, ..
        } => {
            let mut request = String::new();
            io::stdin().read_to_string(&mut request)?;
            let request: arcade_link::InvokeRequest = serde_json::from_str(&request)?;
            let result =
                arcade_core::link::run_blocking(&runtime, &request, &AtomicBool::new(false))
                    .map_err(|error| error.to_string())?;
            writeln!(
                io::stdout().lock(),
                "{}",
                serde_json::to_string_pretty(&result)?
            )?;
        }
        Command::Run {
            tool_id,
            input,
            preset,
            file,
            set,
            output_name,
            options_json,
            json,
            ..
        } => {
            let tool_id = tool_id.ok_or("run needs a tool ID (or --stdin-json)")?;
            let mut options = serde_json::Map::new();
            if let Some(preset) = preset {
                let tool = runtime
                    .list_tools()
                    .into_iter()
                    .find(|tool| tool.id == tool_id)
                    .ok_or_else(|| format!("unknown tool {tool_id}"))?;
                let preset = tool
                    .preset(&preset)
                    .ok_or_else(|| format!("{tool_id} has no preset {preset}"))?;
                options.extend(preset.options.clone());
            }
            if let Some(value) = options_json {
                options.extend(serde_json::from_str::<
                    serde_json::Map<String, serde_json::Value>,
                >(&value)?);
            }
            for assignment in set {
                let (key, value) = assignment
                    .split_once('=')
                    .ok_or_else(|| format!("--set expects KEY=VALUE, got `{assignment}`"))?;
                let value = serde_json::from_str::<serde_json::Value>(value)
                    .ok()
                    .filter(|parsed| {
                        !parsed.is_string() && !parsed.is_object() && !parsed.is_array()
                    })
                    .unwrap_or_else(|| value.into());
                options.insert(key.trim().into(), value);
            }
            if let Some(output_name) = output_name {
                options.insert("outputName".into(), output_name.into());
            }
            // Text goes in under the first text-like type the tool declares,
            // so URL and structured-input tools accept typed input too.
            let text_mime = runtime
                .list_tools()
                .into_iter()
                .find(|tool| tool.id == tool_id)
                .and_then(|tool| {
                    tool.inputs.into_iter().find(|mime| {
                        ["text/", "network/", "structured/", "rows/"]
                            .iter()
                            .any(|prefix| mime.starts_with(prefix))
                    })
                })
                .map(|mime| mime.trim_end_matches("[]").to_owned())
                .unwrap_or_else(|| "text/plain".into());
            let inputs = match (input, file.is_empty()) {
                (Some(input), true) => vec![ToolValue::text(input_text(input)?, text_mime)],
                (None, false) => {
                    file.iter()
                        .map(|path| {
                            if path.is_dir() {
                                runtime.grants().grant_input_directory(path).map(|folder| {
                                    ToolValue {
                                        kind: arcade_contract::ValueKind::Artifact,
                                        value: folder.token,
                                        mime: "folder/reference".into(),
                                    }
                                })
                            } else {
                                runtime
                                    .grants()
                                    .grant(path)
                                    .map(|grant| grant.as_tool_value())
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?
                }
                (Some(_), false) => {
                    return Err("provide either text input or --file, not both".into());
                }
                (None, true) => Vec::new(),
            };
            let result = runtime.run_tool(ToolRequest {
                tool_id,
                inputs,
                options: options.into(),
            })?;
            if result.status == arcade_contract::ResultStatus::Error {
                return Err(result
                    .message
                    .unwrap_or_else(|| "tool failed".into())
                    .into());
            }
            let mut result = result;
            resolve_cli_outputs(&runtime, &mut result.outputs)?;
            if json {
                writeln!(
                    io::stdout().lock(),
                    "{}",
                    serde_json::to_string_pretty(&result)?
                )?;
            } else {
                for output in result.outputs {
                    writeln!(io::stdout().lock(), "{}", output.value)?;
                }
            }
        }
        Command::Pipeline { command } => match command {
            PipelineCommand::List => {
                for pipeline in runtime.list_pipelines()? {
                    writeln!(
                        io::stdout().lock(),
                        "{}\tv{}\t{}",
                        pipeline.id,
                        pipeline.version,
                        pipeline.name
                    )?;
                }
            }
            PipelineCommand::Save { definition } => {
                let pipeline: Pipeline = serde_json::from_slice(&fs::read(definition)?)?;
                let saved = runtime.save_pipeline(pipeline)?;
                writeln!(
                    io::stdout().lock(),
                    "Saved {} ({}), searchable as arcade.pipeline.{}",
                    saved.name,
                    saved.id,
                    saved.id
                )?;
            }
            PipelineCommand::Run {
                id,
                input,
                file,
                json,
                confirm_effects,
            } => {
                if confirm_effects {
                    runtime.set_pipeline_confirmation(|_, _| true);
                }
                let inputs = selected_inputs(&runtime, input, file)?;
                let mut outputs =
                    runtime.run_saved_pipeline(&id, inputs, &AtomicBool::new(false))?;
                for values in outputs.values_mut() {
                    resolve_cli_outputs(&runtime, values)?;
                }
                if json {
                    writeln!(
                        io::stdout().lock(),
                        "{}",
                        serde_json::to_string_pretty(&outputs)?
                    )?;
                } else {
                    let definition = runtime
                        .list_pipelines()?
                        .into_iter()
                        .find(|pipeline| pipeline.id == id)
                        .ok_or("pipeline no longer exists")?;
                    for node_id in &definition.output_nodes {
                        if let Some(values) = outputs.get(node_id) {
                            for value in values {
                                writeln!(io::stdout().lock(), "{}", value.value)?;
                            }
                        }
                    }
                }
            }
            PipelineCommand::RunFile {
                definition,
                input,
                confirm_effects,
            } => {
                if confirm_effects {
                    runtime.set_pipeline_confirmation(|_, _| true);
                }
                let pipeline: Pipeline = serde_json::from_slice(&fs::read(definition)?)?;
                let mut result = pipeline.run(
                    &runtime,
                    vec![ToolValue::text(input_text(input)?, "text/plain")],
                    &AtomicBool::new(false),
                )?;
                for values in result.values_mut() {
                    resolve_cli_outputs(&runtime, values)?;
                }
                for id in &pipeline.output_nodes {
                    if let Some(outputs) = result.get(id) {
                        for output in outputs {
                            writeln!(io::stdout().lock(), "{}", output.value)?;
                        }
                    }
                }
            }
            PipelineCommand::Delete { id } => {
                runtime.delete_pipeline(&id)?;
                writeln!(io::stdout().lock(), "Deleted pipeline {id}")?;
            }
        },
        Command::Plugins { command } => match command {
            PluginCommand::List => {
                for plugin in runtime.list_plugins()? {
                    writeln!(
                        io::stdout().lock(),
                        "{}\t{}\t{}",
                        plugin.manifest.tool_manifest.id,
                        plugin.manifest.tool_manifest.version,
                        plugin.manifest.tool_manifest.name
                    )?;
                }
            }
            PluginCommand::Preview { package } => {
                let preview = runtime.preview_plugin(&package)?;
                writeln!(
                    io::stdout().lock(),
                    "{}",
                    serde_json::to_string_pretty(&preview)?
                )?;
            }
            PluginCommand::Install {
                package,
                grants,
                acknowledge_escalation,
            } => {
                let installed = runtime.install_plugin(
                    &package,
                    PluginInstallApproval {
                        grants: parse_plugin_grants(grants)?,
                        acknowledge_escalation,
                    },
                )?;
                writeln!(
                    io::stdout().lock(),
                    "Installed {} {}",
                    installed.manifest.tool_manifest.id,
                    installed.manifest.tool_manifest.version
                )?;
            }
            PluginCommand::Dev {
                package,
                request_json,
                grants,
            } => {
                let preview = runtime.preview_plugin(&package)?;
                let request: ToolRequest = serde_json::from_str(&request_json)?;
                if request.tool_id != preview.manifest.tool_manifest.id {
                    return Err("request toolId does not match the package manifest".into());
                }
                let result = runtime.run_plugin_dev(
                    &package,
                    &request,
                    PluginInstallApproval {
                        grants: parse_plugin_grants(grants)?,
                        acknowledge_escalation: false,
                    },
                    &AtomicBool::new(false),
                )?;
                if result.status == arcade_contract::ResultStatus::Error {
                    return Err(result
                        .message
                        .unwrap_or_else(|| "plugin failed".into())
                        .into());
                }
                let mut result = result;
                resolve_cli_outputs(&runtime, &mut result.outputs)?;
                writeln!(
                    io::stdout().lock(),
                    "{}",
                    serde_json::to_string_pretty(&result)?
                )?;
            }
            PluginCommand::Uninstall { plugin_id } => {
                runtime.uninstall_plugin(&plugin_id)?;
                writeln!(io::stdout().lock(), "Uninstalled {plugin_id}")?;
            }
        },
        Command::Providers => unreachable!(),
    }
    Ok(())
}

fn selected_inputs(
    runtime: &Arcade,
    input: Option<String>,
    files: Vec<PathBuf>,
) -> Result<Vec<ToolValue>, Box<dyn std::error::Error>> {
    match (input, files.is_empty()) {
        (Some(input), true) => Ok(vec![ToolValue::text(input_text(input)?, "text/plain")]),
        (None, false) => files
            .iter()
            .map(|path| {
                runtime
                    .grants()
                    .grant(path)
                    .map(|grant| grant.as_tool_value())
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into),
        (Some(_), false) => Err("provide either text input or --file, not both".into()),
        (None, true) => Err("provide text input or at least one --file".into()),
    }
}

/// Convert process-local artifact grants into stable file paths at the CLI
/// boundary. The core contract continues to use opaque grants internally.
fn open_runtime() -> Result<Arcade, Box<dyn std::error::Error>> {
    let project_dirs = directories::ProjectDirs::from("dev", "Arcade Box", "Arcade Box")
        .ok_or("could not locate application data directory")?;
    fs::create_dir_all(project_dirs.data_dir())?;
    Ok(Arcade::open(
        &project_dirs.data_dir().join("arcade.sqlite3"),
    )?)
}

/// `tools --json`: a stable description of the built-in tools and their
/// presets, read from the embedded catalog without opening the database.
fn print_tools_json(all: bool) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = arcade_core::builtin_catalog()?;
    let tools: Vec<serde_json::Value> = catalog
        .tools
        .iter()
        .filter(|tool| all || tool.status != arcade_contract::ImplementationStatus::Planned)
        .map(|tool| {
            serde_json::json!({
                "id": tool.id,
                "name": tool.name,
                "description": tool.description,
                "category": tool.category,
                "status": tool.status,
                "inputs": tool.inputs,
                "outputs": tool.outputs,
                "linkAccepts": arcade_core::link::link_accepts(tool),
                "privacy": tool.privacy_class,
                "presets": tool.presets,
                "featuredFor": tool.link.as_ref().map(|link| link.featured_for.clone()).unwrap_or_default(),
            })
        })
        .collect();
    writeln!(
        io::stdout().lock(),
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "schema": 1, "tools": tools }))?
    )?;
    Ok(())
}

fn resolve_cli_outputs(
    runtime: &Arcade,
    outputs: &mut [ToolValue],
) -> Result<(), Box<dyn std::error::Error>> {
    for output in outputs {
        if output.kind == arcade_contract::ValueKind::Artifact {
            let path = runtime.grants().resolve(&output.value)?;
            output.kind = arcade_contract::ValueKind::File;
            output.value = path.to_string_lossy().into_owned();
        }
    }
    Ok(())
}

fn parse_plugin_grants(
    grants: Vec<String>,
) -> Result<BTreeSet<PluginPermissionGrant>, Box<dyn std::error::Error>> {
    grants
        .into_iter()
        .map(|grant| match grant.as_str() {
            "read-user-selected" => Ok(PluginPermissionGrant::ReadUserSelectedFiles),
            _ => Err(format!("unsupported permission grant: {grant}").into()),
        })
        .collect()
}

fn input_text(value: String) -> io::Result<String> {
    if value != "-" {
        return Ok(value);
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcade_contract::{ResultStatus, ValueKind};
    use std::{path::Path, sync::atomic::AtomicBool};

    #[test]
    fn cli_tool_outputs_resolve_artifact_grants_to_file_paths() {
        let runtime = Arcade::in_memory().unwrap();
        let mut result = runtime
            .run_tool(ToolRequest {
                tool_id: "arcade.barcode.qr-generate".into(),
                inputs: vec![ToolValue::text("https://example.test", "text/plain")],
                options: serde_json::json!({}),
            })
            .unwrap();

        assert_eq!(result.status, ResultStatus::Success);
        assert_eq!(result.outputs[0].kind, ValueKind::Artifact);
        resolve_cli_outputs(&runtime, &mut result.outputs).unwrap();
        assert_eq!(result.outputs[0].kind, ValueKind::File);
        let path = Path::new(&result.outputs[0].value);
        assert!(path.is_absolute());
        assert!(path.is_file());
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["outputs"][0]["kind"], "file");
        assert_eq!(json["outputs"][0]["value"], result.outputs[0].value);
    }

    #[test]
    fn cli_pipeline_outputs_resolve_artifact_grants_to_file_paths() {
        let runtime = Arcade::in_memory().unwrap();
        let pipeline = Pipeline {
            id: "cli-qr-output".into(),
            name: "CLI QR Output".into(),
            version: 1,
            nodes: vec![arcade_core::pipeline::PipelineNode {
                link: None,
                id: "qr".into(),
                tool_id: "arcade.barcode.qr-generate".into(),
                inputs: vec![arcade_core::pipeline::InputSource::External { index: 0 }],
                options: serde_json::json!({}),
            }],
            output_nodes: vec!["qr".into()],
        };
        let mut result = pipeline
            .run(
                &runtime,
                vec![ToolValue::text("https://example.test", "text/plain")],
                &AtomicBool::new(false),
            )
            .unwrap();
        let output_path = {
            let values = result.get_mut("qr").unwrap();
            assert_eq!(values[0].kind, ValueKind::Artifact);
            resolve_cli_outputs(&runtime, values).unwrap();
            assert_eq!(values[0].kind, ValueKind::File);
            let path = Path::new(&values[0].value);
            assert!(path.is_absolute());
            assert!(path.is_file());
            values[0].value.clone()
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["qr"][0]["kind"], "file");
        assert_eq!(json["qr"][0]["value"], output_path);
    }
}
