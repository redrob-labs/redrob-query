//! Redrob Query desktop app: an agentic database client in one egui window.
//!
//! Usage: `redrob-query [--version]`
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    if std::env::args().skip(1).any(|a| a == "--version") {
        println!("redrob-query {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    eframe::run_native(
        "Redrob Query",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(Placeholder))),
    )
}

struct Placeholder;

impl eframe::App for Placeholder {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.label("Redrob Query");
    }
}
