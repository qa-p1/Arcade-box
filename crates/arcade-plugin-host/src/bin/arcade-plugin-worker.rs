fn main() {
    if let Err(error) = arcade_plugin_host::worker::run_stdio() {
        eprintln!("Arcade Box plugin worker failed: {error}");
        std::process::exit(2);
    }
}
