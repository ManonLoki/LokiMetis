// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：可信 Codex 进程快照枚举、调试端口 owner 校验、
// 进程句柄的打开/终止/等待、进程路径查询与命令行参数转义，以及本模块单元测试入口。

/// 用一个完整 ToolHelp 快照枚举可信 Codex 进程并保留父进程关系。
fn enumerate_gui_process_snapshot(
    traditional_candidates: &[PathBuf],
    roots: &KnownInstallRoots,
) -> Result<CodexProcessSnapshot, AppError> {
    // SAFETY: 创建进程快照不读写调用方内存；返回的句柄立即交给 OwnedHandle 唯一持有并负责关闭。
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map(OwnedHandle)
        .map_err(|_| process_inspection_error())?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: snapshot 句柄在本函数内有效；entry 已设置 dwSize 且为本栈可写结构。
    if let Err(error) = unsafe { Process32FirstW(snapshot.0, &mut entry) } {
        return if error.code() == ERROR_NO_MORE_FILES.to_hresult() {
            Ok(CodexProcessSnapshot {
                processes: Vec::new(),
                parents: std::collections::HashMap::new(),
            })
        } else {
            Err(process_inspection_error())
        };
    }

    let mut processes = Vec::new();
    let mut parents = std::collections::HashMap::new();
    loop {
        parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
        let name = utf16_array_to_string(&entry.szExeFile);
        if is_candidate_process_name(&name) {
            let path = match query_process_path(entry.th32ProcessID) {
                Some(path) => Some(path),
                None if !snapshot_still_contains_candidate_process(entry.th32ProcessID)? => None,
                None => query_process_path(entry.th32ProcessID)
                    .ok_or_else(process_inspection_error)
                    .map(Some)?,
            };
            if let Some(path) = path
                && verified_process_identity(
                    entry.th32ProcessID,
                    &path,
                    traditional_candidates,
                    roots,
                )
            {
                processes.push(GuiProcess {
                    pid: entry.th32ProcessID,
                    path,
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
    Ok(CodexProcessSnapshot { processes, parents })
}

/// 路径查询失败时重新确认候选 PID 是否仍存在，避免把不可验证进程当成已退出。
fn snapshot_still_contains_candidate_process(pid: u32) -> Result<bool, AppError> {
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
            && is_candidate_process_name(&utf16_array_to_string(&entry.szExeFile))
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

/// 验证 CDP 端口的唯一回环 listener 属于指定可信 Codex 进程树。
pub(crate) fn codex_endpoint_owned_by_root(
    port: u16,
    expected_root_pid: u32,
) -> Result<bool, AppError> {
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    let snapshot = enumerate_gui_process_snapshot(&traditional_candidates, &roots)?;
    let verified_pids = snapshot
        .processes
        .iter()
        .map(|process| process.pid)
        .collect::<HashSet<_>>();
    let owner = loopback_listener_owner(port).map_err(|_| cdp_owner_inspection_error())?;
    Ok(owner.is_some_and(|owner_pid| {
        process_belongs_to_verified_root(
            owner_pid,
            expected_root_pid,
            &verified_pids,
            &snapshot.parents,
        )
    }))
}

/// 返回不暴露 PID 或端口的 Codex CDP owner 检查错误。
fn cdp_owner_inspection_error() -> AppError {
    AppError::new(
        "skin.codex_cdp_owner_inspection_failed",
        "无法验证 Codex 调试端口所属进程，未应用皮肤。",
    )
}

/// 窗口激活前重新验证 PID 属于当前可信 Codex 或 WorkBuddy 宿主。
fn is_verified_host_process(pid: u32) -> bool {
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    enumerate_gui_processes(&traditional_candidates, &roots)
        .is_ok_and(|processes| processes.iter().any(|process| process.pid == pid))
        || workbuddy::is_verified_gui_process(pid)
}

/// 以终止、查询与等待权限打开已校验进程，并再次核对句柄对应的进程身份。
fn open_verified_process(process: &GuiProcess) -> Result<OwnedHandle, AppError> {
    // SAFETY: 按 PID 打开进程只申请所列权限，不访问调用方内存；成功的句柄立即交给 OwnedHandle 持有。
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
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
    let roots = KnownInstallRoots::discover();
    let traditional_candidates = discover_traditional_executables(&roots);
    if !is_verified_gui_process(handle.0, &current_path, &traditional_candidates, &roots) {
        return Err(AppError::new(
            "skin.codex_force_close_failed",
            "Codex/GPT 桌面应用身份无法再次验证，未执行强制关闭。",
        ));
    }
    Ok(handle)
}

/// 用当前 PID 的查询句柄把路径身份与签名或 AppModel 包身份绑定。
fn verified_process_identity(
    pid: u32,
    path: &Path,
    traditional_candidates: &[PathBuf],
    roots: &KnownInstallRoots,
) -> bool {
    // SAFETY: 以受限查询权限按 PID 打开进程；成功的句柄随即交给 OwnedHandle 持有并关闭。
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };
    let handle = OwnedHandle(handle);
    is_verified_gui_process(handle.0, path, traditional_candidates, roots)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 将零等待的 Win32 结果归一化，避免把查询失败误当成进程退出。
enum ProcessWaitState {
    /// 进程已退出。
    Exited,
    /// 进程仍在运行。
    Running,
    /// 查询失败，不能当作已退出。
    Failed,
}

/// 把 Win32 等待返回值分为已退出、仍运行和不可确认三种状态。
fn classify_process_wait(result: windows::Win32::Foundation::WAIT_EVENT) -> ProcessWaitState {
    if result == WAIT_OBJECT_0 {
        ProcessWaitState::Exited
    } else if result == WAIT_TIMEOUT {
        ProcessWaitState::Running
    } else {
        ProcessWaitState::Failed
    }
}

/// 发出终止请求；Win32 失败必须返回给调用方，不能用日志吞掉。
fn request_process_termination(handle: &OwnedHandle) -> Result<(), AppError> {
    // SAFETY: handle 已经通过路径与安装身份复核，并以 PROCESS_TERMINATE 权限打开。
    unsafe { TerminateProcess(handle.0, 0) }.map_err(|_| {
        AppError::new(
            "skin.codex_force_close_failed",
            "无法关闭所选 Codex 实例，请保存工作后手动退出。",
        )
    })
}

/// 在共同截止时间前确认进程对象已进入 signaled 终态。
async fn wait_for_process_exit_until(
    handle: &OwnedHandle,
    deadline: tokio::time::Instant,
) -> Result<(), AppError> {
    loop {
        // SAFETY: handle 在整个 await 循环中由调用方持有，零等待不会阻塞 Tokio worker。
        match classify_process_wait(unsafe { WaitForSingleObject(handle.0, 0) }) {
            ProcessWaitState::Exited => return Ok(()),
            ProcessWaitState::Failed => {
                return Err(AppError::new(
                    "skin.codex_force_close_failed",
                    "无法确认旧 Codex 进程是否已经退出，未启动替代实例。",
                ));
            }
            ProcessWaitState::Running => {}
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(AppError::new(
                "skin.codex_force_close_timeout",
                "旧 Codex 进程未能在限定时间内退出，未启动替代实例。",
            ));
        }
        tokio::time::sleep(CODEX_PAGE_POLL_INTERVAL.min(deadline.saturating_duration_since(now)))
            .await;
    }
}

/// 以受限查询权限打开 PID 并读取其可执行文件路径。
fn query_process_path(pid: u32) -> Option<PathBuf> {
    // SAFETY: 以受限查询权限按 PID 打开进程；成功的句柄随即交给 OwnedHandle 持有并关闭。
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .ok()
        .map(OwnedHandle)?;
    query_process_path_from_handle(handle.0)
}

/// 从进程句柄读取可执行文件的完整 Win32 路径。
fn query_process_path_from_handle(handle: HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0_u16; PROCESS_PATH_CAPACITY];
    let mut length = buffer.len() as u32;
    // SAFETY: handle 由调用方保证有效且具备查询权限；buffer 可写长度与传入的 length 一致，返回后按 length 截断。
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

/// 把以 NUL 结尾的 UTF-16 定长数组转换为字符串。
fn utf16_array_to_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

/// 进程映像名是否为 Codex 桌面应用的候选名（不区分大小写）。
fn is_candidate_process_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("ChatGPT.exe") || name.eq_ignore_ascii_case("Codex.exe")
}

/// 按 Windows 命令行规则为参数加引号并转义引号与结尾反斜杠。
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

#[cfg(test)]
mod tests {
    include!("tests.rs");
}
