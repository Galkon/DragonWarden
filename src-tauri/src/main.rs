// Hides the Windows console in release builds; the managed game keeps its own console via -NewConsole.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    dragonwarden_lib::run();
}
