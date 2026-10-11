//! 在 Windows 上发现、验证、激活和定向重启 Codex GUI 进程。

mod install_identity;
mod port_owner;
mod process_query;
mod store_activation;
mod workbuddy;

pub(crate) use workbuddy::{
    force_close_workbuddy_gui, launch_workbuddy, restart_workbuddy_gui_process,
    workbuddy_endpoint_owned_by_root, workbuddy_gui_process_command_lines, workbuddy_gui_processes,
    workbuddy_is_gui_running,
};

use std::collections::HashSet;
use std::mem::size_of;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_FILES, HANDLE, HWND, LPARAM, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GW_OWNER, GetWindow,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, SW_RESTORE, SetForegroundWindow,
    ShowWindowAsync,
};
use windows::core::{BOOL, PWSTR};

use super::{CODEX_FORCE_CLOSE_TIMEOUT, CODEX_PAGE_POLL_INTERVAL, error::AppError};
use install_identity::{
    KnownInstallRoots, discover_traditional_executables, is_store_gui_path,
    is_verified_gui_process, paths_equal_ignore_ascii_case, revalidate_traditional_launch_path,
    store_package_data_exists,
};
use port_owner::loopback_listener_owner;
use process_query::{process_belongs_to_verified_root, query_verified_process_command_line};
use store_activation::activate_store_codex_owned;

const PROCESS_PATH_CAPACITY: usize = 32_768;

#[derive(Debug, PartialEq, Eq)]
/// 执行 Windows Codex 宿主中的 `LaunchDecision` 步骤。
enum LaunchDecision {
    StartedStore,
    StartTraditional(PathBuf),
    StoreFailed,
    NotFound,
}

/// 执行 Windows Codex 宿主中的 `OwnedHandle` 步骤。
struct OwnedHandle(HANDLE);

// SAFETY: OwnedHandle 仅包装可跨线程使用的 Win32 内核 HANDLE（进程或 ToolHelp 快照），
// 所有权唯一且 Drop 只关闭一次；具体读写仍由调用方的 Rust 所有权同步。
unsafe impl Send for OwnedHandle {}
// SAFETY: 对内核 HANDLE 的只读等待/查询可由不同线程执行，关闭仍由唯一 owner 负责。
unsafe impl Sync for OwnedHandle {}

#[derive(Clone, Copy)]
/// 执行 Windows Codex 宿主中的 `WindowCandidate` 步骤。
struct WindowCandidate {
    handle: HWND,
    pid: u32,
    visible: bool,
    owned: bool,
}

#[derive(Debug, PartialEq, Eq)]
/// 执行 Windows Codex 宿主中的 `GuiProcess` 步骤。
pub(crate) struct GuiProcess {
    pid: u32,
    path: PathBuf,
}

impl GuiProcess {
    /// 执行 Windows Codex 宿主中的 `pid` 步骤。
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// 执行 Windows Codex 宿主中的 `path` 步骤。
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
/// 保存同一次 ToolHelp 快照中的可信 Codex 进程与完整父进程关系。
struct CodexProcessSnapshot {
    processes: Vec<GuiProcess>,
    parents: std::collections::HashMap<u32, u32>,
}

impl Drop for OwnedHandle {
    /// 执行 Windows Codex 宿主中的 `drop` 步骤。
    fn drop(&mut self) {
        // SAFETY: 此类型只包装成功取得且由当前函数拥有的 Windows HANDLE。
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 执行 Windows Codex 宿主中的 `is_gui_running` 步骤。
pub async fn is_gui_running() -> Result<bool, AppError> {
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    Ok(!enumerate_gui_processes(&traditional_candidates, &roots)?.is_empty())
}

/// 执行 Windows Codex 宿主中的 `gui_process_command_lines` 步骤。
pub async fn gui_process_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    Ok(gui_processes()
        .await?
        .into_iter()
        .map(|(process, command_line)| (process.pid(), command_line))
        .collect())
}

/// 执行 Windows Codex 宿主中的 `gui_processes` 步骤。
pub async fn gui_processes() -> Result<Vec<(GuiProcess, String)>, AppError> {
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    let verified = enumerate_gui_processes(&traditional_candidates, &roots)?;
    if verified.is_empty() {
        return Ok(Vec::new());
    }
    verified
        .into_iter()
        .map(|process| {
            let command_line = query_verified_process_command_line(process.pid, &process.path)
                .ok_or_else(process_inspection_error)?;
            Ok((process, command_line))
        })
        .collect()
}

/// 执行 Windows Codex 宿主中的 `activate_gui_process` 步骤。
pub(crate) fn activate_gui_process(pid: u32) -> bool {
    if !is_verified_host_process(pid) {
        return false;
    }
    /// 执行 Windows Codex 宿主中的 `collect_window` 步骤。
    unsafe extern "system" fn collect_window(hwnd: HWND, parameter: LPARAM) -> BOOL {
        // SAFETY: LPARAM 指向本函数同步调用 EnumWindows 期间仍然存活的 Vec。
        let candidates = unsafe { &mut *(parameter.0 as *mut Vec<WindowCandidate>) };
        let mut window_pid = 0u32;
        // SAFETY: hwnd 由 EnumWindows 提供，输出指针指向当前栈变量。
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
        // SAFETY: hwnd 在当前枚举回调期间有效；查询不改变窗口状态。
        let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
        // SAFETY: hwnd 在当前枚举回调期间有效；这里只读取 owner。
        let owned = unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|owner| !owner.0.is_null());
        candidates.push(WindowCandidate {
            handle: hwnd,
            pid: window_pid,
            visible,
            owned,
        });
        BOOL(1)
    }

    let mut candidates = Vec::new();
    // SAFETY: 回调只在 EnumWindows 返回前使用 candidates 指针。
    let _ = unsafe {
        EnumWindows(
            Some(collect_window),
            LPARAM((&mut candidates as *mut Vec<WindowCandidate>) as isize),
        )
    };
    let Some(window) = select_target_window(&candidates, pid) else {
        return false;
    };
    // SAFETY: 窗口来自当前同步枚举；失败只表示系统拒绝激活。
    unsafe {
        let _ = AllowSetForegroundWindow(pid);
        if IsIconic(window).as_bool() {
            let _ = ShowWindowAsync(window, SW_RESTORE);
        }
        let _ = BringWindowToTop(window);
        SetForegroundWindow(window).as_bool()
    }
}

/// 执行 Windows Codex 宿主中的 `select_target_window` 步骤。
fn select_target_window(candidates: &[WindowCandidate], pid: u32) -> Option<HWND> {
    candidates
        .iter()
        .find(|candidate| candidate.pid == pid && candidate.visible && !candidate.owned)
        .map(|candidate| candidate.handle)
}

/// 执行 Windows Codex 宿主中的 `restart_gui_process` 步骤。
pub async fn restart_gui_process(
    pid: u32,
    expected_path: &Path,
    arguments: &[String],
    port: u16,
) -> Result<(), AppError> {
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    let process = enumerate_gui_processes(&traditional_candidates, &roots)?
        .into_iter()
        .find(|process| {
            process.pid == pid && paths_equal_ignore_ascii_case(&process.path, expected_path)
        })
        .ok_or_else(|| {
            AppError::new(
                "skin.codex_instance_changed",
                "所选 Codex 实例已退出或身份发生变化，请重新选择。",
            )
        })?;
    let handle = open_verified_process(&process)?;
    request_process_termination(&handle)?;
    wait_for_process_exit_until(
        &handle,
        tokio::time::Instant::now() + CODEX_FORCE_CLOSE_TIMEOUT,
    )
    .await?;
    let debug_arguments = [
        "--remote-debugging-address=127.0.0.1".to_owned(),
        format!("--remote-debugging-port={port}"),
    ];
    if is_store_gui_path(expected_path, &roots) {
        let activation_arguments = arguments
            .iter()
            .chain(debug_arguments.iter())
            .map(|argument| quote_windows_argument(argument))
            .collect::<Vec<_>>()
            .join(" ");
        return activate_store_codex_owned(activation_arguments)
            .await?
            .map_err(|failure| {
                AppError::with_details(
                    "skin.codex_launch_failed",
                    "无法重新激活所选 Codex 实例。",
                    vec![format!("activation_stage={}", failure.reason())],
                )
            });
    }
    let executable =
        revalidate_traditional_launch_path(expected_path, &roots).ok_or_else(|| {
            AppError::new(
                "skin.codex_instance_changed",
                "Codex 安装路径在重启前发生变化，未启动替代进程。",
            )
        })?;
    let mut command = tokio::process::Command::new(executable);
    command.args(arguments).args(debug_arguments);
    command.spawn().map_err(|_| {
        AppError::new(
            "skin.codex_launch_failed",
            "所选 Codex 实例已关闭，但无法使用原启动参数重新打开。",
        )
    })?;
    Ok(())
}

/// 执行 Windows Codex 宿主中的 `process_inspection_error` 步骤。
fn process_inspection_error() -> AppError {
    AppError::new(
        "skin.codex_process_inspection_failed",
        "无法读取 Codex 调试启动参数。",
    )
}

/// 执行 Windows Codex 宿主中的 `force_close_gui` 步骤。
pub async fn force_close_gui() -> Result<usize, AppError> {
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    let processes = enumerate_gui_processes(&traditional_candidates, &roots)?;
    let count = processes.len();
    let handles = processes
        .iter()
        .map(open_verified_process)
        .collect::<Result<Vec<_>, _>>()?;
    let mut first_error = None;
    for handle in &handles {
        if let Err(error) = request_process_termination(handle) {
            first_error.get_or_insert(error);
        }
    }
    let deadline = tokio::time::Instant::now() + CODEX_FORCE_CLOSE_TIMEOUT;
    for handle in &handles {
        if let Err(error) = wait_for_process_exit_until(handle, deadline).await {
            first_error.get_or_insert(error);
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    if count > 0 {
        tracing::info!("已关闭受支持的 Codex GUI 进程，数量={count}");
    }
    Ok(count)
}

/// 执行 Windows Codex 宿主中的 `launch` 步骤。
pub async fn launch() -> Result<(), AppError> {
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    if !enumerate_gui_processes(&traditional_candidates, &roots)?.is_empty() {
        return Err(AppError::new(
            "skin.codex_manual_close_required",
            "Codex 正在运行但未开放皮肤所需的调试端口。请保存工作并手动完全退出 Codex，再由LokiMetris启动。",
        ));
    }

    let (store_started, store_failure) = match activate_store_codex_owned(
        "--remote-debugging-address=127.0.0.1 --remote-debugging-port=9341".to_owned(),
    )
    .await?
    {
        Ok(()) => (true, None),
        Err(failure) => {
            tracing::warn!(
                activation_stage = failure.reason(),
                "Windows Store Codex activation failed"
            );
            (false, Some(failure))
        }
    };
    let traditional = (!store_started)
        .then(|| traditional_candidates.into_iter().next())
        .flatten();
    let store_known = store_package_data_exists(&roots);

    match decide_launch(store_started, traditional, store_known) {
        LaunchDecision::StartedStore => {
            tracing::info!("已通过 Windows 应用激活服务启动 Codex");
            Ok(())
        }
        LaunchDecision::StartTraditional(executable) => {
            let executable =
                revalidate_traditional_launch_path(&executable, &roots).ok_or_else(|| {
                    AppError::new(
                        "skin.codex_not_found",
                        "Codex 安装路径在启动前发生变化，请重新安装或刷新。",
                    )
                })?;
            tokio::process::Command::new(executable)
                .args([
                    "--remote-debugging-address=127.0.0.1",
                    "--remote-debugging-port=9341",
                ])
                .spawn()
                .map_err(|_| AppError::new("skin.codex_launch_failed", "无法启动 Codex。"))?;
            tracing::info!("已通过传统桌面入口启动 Codex");
            Ok(())
        }
        LaunchDecision::StoreFailed => Err(AppError::with_details(
            "skin.codex_launch_failed",
            "无法通过 Windows 应用激活服务启动 Codex。",
            store_failure
                .map(|failure| vec![format!("activation_stage={}", failure.reason())])
                .unwrap_or_default(),
        )),
        LaunchDecision::NotFound => Err(AppError::new(
            "skin.codex_not_found",
            "未找到官方 ChatGPT/Codex 桌面应用，请先安装或更新应用。",
        )),
    }
}

/// 执行 Windows Codex 宿主中的 `decide_launch` 步骤。
fn decide_launch(
    store_started: bool,
    traditional: Option<PathBuf>,
    store_known: bool,
) -> LaunchDecision {
    if store_started {
        LaunchDecision::StartedStore
    } else if let Some(executable) = traditional {
        LaunchDecision::StartTraditional(executable)
    } else if store_known {
        LaunchDecision::StoreFailed
    } else {
        LaunchDecision::NotFound
    }
}

/// 执行 Windows Codex 宿主中的 `enumerate_gui_processes` 步骤。
fn enumerate_gui_processes(
    traditional_candidates: &[PathBuf],
    roots: &KnownInstallRoots,
) -> Result<Vec<GuiProcess>, AppError> {
    Ok(enumerate_gui_process_snapshot(traditional_candidates, roots)?.processes)
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - windows_codex_tests.rs：本模块的单元测试
include!("windows_codex_tests.rs");
