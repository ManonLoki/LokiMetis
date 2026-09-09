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
use super::process_query::query_verified_process_command_line;
use super::{
    AppError, GuiProcess, OwnedHandle, query_process_path, query_process_path_from_handle,
    utf16_array_to_string,
};

#[derive(Debug)]
/// 保存已通过官方路径验证的 WorkBuddy 进程及其直接父进程。
struct WorkBuddyProcess {
    process: GuiProcess,
    parent_pid: u32,
}

#[derive(Debug)]
/// 保存同一次 ToolHelp 快照中的可信 WorkBuddy 进程与完整父进程关系。
struct WorkBuddySnapshot {
    processes: Vec<WorkBuddyProcess>,
    parents: HashMap<u32, u32>,
}

#[derive(Debug)]
/// 保存可信 WorkBuddy 进程树及可选命令行；命令行不参与进程身份判定。
struct InspectedWorkBuddyProcesses {
    processes: Vec<(WorkBuddyProcess, String)>,
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

/// 沿可信进程快照父链确认 listener owner 属于指定 WorkBuddy 根。
fn process_belongs_to_verified_root(
    process_pid: u32,
    root_pid: u32,
    verified_pids: &HashSet<u32>,
    parents: &HashMap<u32, u32>,
) -> bool {
    if !verified_pids.contains(&process_pid) || !verified_pids.contains(&root_pid) {
        return false;
    }
    let mut current = process_pid;
    let mut visited = HashSet::new();
    while current != 0 && visited.insert(current) {
        if current == root_pid {
            return true;
        }
        current = parents.get(&current).copied().unwrap_or_default();
    }
    false
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
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(OwnedHandle)
        .map_err(|_| process_inspection_error())?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
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

/// 打开可终止句柄后再次核对路径；进程已退出时安全返回空值。
fn open_verified_process(process: &GuiProcess) -> Result<Option<OwnedHandle>, AppError> {
    let handle = match unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            process.pid,
        )
    } {
        Ok(handle) => OwnedHandle(handle),
        Err(_) => {
            if !snapshot_still_contains_named_process(process.pid, "WorkBuddy.exe")? {
                return Ok(None);
            }
            return Err(AppError::new(
                "skin.workbuddy_force_close_failed",
                "无法关闭 WorkBuddy，请保存工作后手动退出。",
            ));
        }
    };
    let Some(current_path) = query_process_path_from_handle(handle.0) else {
        if unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 {
            return Ok(None);
        }
        return Err(AppError::new(
            "skin.workbuddy_force_close_failed",
            "WorkBuddy 进程仍在运行但无法复核路径，未继续强制关闭。",
        ));
    };
    if !paths_equal_ignore_ascii_case(&current_path, &process.path) {
        return Err(AppError::new(
            "skin.workbuddy_force_close_failed",
            "WorkBuddy 进程状态已变化，未执行强制关闭。",
        ));
    }
    if !discover_executables()
        .iter()
        .any(|candidate| paths_equal_ignore_ascii_case(candidate, &current_path))
    {
        return Err(AppError::new(
            "skin.workbuddy_force_close_failed",
            "WorkBuddy 进程身份无法再次验证，未执行强制关闭。",
        ));
    }
    Ok(Some(handle))
}

/// 从 Windows 官方程序目录收集去重且真实存在的 WorkBuddy 可执行文件。
fn discover_executables() -> Vec<PathBuf> {
    let roots = KnownInstallRoots::discover();
    let mut candidates = Vec::new();
    if let Some(root) = roots.local_app_data.as_ref() {
        candidates.extend([
            fixed_existing_executable(root, "Programs/WorkBuddy/WorkBuddy.exe"),
            fixed_existing_executable(root, "Tencent/WorkBuddy/WorkBuddy.exe"),
        ]);
    }
    for root in &roots.program_files {
        candidates.push(fixed_existing_executable(root, "WorkBuddy/WorkBuddy.exe"));
    }
    let mut discovered: Vec<PathBuf> = Vec::new();
    for candidate in candidates
        .into_iter()
        .flatten()
        .filter(|candidate| is_trusted_traditional_executable(candidate, TrustedPublisher::Tencent))
    {
        if !discovered
            .iter()
            .any(|existing| paths_equal_ignore_ascii_case(existing, &candidate))
        {
            discovered.push(candidate);
        }
    }
    discovered
}

/// 窗口激活前确认 PID 当前仍属于可信 WorkBuddy 安装路径。
pub(super) fn is_verified_gui_process(pid: u32) -> bool {
    enumerate_process_snapshot(&discover_executables()).is_ok_and(|snapshot| {
        snapshot
            .processes
            .iter()
            .any(|process| process.process.pid == pid)
    })
}

#[cfg(test)]
mod tests {
    use super::super::port_owner::unique_loopback_listener_owner;
    use super::{
        GuiProcess, WorkBuddyProcess, attach_command_lines, process_belongs_to_verified_root,
        select_workbuddy_roots,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use windows::Win32::NetworkManagement::IpHelper::{
        MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    };

    /// 构造不接触真实进程的 WorkBuddy 树节点。
    fn process(pid: u32, parent_pid: u32, arguments: &str) -> (WorkBuddyProcess, String) {
        (
            WorkBuddyProcess {
                process: GuiProcess {
                    pid,
                    path: PathBuf::from(
                        r"C:\Users\test\AppData\Local\Programs\WorkBuddy\WorkBuddy.exe",
                    ),
                },
                parent_pid,
            },
            format!(r#"WorkBuddy.exe {arguments}"#),
        )
    }

    #[test]
    /// 验证 renderer 与无 `--type` 的脚本子进程不会被误算为多个 GUI 实例。
    fn workbuddy_root_filter_excludes_same_executable_descendants() {
        let processes = vec![
            process(100, 10, ""),
            process(101, 100, "--type=renderer --remote-debugging-port=9441"),
            process(102, 100, "daemon.js --stdio"),
            process(103, 102, "sidecar.js --token=<redacted>"),
            process(104, 103, "--require bootstrap.cjs"),
        ];
        let parents = processes
            .iter()
            .map(|(process, _)| (process.process.pid, process.parent_pid))
            .collect();
        let roots = select_workbuddy_roots(processes, &parents);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].0.process.pid, 100);
    }

    #[test]
    /// 验证非 WorkBuddy 中间进程不会把同树后代提升为第二个 GUI 根。
    fn workbuddy_root_filter_walks_through_other_processes() {
        let processes = vec![process(100, 10, ""), process(101, 150, "daemon.js --stdio")];
        let parents = HashMap::from([(100, 10), (150, 100), (101, 150)]);
        let roots = select_workbuddy_roots(processes, &parents);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].0.process.pid, 100);
    }

    #[test]
    /// 验证根进程命令行暂时缺失时仍保留根身份，而不是让子进程冒充实例。
    fn missing_root_command_line_does_not_remove_the_root() {
        let raw = vec![process(100, 10, "").0, process(101, 100, "").0];
        let command_lines = HashMap::from([(
            101,
            "WorkBuddy.exe --type=renderer --remote-debugging-port=9441".into(),
        )]);
        let processes = attach_command_lines(raw, &command_lines);
        let parents = HashMap::from([(100, 10), (101, 100)]);
        let roots = select_workbuddy_roots(processes, &parents);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].0.process.pid, 100);
        assert!(roots[0].1.is_empty());
    }

    #[test]
    /// 验证两个彼此独立的 WorkBuddy 树根仍保持为两个实例，不能被静默合并。
    fn workbuddy_root_filter_preserves_independent_roots() {
        let processes = vec![
            process(100, 10, ""),
            process(101, 100, "--type=renderer --remote-debugging-port=9441"),
            process(200, 20, ""),
            process(201, 200, "--type=renderer --remote-debugging-port=9442"),
        ];
        let parents = processes
            .iter()
            .map(|(process, _)| (process.process.pid, process.parent_pid))
            .collect();
        let roots = select_workbuddy_roots(processes, &parents);
        assert_eq!(
            roots
                .into_iter()
                .map(|(process, _)| process.process.pid)
                .collect::<Vec<_>>(),
            vec![100, 200]
        );
    }

    #[test]
    /// CDP listener 必须是指定回环端口的唯一 owner，通配地址与多个 owner 均拒绝。
    fn workbuddy_listener_owner_requires_unique_loopback_binding() {
        let row = |address: [u8; 4], port: u16, pid: u32| MIB_TCPROW_OWNER_PID {
            dwLocalAddr: u32::from_ne_bytes(address),
            dwLocalPort: u16::to_be(port) as u32,
            dwOwningPid: pid,
            ..Default::default()
        };
        assert_eq!(
            unique_loopback_listener_owner(&[row([127, 0, 0, 1], 9442, 101)], &[], 9442),
            Some(101)
        );
        assert_eq!(
            unique_loopback_listener_owner(
                &[
                    row([127, 0, 0, 1], 9442, 101),
                    row([127, 0, 0, 1], 9442, 101),
                ],
                &[],
                9442,
            ),
            Some(101)
        );
        assert_eq!(
            unique_loopback_listener_owner(
                &[
                    row([127, 0, 0, 1], 9442, 101),
                    row([127, 0, 0, 2], 9442, 202),
                ],
                &[],
                9442,
            ),
            None
        );
        assert_eq!(
            unique_loopback_listener_owner(&[row([0, 0, 0, 0], 9442, 101)], &[], 9442),
            None
        );
        assert_eq!(
            unique_loopback_listener_owner(
                &[row([127, 0, 0, 1], 9442, 101)],
                &[MIB_TCP6ROW_OWNER_PID {
                    ucLocalAddr: [0; 16],
                    dwLocalPort: u16::to_be(9442) as u32,
                    dwOwningPid: 101,
                    ..Default::default()
                }],
                9442,
            ),
            None
        );
    }

    #[test]
    /// renderer owner 可跨非 WorkBuddy 中间进程归属于唯一官方根，但未知 owner 不可接受。
    fn workbuddy_listener_owner_must_belong_to_verified_root() {
        let verified = HashSet::from([100, 101]);
        let parents = HashMap::from([(100, 10), (150, 100), (101, 150)]);
        assert!(process_belongs_to_verified_root(
            101, 100, &verified, &parents
        ));
        assert!(!process_belongs_to_verified_root(
            150, 100, &verified, &parents
        ));
        assert!(!process_belongs_to_verified_root(
            101, 200, &verified, &parents
        ));
    }
}
