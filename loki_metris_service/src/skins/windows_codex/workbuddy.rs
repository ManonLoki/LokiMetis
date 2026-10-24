//! Windows WorkBuddy 主进程的发现、验证、启动和定向重启。

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_NO_MORE_FILES, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    TerminateProcess, WaitForSingleObject,
};

use super::super::{CODEX_FORCE_CLOSE_TIMEOUT, CODEX_PAGE_POLL_INTERVAL};
use super::install_identity::{
    KnownInstallRoots, TrustedPublisher, fixed_existing_executable,
    is_trusted_traditional_executable, paths_equal_ignore_ascii_case,
};
use super::port_owner::loopback_listener_owner;
use super::process_query::{process_belongs_to_verified_root, query_verified_process_command_line};
use super::{
    AppError, GuiProcess, OwnedHandle, query_process_path, query_process_path_from_handle,
    utf16_array_to_string,
};

#[derive(Debug)]
/// 保存已通过官方路径验证的 WorkBuddy 进程及其直接父进程。
struct WorkBuddyProcess {
    /// 已验证的进程。
    process: GuiProcess,
    /// 直接父进程 PID。
    parent_pid: u32,
}

#[derive(Debug)]
/// 保存同一次 ToolHelp 快照中的可信 WorkBuddy 进程与完整父进程关系。
struct WorkBuddySnapshot {
    /// 可信 WorkBuddy 进程。
    processes: Vec<WorkBuddyProcess>,
    /// 进程到父进程的映射。
    parents: HashMap<u32, u32>,
}

#[derive(Debug)]
/// 保存可信 WorkBuddy 进程树及可选命令行；命令行不参与进程身份判定。
struct InspectedWorkBuddyProcesses {
    /// 可信进程及其命令行。
    processes: Vec<(WorkBuddyProcess, String)>,
    /// 进程到父进程的映射。
    parents: HashMap<u32, u32>,
}

/// 检查经过安装路径验证的 WorkBuddy 主进程是否存在。
pub(crate) async fn workbuddy_is_gui_running() -> Result<bool, AppError> {
    Ok(!enumerate_process_snapshot(&discover_executables())?
        .processes
        .is_empty())
}

/// 返回 WorkBuddy 主进程及其命令行，辅助进程不会进入结果。
pub(crate) async fn workbuddy_gui_processes() -> Result<Vec<(GuiProcess, String)>, AppError> {
    let inspected = inspected_workbuddy_processes().await?;
    if inspected.processes.is_empty() {
        return Ok(Vec::new());
    }
    Ok(
        select_workbuddy_roots(inspected.processes, &inspected.parents)
            .into_iter()
            .map(|(process, command_line)| (process.process, command_line))
            .collect(),
    )
}

/// 返回所有经过官方路径验证的 WorkBuddy 进程命令行，用于发现 renderer 暴露的 CDP 端口。
pub(crate) async fn workbuddy_gui_process_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    Ok(inspected_workbuddy_processes()
        .await?
        .processes
        .into_iter()
        .map(|(process, line)| (process.process.pid(), line))
        .collect())
}

/// 验证指定回环监听端口的唯一 owner 确实位于唯一的官方 WorkBuddy 进程树内。
pub(crate) fn workbuddy_endpoint_owned_by_root(
    port: u16,
    expected_root_pid: u32,
) -> Result<bool, AppError> {
    let snapshot = enumerate_process_snapshot(&discover_executables())?;
    let verified_pids = snapshot
        .processes
        .iter()
        .map(|process| process.process.pid)
        .collect::<HashSet<_>>();
    let roots = snapshot
        .processes
        .iter()
        .filter(|process| !has_verified_ancestor(process, &verified_pids, &snapshot.parents))
        .map(|process| process.process.pid)
        .collect::<Vec<_>>();
    if roots.as_slice() != [expected_root_pid] {
        return Ok(false);
    }

    let Some(owner_pid) =
        loopback_listener_owner(port).map_err(|_| cdp_owner_inspection_error())?
    else {
        return Ok(false);
    };
    Ok(process_belongs_to_verified_root(
        owner_pid,
        expected_root_pid,
        &verified_pids,
        &snapshot.parents,
    ))
}

/// 返回不暴露进程或端口详情的 CDP owner 检查错误。
fn cdp_owner_inspection_error() -> AppError {
    AppError::new(
        "skin.workbuddy_cdp_owner_inspection_failed",
        "无法验证 WorkBuddy 调试端口所属进程，未应用皮肤。",
    )
}

/// 合并 ToolHelp 的可信路径/父子关系与原生命令行，不把命令行当作进程身份。
async fn inspected_workbuddy_processes() -> Result<InspectedWorkBuddyProcesses, AppError> {
    let snapshot = enumerate_process_snapshot(&discover_executables())?;
    if snapshot.processes.is_empty() {
        return Ok(InspectedWorkBuddyProcesses {
            processes: Vec::new(),
            parents: snapshot.parents,
        });
    }
    let command_lines = snapshot
        .processes
        .iter()
        .filter_map(|process| {
            query_verified_process_command_line(process.process.pid, &process.process.path)
                .map(|line| (process.process.pid, line))
        })
        .collect::<HashMap<_, _>>();
    Ok(InspectedWorkBuddyProcesses {
        processes: attach_command_lines(snapshot.processes, &command_lines),
        parents: snapshot.parents,
    })
}

/// 为路径已经验证的进程附加命令行；缺失值只会降级参数/端口发现。
fn attach_command_lines(
    processes: Vec<WorkBuddyProcess>,
    command_lines: &HashMap<u32, String>,
) -> Vec<(WorkBuddyProcess, String)> {
    processes
        .into_iter()
        .map(|process| {
            let command_line = command_lines
                .get(&process.process.pid)
                .cloned()
                .unwrap_or_default();
            (process, command_line)
        })
        .collect()
}

/// 只保留没有可信 WorkBuddy 祖先的树根，允许祖先链中存在其它辅助程序。
fn select_workbuddy_roots(
    processes: Vec<(WorkBuddyProcess, String)>,
    parents: &HashMap<u32, u32>,
) -> Vec<(WorkBuddyProcess, String)> {
    let verified_pids = processes
        .iter()
        .map(|(process, _)| process.process.pid)
        .collect::<HashSet<_>>();
    processes
        .into_iter()
        .filter(|(process, _)| !has_verified_ancestor(process, &verified_pids, parents))
        .collect()
}

/// 沿同一系统快照的父链查找可信祖先，并以已访问集合防御异常环。
fn has_verified_ancestor(
    process: &WorkBuddyProcess,
    verified_pids: &HashSet<u32>,
    parents: &HashMap<u32, u32>,
) -> bool {
    let mut ancestor = process.parent_pid;
    let mut visited = HashSet::new();
    while ancestor != 0 && ancestor != process.process.pid && visited.insert(ancestor) {
        if verified_pids.contains(&ancestor) {
            return true;
        }
        ancestor = parents.get(&ancestor).copied().unwrap_or_default();
    }
    false
}

/// 在没有既存可信实例时启动官方 WorkBuddy，并设置本机调试端口。
pub(crate) async fn launch_workbuddy(port: u16) -> Result<(), AppError> {
    let candidates = discover_executables();
    if !enumerate_process_snapshot(&candidates)?
        .processes
        .is_empty()
    {
        return Err(AppError::new(
            "skin.workbuddy_recovery_required",
            "WorkBuddy 正在运行但未开放皮肤所需的调试端口，需要确认后恢复调试连接。",
        ));
    }
    let expected_path = candidates.into_iter().next().ok_or_else(|| {
        AppError::new(
            "skin.workbuddy_not_found",
            "未找到官方 WorkBuddy 桌面应用，请先安装或更新应用。",
        )
    })?;
    let executable = discover_executables()
        .into_iter()
        .find(|candidate| paths_equal_ignore_ascii_case(candidate, &expected_path))
        .ok_or_else(|| {
            AppError::new(
                "skin.workbuddy_not_found",
                "WorkBuddy 安装路径在启动前发生变化，请重新安装或刷新。",
            )
        })?;
    tokio::process::Command::new(executable)
        .env("WORKBUDDY_REMOTE_DEBUGGING_PORT", port.to_string())
        .spawn()
        .map_err(|_| AppError::new("skin.workbuddy_launch_failed", "无法启动 WorkBuddy。"))?;
    Ok(())
}

/// 复核所选实例身份后关闭全部旧进程，再按原参数重启 WorkBuddy。
pub(crate) async fn restart_workbuddy_gui_process(
    pid: u32,
    expected_path: &Path,
    arguments: &[String],
    port: u16,
) -> Result<(), AppError> {
    let unchanged = workbuddy_gui_processes()
        .await?
        .into_iter()
        .map(|(process, _)| process)
        .any(|process| {
            process.pid == pid && paths_equal_ignore_ascii_case(&process.path, expected_path)
        });
    if !unchanged {
        return Err(AppError::new(
            "skin.workbuddy_instance_changed",
            "所选 WorkBuddy 实例已退出或身份发生变化，请重新选择。",
        ));
    }
    force_close_workbuddy_gui().await?;
    let executable = discover_executables()
        .into_iter()
        .find(|candidate| paths_equal_ignore_ascii_case(candidate, expected_path))
        .ok_or_else(|| {
            AppError::new(
                "skin.workbuddy_instance_changed",
                "WorkBuddy 安装路径在重启前发生变化，未启动替代进程。",
            )
        })?;
    tokio::process::Command::new(executable)
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

/// 在超时内逐轮复核并关闭全部官方 WorkBuddy 进程，返回涉及的 PID 数量。
pub(crate) async fn force_close_workbuddy_gui() -> Result<usize, AppError> {
    let candidates = discover_executables();
    let deadline = tokio::time::Instant::now() + CODEX_FORCE_CLOSE_TIMEOUT;
    let mut targeted = HashSet::new();
    let mut termination_requested = false;
    while tokio::time::Instant::now() < deadline {
        let processes = match enumerate_process_snapshot(&candidates) {
            Ok(snapshot) => snapshot.processes,
            Err(error) if termination_requested => {
                return Err(force_close_incomplete_error(&error));
            }
            Err(error) => return Err(error),
        };
        if processes.is_empty() {
            return Ok(targeted.len());
        }
        for process in processes {
            targeted.insert(process.process.pid);
            let handle = match open_verified_process(&process.process) {
                Ok(handle) => handle,
                Err(error) if termination_requested => {
                    return Err(force_close_incomplete_error(&error));
                }
                Err(error) => return Err(error),
            };
            let Some(handle) = handle else {
                continue;
            };
            // SAFETY: handle 由 open_verified_process 在复核身份后以终止权限打开并由 OwnedHandle 持有，本次调用期间保持有效。
            match unsafe { TerminateProcess(handle.0, 0) } {
                Ok(()) => termination_requested = true,
                Err(_) => tracing::warn!("WorkBuddy 进程在关闭期间已结束或状态变化"),
            }
        }
        tokio::time::sleep(CODEX_PAGE_POLL_INTERVAL).await;
    }
    Err(AppError::new(
        "skin.workbuddy_force_close_timeout",
        "旧 WorkBuddy 进程未能完全退出，未启动新实例。",
    ))
}

/// 表示关闭已部分执行但后续身份复核失败，禁止直接启动替代实例。
fn force_close_incomplete_error(cause: &AppError) -> AppError {
    AppError::with_details(
        "skin.workbuddy_force_close_incomplete",
        "关闭旧 WorkBuddy 时无法继续验证进程；可能已有部分旧实例关闭，未启动新实例。",
        vec![format!("cause_code={}", cause.code)],
    )
}

/// 用 ToolHelp 建立进程与父链快照，并仅接纳路径匹配官方候选的 WorkBuddy。
fn enumerate_process_snapshot(candidates: &[PathBuf]) -> Result<WorkBuddySnapshot, AppError> {
    // SAFETY: 创建进程快照不读写调用方内存；返回的句柄立即交给 OwnedHandle 唯一持有并负责关闭。
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
    // SAFETY: snapshot 句柄在本函数内有效；entry 已设置 dwSize 且为本栈可写结构。
    unsafe { Process32FirstW(snapshot.0, &mut entry) }.map_err(|_| process_inspection_error())?;
    let mut processes = Vec::new();
    let mut parents = HashMap::new();
    loop {
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        let name = utf16_array_to_string(&entry.szExeFile);
        if name.eq_ignore_ascii_case("WorkBuddy.exe") {
            let path = match query_process_path(entry.th32ProcessID) {
                Some(path) => Some(path),
                None if !snapshot_still_contains_named_process(
                    entry.th32ProcessID,
                    "WorkBuddy.exe",
                )? =>
                {
                    None
                }
                None => query_process_path(entry.th32ProcessID)
                    .ok_or_else(process_inspection_error)
                    .map(Some)?,
            };
            if let Some(path) = path
                && candidates
                    .iter()
                    .any(|candidate| paths_equal_ignore_ascii_case(&path, candidate))
                && is_trusted_traditional_executable(&path, TrustedPublisher::Tencent)
            {
                processes.push(WorkBuddyProcess {
                    process: GuiProcess {
                        pid: entry.th32ProcessID,
                        path,
                    },
                    parent_pid: entry.th32ParentProcessID,
                });
            }
        }
        // SAFETY: 与 Process32FirstW 相同的有效快照句柄与可写 entry。
        match unsafe { Process32NextW(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(_) => return Err(process_inspection_error()),
        }
    }
    Ok(WorkBuddySnapshot { processes, parents })
}

/// 路径查询失败时用新快照区分已退出进程与仍存活但不可验证的同名进程。
fn snapshot_still_contains_named_process(pid: u32, expected_name: &str) -> Result<bool, AppError> {
    // SAFETY: 创建进程快照不读写调用方内存；返回的句柄立即交给 OwnedHandle 唯一持有并负责关闭。
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(OwnedHandle)
        .map_err(|_| process_inspection_error())?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: snapshot 句柄在本函数内有效；entry 已设置 dwSize 且为本栈可写结构。
    match unsafe { Process32FirstW(snapshot.0, &mut entry) } {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => return Ok(false),
        Err(_) => return Err(process_inspection_error()),
    }
    loop {
        if entry.th32ProcessID == pid
            && utf16_array_to_string(&entry.szExeFile).eq_ignore_ascii_case(expected_name)
        {
            return Ok(true);
        }
        // SAFETY: 与 Process32FirstW 相同的有效快照句柄与可写 entry。
        match unsafe { Process32NextW(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => return Ok(false),
            Err(_) => return Err(process_inspection_error()),
        }
    }
}

/// ToolHelp 必须完整走到 `ERROR_NO_MORE_FILES`，其它错误不能被当作“已经排空”。
fn process_inspection_error() -> AppError {
    AppError::new(
        "skin.workbuddy_process_inspection_failed",
        "无法完整检查 WorkBuddy 进程，未执行启动或关闭。",
    )
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - workbuddy_process.rs：WorkBuddy 进程打开与身份复核、安装路径发现，以及本模块单元测试
include!("workbuddy_process.rs");
