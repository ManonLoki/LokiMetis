#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 先处理内部 Hook 中继子进程模式，否则启动 GUI 应用。
fn main() {
    if let Some(exit_code) = loki_metis_gui_lib::run_hook_relay_if_requested() {
        std::process::exit(exit_code);
    }
    loki_metis_gui_lib::run();
}
