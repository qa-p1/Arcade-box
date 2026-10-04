#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

const HELP: &str = "Arcade Box

  arcade-desktop                   Start Arcade Box in the tray (or show the Island if it is running)
  arcade-desktop --background      Start in the tray without showing anything
  arcade-desktop --settings        Open Settings
  arcade-desktop --quit            Quit the running instance
  arcade-desktop --version         Print the version
  arcade-desktop --arcade-manifest Print the Arcade Link manifest (no side effects)

Terminal tools: arcade-box (alias arcadebox) --help";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            arcade_desktop::attach_console();
            println!("Arcade Box {}", env!("CARGO_PKG_VERSION"));
        }
        Some("--help" | "-h") => {
            arcade_desktop::attach_console();
            println!("{HELP}");
        }
        Some("--arcade-manifest") => {
            arcade_desktop::attach_console();
            arcade_desktop::print_manifest();
        }
        _ => arcade_desktop::run(args),
    }
}
