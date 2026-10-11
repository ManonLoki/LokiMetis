// 本文件是 workbuddy.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

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
