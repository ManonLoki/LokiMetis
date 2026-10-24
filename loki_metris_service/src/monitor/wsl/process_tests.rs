//! Windows WSL child owner 的真实进程生命周期回归。

use super::super::process_owner::retained_wsl_child_ids;
use super::*;

/// 超时 child 必须立即进入 owner，并在没有后续命令时由后台 reaper 回收。
#[test]
fn unreaped_child_handle_is_reaped_during_idle() {
    let system_wsl = windows_system_wsl_executable("test", "error.hooks.writeFailed")
        .expect("Windows System32 wsl.exe");
    let command = system_wsl
        .parent()
        .expect("System32 parent")
        .join("cmd.exe");
    let child = Command::new(command)
        .args(["/D", "/Q", "/C", "set /P lokimetis="])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("long-running Windows child");
    let process_id = child.id();

    retain_wsl_child(child);
    assert!(retained_wsl_child_ids().contains(&process_id));

    let deadline = Instant::now() + Duration::from_secs(1);
    while retained_wsl_child_ids().contains(&process_id) && Instant::now() < deadline {
        thread::sleep(WSL_COMMAND_POLL);
    }
    assert!(!retained_wsl_child_ids().contains(&process_id));
    super::super::process_owner::shutdown_retained_wsl_children_until(deadline);
}
