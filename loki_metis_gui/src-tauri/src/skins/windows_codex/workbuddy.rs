//! Windows WorkBuddy 主进程的发现、验证、启动和定向重启。

use std::mem::size_of;
use std::path::{Path, PathBuf};

use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
};
use wmi::WMIConnection;

use super::{
    AppError, GuiProcess, OwnedHandle, PROCESS_COMMAND_LINE_TIMEOUT, WmiProcess,
    paths_equal_ignore_ascii_case, query_process_path, utf16_array_to_string,
};

const WORKBUDDY_DEBUG_PORT: &str = "9441";

/// 检查经过安装路径验证的 WorkBuddy 主进程是否存在。
pub(crate) async fn workbuddy_is_gui_running() -> Result<bool, AppError> {
    Ok(!enumerate_gui_processes(&discover_executables())?.is_empty())
}

/// 返回 WorkBuddy 主进程及其命令行，辅助进程不会进入结果。
pub(crate) async fn workbuddy_gui_processes() -> Result<Vec<(GuiProcess, String)>, AppError> {
    let candidates = discover_executables();
    let verified = enumerate_gui_processes(&candidates)?;
    if verified.is_empty() {
        return Ok(Vec::new());
    }
    let query = tokio::task::spawn_blocking(query_process_command_lines);
    let rows = tokio::time::timeout(PROCESS_COMMAND_LINE_TIMEOUT, query)
        .await
        .map_err(|_| process_inspection_error())?
        .map_err(|_| process_inspection_error())?
        .map_err(|_| process_inspection_error())?;
    let command_lines = rows
        .into_iter()
        .filter_map(|row| Some((row.process_id, row.command_line?)))
        .collect::<std::collections::HashMap<_, _>>();
    Ok(verified
        .into_iter()
        .filter_map(|process| {
            let command_line = command_lines.get(&process.pid)?.clone();
            Some((process, command_line))
        })
        .collect())
}

pub(crate) async fn workbuddy_gui_process_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    Ok(workbuddy_gui_processes()
        .await?
        .into_iter()
        .map(|(process, line)| (process.pid(), line))
        .collect())
}

pub(crate) async fn launch_workbuddy() -> Result<(), AppError> {
    let candidates = discover_executables();
    if !enumerate_gui_processes(&candidates)?.is_empty() {
        return Err(AppError::new(
            "skin.workbuddy_manual_close_required",
            "WorkBuddy 正在运行但未开放皮肤所需的调试端口。请保存工作并手动完全退出 WorkBuddy，再由LokiMetis启动。",
        ));
    }
    let executable = candidates.into_iter().next().ok_or_else(|| {
        AppError::new(
            "skin.workbuddy_not_found",
            "未找到官方 WorkBuddy 桌面应用，请先安装或更新应用。",
        )
    })?;
    tokio::process::Command::new(executable)
        .env("WORKBUDDY_REMOTE_DEBUGGING_PORT", WORKBUDDY_DEBUG_PORT)
        .spawn()
        .map_err(|_| AppError::new("skin.workbuddy_launch_failed", "无法启动 WorkBuddy。"))?;
    Ok(())
}

pub(crate) async fn restart_workbuddy_gui_process(
    pid: u32,
    expected_path: &Path,
    arguments: &[String],
    port: u16,
) -> Result<(), AppError> {
    let process = enumerate_gui_processes(&discover_executables())?
        .into_iter()
        .find(|process| {
            process.pid == pid && paths_equal_ignore_ascii_case(&process.path, expected_path)
        })
        .ok_or_else(|| {
            AppError::new(
                "skin.workbuddy_instance_changed",
                "所选 WorkBuddy 实例已退出或身份发生变化，请重新选择。",
            )
        })?;
    let handle = open_verified_process(&process)?;
    unsafe { TerminateProcess(handle.0, 0) }.map_err(|_| {
        AppError::new(
            "skin.workbuddy_force_close_failed",
            "无法关闭所选 WorkBuddy 实例。",
        )
    })?;
    drop(handle);
    tokio::process::Command::new(expected_path)
        .args(arguments)
        .env("WORKBUDDY_REMOTE_DEBUGGING_PORT", port.to_string())
        .spawn()
        .map_err(|_| {
            AppError::new(
                "skin.workbuddy_launch_failed",
                "所选 WorkBuddy 实例已关闭，但无法使用原启动参数重新打开。",
            )
        })?;
    Ok(())
}

pub(crate) async fn force_close_workbuddy_gui() -> Result<usize, AppError> {
    let processes = enumerate_gui_processes(&discover_executables())?;
    let count = processes.len();
    let handles = processes
        .iter()
        .map(open_verified_process)
        .collect::<Result<Vec<_>, _>>()?;
    for handle in handles {
        let _ = unsafe { TerminateProcess(handle.0, 0) };
    }
    Ok(count)
}

fn query_process_command_lines() -> Result<Vec<WmiProcess>, ()> {
    let connection = WMIConnection::new().map_err(|_| ())?;
    connection
        .raw_query("SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name = 'WorkBuddy.exe'")
        .map_err(|_| ())
}

fn enumerate_gui_processes(candidates: &[PathBuf]) -> Result<Vec<GuiProcess>, AppError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(OwnedHandle)
        .map_err(|_| {
            AppError::new(
                "skin.workbuddy_launch_failed",
                "无法检查 WorkBuddy 运行状态。",
            )
        })?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_err() {
        return Ok(Vec::new());
    }
    let mut processes = Vec::new();
    loop {
        let name = utf16_array_to_string(&entry.szExeFile);
        if name.eq_ignore_ascii_case("WorkBuddy.exe")
            && let Some(path) = query_process_path(entry.th32ProcessID)
            && candidates
                .iter()
                .any(|candidate| paths_equal_ignore_ascii_case(&path, candidate))
        {
            processes.push(GuiProcess {
                pid: entry.th32ProcessID,
                path,
            });
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }
    Ok(processes)
}

fn open_verified_process(process: &GuiProcess) -> Result<OwnedHandle, AppError> {
    unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            process.pid,
        )
    }
    .map(OwnedHandle)
    .map_err(|_| {
        AppError::new(
            "skin.workbuddy_force_close_failed",
            "无法关闭 WorkBuddy，请保存工作后手动退出。",
        )
    })
}

fn process_inspection_error() -> AppError {
    AppError::new(
        "skin.workbuddy_process_inspection_failed",
        "无法读取 WorkBuddy 调试启动参数。",
    )
}

fn discover_executables() -> Vec<PathBuf> {
    let mut candidates = std::env::var_os("PATH")
        .as_deref()
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|directory| directory.join("WorkBuddy.exe"))
        .collect::<Vec<_>>();
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        let root = PathBuf::from(root);
        candidates.extend([
            root.join("Programs/WorkBuddy/WorkBuddy.exe"),
            root.join("Tencent/WorkBuddy/WorkBuddy.exe"),
        ]);
    }
    for root in [
        std::env::var_os("ProgramFiles"),
        std::env::var_os("ProgramFiles(x86)"),
    ]
    .into_iter()
    .flatten()
    {
        candidates.push(PathBuf::from(root).join("WorkBuddy/WorkBuddy.exe"));
    }
    let mut discovered: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if candidate.is_file()
            && !discovered
                .iter()
                .any(|existing| paths_equal_ignore_ascii_case(existing, &candidate))
        {
            discovered.push(candidate);
        }
    }
    discovered
}
