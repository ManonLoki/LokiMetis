#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(exit_code) = loki_metis_gui_lib::run_hook_relay_if_requested() {
        std::process::exit(exit_code);
    }
    loki_metis_gui_lib::run();
}
