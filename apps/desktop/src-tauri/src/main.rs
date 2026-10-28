#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

const HELP: &str = "Arcade Box

  arcade-desktop                   Start Arcade Box in the tray (or show the Island if it is running)
  arcade-desktop --background      Start in the tray without showing anything
  arcade-desktop --show            Show the Island
  arcade-desktop --hide            Hide the Island
  arcade-desktop --toggle          Show or hide the Island
  arcade-desktop --dashboard       Open the dashboard
  arcade-desktop --settings        Open Settings
  arcade-desktop --pipelines       Open Pipelines
  arcade-desktop --restart         Restart the running instance
  arcade-desktop --quit            Quit the running instance
  arcade-desktop --install [--silent] [--no-launch] [--keep-download] [--channel C]
                                   Install this AppImage / macOS app bundle
  arcade-desktop --uninstall [--silent] [--remove-data]
                                   Remove this installation via its receipt
  arcade-desktop --repair          Rewrite the launcher, command and login entries
  arcade-desktop --integration-status
                                   Print the integration as JSON (no side effects)
  arcade-desktop --status          Print whether Box is running, its version and Link state (JSON)
  arcade-desktop --version         Print the version
  arcade-desktop --arcade-manifest Print the Arcade Link manifest (no side effects)
  arcade-desktop --arcade-invoke   Run one Arcade Link request from stdin, without any UI

Terminal tools: arcade-box (alias arcadebox) --help";

fn main() {
    arcade_link::childenv::on_startup();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = arcade_desktop::installer::run_terminal_alias(&args) {
        std::process::exit(code);
    }
    // The terminal tools above resolve relative paths against the user's
    // directory; the Island and Settings are webviews, and WebKit needs the
    // AppImage's own directory.
    arcade_link::childenv::enter_image_dir();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            arcade_desktop::attach_console();
            println!("Arcade Box {}", env!("CARGO_PKG_VERSION"));
        }
        Some("--help" | "-h") => {
            arcade_desktop::attach_console();
            println!("{HELP}");
        }
        Some("--status") => {
            arcade_desktop::attach_console();
            arcade_desktop::print_status();
        }
        Some("--arcade-manifest") => {
            arcade_desktop::attach_console();
            arcade_desktop::print_manifest();
        }
        Some("--arcade-invoke") => std::process::exit(arcade_desktop::serve_oneshot()),
        Some("--install" | "--uninstall" | "--repair" | "--integration-status") => {
            arcade_desktop::attach_console();
            if let Err(error) = integration_command(&args) {
                eprintln!("arcade-desktop: {error}");
                std::process::exit(1);
            }
        }
        _ => {
            let argv: Vec<String> = std::env::args().collect();
            if !arcade_desktop::installer::first_run_prompt(&argv) {
                arcade_desktop::run(args);
            }
        }
    }
}

/// `--install`, `--uninstall`, `--repair` and `--integration-status` (SPEC §8.1).
fn integration_command(args: &[String]) -> Result<(), String> {
    use arcade_desktop::installer;
    let action = args[0].as_str();
    let (mut silent, mut no_launch, mut remove_data, mut channel) = (false, false, false, None);
    let mut keep_download = false;
    let mut it = args[1..].iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--silent" => silent = true,
            "--no-launch" => no_launch = true,
            // Read by the installer itself; validated here.
            "--keep-download" => keep_download = true,
            "--remove-data" => remove_data = true,
            "--channel" => {
                channel = Some(
                    it.next()
                        .filter(|c| !c.is_empty() && !c.starts_with('-'))
                        .ok_or("--channel needs a channel name")?
                        .clone(),
                )
            }
            other => return Err(format!("unexpected argument {other:?} for {action}")),
        }
    }
    if (no_launch || keep_download || channel.is_some()) && action != "--install" {
        return Err("--no-launch, --keep-download and --channel require --install".into());
    }
    if remove_data && action != "--uninstall" {
        return Err("--remove-data requires --uninstall".into());
    }
    if silent && !matches!(action, "--install" | "--uninstall") {
        return Err("--silent requires --install or --uninstall".into());
    }
    let output = match action {
        "--install" => {
            installer::install_cli(silent, no_launch, channel.as_deref().unwrap_or("stable"))?
        }
        "--uninstall" => installer::uninstall_cli(silent, remove_data)?,
        "--repair" => installer::repair_cli()?,
        _ => installer::integration_status_cli()?,
    };
    if !output.is_empty() {
        println!("{output}");
    }
    Ok(())
}
