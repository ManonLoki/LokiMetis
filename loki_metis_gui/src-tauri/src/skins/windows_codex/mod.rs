//! 在 Windows 上发现、验证、激活和定向重启 Codex GUI 进程。

use std::collections::HashSet;
use std::ffi::OsStr;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoAllowSetForegroundWindow, CoCreateInstance,
    CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::Shell::{
    AO_NONE, ApplicationActivationManager, IApplicationActivationManager,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GW_OWNER, GetWindow,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, SW_RESTORE, SetForegroundWindow,
    ShowWindowAsync,
};
use windows::core::{BOOL, PCWSTR, PWSTR, w};
use wmi::WMIConnection;

use super::error::AppError;

const STORE_PACKAGE_FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";
const PROCESS_PATH_CAPACITY: usize = 32_768;
const PROCESS_COMMAND_LINE_TIMEOUT: Duration = Duration::from_secs(2);

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
/// 执行 Windows Codex 宿主中的 `WmiProcess` 步骤。
struct WmiProcess {
    process_id: u32,
    command_line: Option<String>,
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
    let traditional_candidates = discover_traditional_executables();
    Ok(!enumerate_gui_processes(&traditional_candidates)?.is_empty())
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
    let traditional_candidates = discover_traditional_executables();
    let verified = enumerate_gui_processes(&traditional_candidates)?;
    if verified.is_empty() {
        return Ok(Vec::new());
    }
    let query = tokio::task::spawn_blocking(query_gui_process_command_lines);
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
        .filter(|process| {
            query_process_path(process.pid)
                .is_some_and(|path| paths_equal_ignore_ascii_case(&path, &process.path))
        })
        .filter_map(|process| {
            let command_line = command_lines.get(&process.pid)?.clone();
            Some((process, command_line))
        })
        .collect())
}

/// 执行 Windows Codex 宿主中的 `activate_gui_process` 步骤。
pub(crate) fn activate_gui_process(pid: u32) -> bool {
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
    let traditional_candidates = discover_traditional_executables();
    let process = enumerate_gui_processes(&traditional_candidates)?
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
    unsafe { TerminateProcess(handle.0, 0) }.map_err(|_| {
        AppError::new(
            "skin.codex_force_close_failed",
            "无法关闭所选 Codex 实例，请保存工作后手动退出。",
        )
    })?;
    drop(handle);
    let debug_arguments = [
        "--remote-debugging-address=127.0.0.1".to_owned(),
        format!("--remote-debugging-port={port}"),
    ];
    if is_store_gui_path(expected_path) {
        let activation_arguments = arguments
            .iter()
            .chain(debug_arguments.iter())
            .map(|argument| quote_windows_argument(argument))
            .collect::<Vec<_>>()
            .join(" ");
        return tokio::task::spawn_blocking(move || {
            activate_store_codex_with_arguments(&activation_arguments)
        })
        .await
        .map_err(|_| AppError::new("skin.codex_launch_failed", "无法重新激活所选 Codex 实例。"))?
        .map_err(|_| AppError::new("skin.codex_launch_failed", "无法重新激活所选 Codex 实例。"));
    }
    let mut command = tokio::process::Command::new(expected_path);
    command.args(arguments).args(debug_arguments);
    command.spawn().map_err(|_| {
        AppError::new(
            "skin.codex_launch_failed",
            "所选 Codex 实例已关闭，但无法使用原启动参数重新打开。",
        )
    })?;
    Ok(())
}

/// 执行 Windows Codex 宿主中的 `query_gui_process_command_lines` 步骤。
fn query_gui_process_command_lines() -> Result<Vec<WmiProcess>, ()> {
    let connection = WMIConnection::new().map_err(|_| ())?;
    connection
        .raw_query(
            "SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name = 'ChatGPT.exe' OR Name = 'Codex.exe'",
        )
        .map_err(|_| ())
}

/// 执行 Windows Codex 宿主中的 `filter_verified_command_lines` 步骤。
fn filter_verified_command_lines(
    verified: &[GuiProcess],
    rows: Vec<WmiProcess>,
) -> Vec<(u32, String)> {
    let verified = verified
        .iter()
        .map(|process| process.pid)
        .collect::<HashSet<_>>();
    rows.into_iter()
        .filter_map(|row| {
            verified.contains(&row.process_id).then_some(())?;
            Some((row.process_id, row.command_line?))
        })
        .collect()
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
    let traditional_candidates = discover_traditional_executables();
    let processes = enumerate_gui_processes(&traditional_candidates)?;
    let count = processes.len();
    let handles = processes
        .iter()
        .map(open_verified_process)
        .collect::<Result<Vec<_>, _>>()?;
    for handle in handles {
        if unsafe { TerminateProcess(handle.0, 0) }.is_err() {
            tracing::warn!("Codex GUI 进程在关闭期间已结束或状态变化");
        }
    }
    if count > 0 {
        tracing::info!("已关闭受支持的 Codex GUI 进程，数量={count}");
    }
    Ok(count)
}

/// 执行 Windows Codex 宿主中的 `launch` 步骤。
pub async fn launch() -> Result<(), AppError> {
    let traditional_candidates = discover_traditional_executables();
    if !enumerate_gui_processes(&traditional_candidates)?.is_empty() {
        return Err(AppError::new(
            "skin.codex_manual_close_required",
            "Codex 正在运行但未开放皮肤所需的调试端口。请保存工作并手动完全退出 Codex，再由LokiMetis启动。",
        ));
    }

    let store_started = tokio::task::spawn_blocking(activate_store_codex)
        .await
        .map_err(|_| AppError::new("skin.codex_launch_failed", "无法启动 Codex。"))?
        .is_ok();
    let traditional = (!store_started)
        .then(|| traditional_candidates.into_iter().next())
        .flatten();
    let store_known = store_package_data_exists();

    match decide_launch(store_started, traditional, store_known) {
        LaunchDecision::StartedStore => {
            tracing::info!("已通过 Windows 应用激活服务启动 Codex");
            Ok(())
        }
        LaunchDecision::StartTraditional(executable) => {
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
        LaunchDecision::StoreFailed => Err(AppError::new(
            "skin.codex_launch_failed",
            "无法通过 Windows 应用激活服务启动 Codex。",
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
) -> Result<Vec<GuiProcess>, AppError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(OwnedHandle)
        .map_err(|_| AppError::new("skin.codex_launch_failed", "无法检查 Codex 运行状态。"))?;
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
        if is_candidate_process_name(&name) {
            if let Some(path) = query_process_path(entry.th32ProcessID) {
                if is_verified_gui_process_path(&path, traditional_candidates) {
                    processes.push(GuiProcess {
                        pid: entry.th32ProcessID,
                        path,
                    });
                }
            }
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }
    Ok(processes)
}

/// 执行 Windows Codex 宿主中的 `open_verified_process` 步骤。
fn open_verified_process(process: &GuiProcess) -> Result<OwnedHandle, AppError> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            process.pid,
        )
    }
    .map(OwnedHandle)
    .map_err(|_| {
        AppError::new(
            "skin.codex_force_close_failed",
            "无法关闭当前 Codex/GPT 桌面应用，请保存工作后手动退出。",
        )
    })?;
    let current_path = query_process_path_from_handle(handle.0).ok_or_else(|| {
        AppError::new(
            "skin.codex_force_close_failed",
            "Codex/GPT 桌面应用进程状态已变化，未执行强制关闭。",
        )
    })?;
    if !paths_equal_ignore_ascii_case(&current_path, &process.path) {
        return Err(AppError::new(
            "skin.codex_force_close_failed",
            "Codex/GPT 桌面应用进程状态已变化，未执行强制关闭。",
        ));
    }
    Ok(handle)
}

/// 执行 Windows Codex 宿主中的 `query_process_path` 步骤。
fn query_process_path(pid: u32) -> Option<PathBuf> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .ok()
        .map(OwnedHandle)?;
    query_process_path_from_handle(handle.0)
}

/// 执行 Windows Codex 宿主中的 `query_process_path_from_handle` 步骤。
fn query_process_path_from_handle(handle: HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0_u16; PROCESS_PATH_CAPACITY];
    let mut length = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()?;
    buffer.truncate(length as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

/// 执行 Windows Codex 宿主中的 `utf16_array_to_string` 步骤。
fn utf16_array_to_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

/// 执行 Windows Codex 宿主中的 `is_candidate_process_name` 步骤。
fn is_candidate_process_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("ChatGPT.exe") || name.eq_ignore_ascii_case("Codex.exe")
}

/// 执行 Windows Codex 宿主中的 `is_gui_executable` 步骤。
fn is_gui_executable(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    if name.eq_ignore_ascii_case("ChatGPT.exe") {
        return true;
    }
    name.eq_ignore_ascii_case("Codex.exe")
        && !is_local_codex_cli_path(path)
        && !path.components().any(|component| {
            component.as_os_str().to_str().is_some_and(|value| {
                value.eq_ignore_ascii_case("resources") || value.eq_ignore_ascii_case("WindowsApps")
            })
        })
}

/// 执行 Windows Codex 宿主中的 `is_local_codex_cli_path` 步骤。
fn is_local_codex_cli_path(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    components
        .windows(3)
        .any(|window| window[0] == "openai" && window[1] == "codex" && window[2] == "bin")
}

/// 执行 Windows Codex 宿主中的 `is_verified_gui_process_path` 步骤。
fn is_verified_gui_process_path(path: &Path, traditional_candidates: &[PathBuf]) -> bool {
    is_store_gui_path(path)
        || traditional_candidates
            .iter()
            .any(|candidate| paths_equal_ignore_ascii_case(path, candidate))
}

/// 执行 Windows Codex 宿主中的 `paths_equal_ignore_ascii_case` 步骤。
fn paths_equal_ignore_ascii_case(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

/// 执行 Windows Codex 宿主中的 `is_store_gui_path` 步骤。
fn is_store_gui_path(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("ChatGPT.exe"))
        && path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|value| value.to_ascii_lowercase().starts_with("openai.codex_"))
        })
}

/// 执行 Windows Codex 宿主中的 `activate_store_codex` 步骤。
fn activate_store_codex() -> Result<(), ()> {
    activate_store_codex_with_arguments(
        "--remote-debugging-address=127.0.0.1 --remote-debugging-port=9341",
    )
}

/// 执行 Windows Codex 宿主中的 `activate_store_codex_with_arguments` 步骤。
fn activate_store_codex_with_arguments(arguments: &str) -> Result<(), ()> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    initialized.ok().map_err(|_| ())?;
    let arguments = arguments
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = (|| {
        let manager: IApplicationActivationManager = unsafe {
            CoCreateInstance(
                &ApplicationActivationManager,
                None::<&windows::core::IUnknown>,
                CLSCTX_LOCAL_SERVER,
            )
        }
        .map_err(|_| ())?;
        let _ = unsafe { CoAllowSetForegroundWindow(&manager, None) };
        unsafe {
            manager.ActivateApplication(
                w!("OpenAI.Codex_2p2nqsd0c76g0!App"),
                PCWSTR(arguments.as_ptr()),
                AO_NONE,
            )
        }
        .map(|_| ())
        .map_err(|_| ())
    })();
    unsafe { CoUninitialize() };
    result
}

/// 执行 Windows Codex 宿主中的 `quote_windows_argument` 步骤。
fn quote_windows_argument(value: &str) -> String {
    if !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return value.to_owned();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;
    for character in value.chars() {
        if character == '\\' {
            backslashes += 1;
        } else if character == '"' {
            quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
            quoted.push('"');
            backslashes = 0;
        } else {
            quoted.extend(std::iter::repeat_n('\\', backslashes));
            backslashes = 0;
            quoted.push(character);
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// 执行 Windows Codex 宿主中的 `discover_traditional_executables` 步骤。
fn discover_traditional_executables() -> Vec<PathBuf> {
    let mut candidates = path_executable_candidates(std::env::var_os("PATH").as_deref());
    candidates.extend(traditional_candidates(
        std::env::var_os("LOCALAPPDATA").as_deref(),
        std::env::var_os("ProgramFiles").as_deref(),
        std::env::var_os("ProgramFiles(x86)").as_deref(),
    ));
    let mut discovered: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if is_gui_executable(&candidate)
            && candidate.is_file()
            && !discovered
                .iter()
                .any(|existing| paths_equal_ignore_ascii_case(existing, &candidate))
        {
            discovered.push(candidate);
        }
    }
    discovered
}

/// 执行 Windows Codex 宿主中的 `path_executable_candidates` 步骤。
fn path_executable_candidates(path: Option<&OsStr>) -> Vec<PathBuf> {
    path.into_iter()
        .flat_map(std::env::split_paths)
        .flat_map(|directory| {
            ["ChatGPT.exe", "Codex.exe"].map(|executable| directory.join(executable))
        })
        .collect()
}

/// 执行 Windows Codex 宿主中的 `traditional_candidates` 步骤。
fn traditional_candidates(
    local_app_data: Option<&OsStr>,
    program_files: Option<&OsStr>,
    program_files_x86: Option<&OsStr>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(root) = local_app_data {
        let root = PathBuf::from(root);
        candidates.extend([
            root.join("Programs/ChatGPT/ChatGPT.exe"),
            root.join("Programs/Codex/Codex.exe"),
            root.join("OpenAI/ChatGPT/ChatGPT.exe"),
            root.join("Microsoft/WindowsApps/ChatGPT.exe"),
        ]);
    }
    for root in [program_files, program_files_x86].into_iter().flatten() {
        let root = PathBuf::from(root);
        candidates.extend([
            root.join("ChatGPT/ChatGPT.exe"),
            root.join("OpenAI/ChatGPT/ChatGPT.exe"),
            root.join("Codex/Codex.exe"),
        ]);
    }
    candidates
}

/// 执行 Windows Codex 宿主中的 `store_package_data_exists` 步骤。
fn store_package_data_exists() -> bool {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .is_some_and(|root| root.join("Packages").join(STORE_PACKAGE_FAMILY).is_dir())
}

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
