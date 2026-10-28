// Release builds on Windows are GUI apps: no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("oxidraft {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    oxidraft_app::run()
}
