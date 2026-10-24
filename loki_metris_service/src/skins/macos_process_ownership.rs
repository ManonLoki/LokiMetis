// 本文件是 macos_process.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：宿主进程错误、回环监听 owner 解析与进程树归属判定，以及本模块单元测试

/// 返回进程身份在操作前发生变化的稳定错误。
fn instance_changed_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_instance_changed",
            SkinHostKind::WorkBuddy => "skin.workbuddy_instance_changed",
        },
        format!(
            "所选 {} 实例身份已变化，请刷新后重试。",
            host.display_name()
        ),
    )
}

/// 返回无法完整终止可信宿主进程的稳定错误。
fn force_close_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_force_close_failed",
            SkinHostKind::WorkBuddy => "skin.workbuddy_force_close_failed",
        },
        format!("无法关闭 {}，请保存工作后手动退出。", host.display_name()),
    )
}

/// 返回宿主未能在强制关闭期限内退出的稳定错误。
fn force_close_timeout_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_force_close_timeout",
            SkinHostKind::WorkBuddy => "skin.workbuddy_force_close_timeout",
        },
        format!(
            "{} 未能在关闭期限内退出，请保存工作后手动关闭。",
            host.display_name()
        ),
    )
}

/// 返回不暴露端口、PID 或安装路径的宿主 owner 校验错误。
fn endpoint_owner_inspection_failed(host: SkinHostKind) -> AppError {
    if host == SkinHostKind::WorkBuddy {
        return super::workbuddy_owner_inspection_failed();
    }
    AppError::new(
        "skin.codex_cdp_owner_inspection_failed",
        format!(
            "无法验证 {} 调试端口所属进程，未应用皮肤。",
            host.display_name()
        ),
    )
}

/// 列出确实绑定在该回环端口（IPv4/IPv6 皆可）上的全部监听者 PID；
/// 出现非回环地址（如通配 `*`）一律保守拒绝返回 `None`。
fn loopback_listener_owners(output: &str, port: u16) -> Option<BTreeSet<u32>> {
    let ipv4 = format!("127.0.0.1:{port}");
    let ipv6 = format!("[::1]:{port}");
    let mut current_pid = None;
    let mut owners = BTreeSet::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        match line.as_bytes()[0] {
            b'p' => current_pid = line[1..].parse::<u32>().ok(),
            b'n' => {
                let address = &line[1..];
                if address != ipv4 && address != ipv6 {
                    return None;
                }
                owners.insert(current_pid?);
            }
            _ => {}
        }
    }
    (!owners.is_empty()).then_some(owners)
}

/// 解析 `ps` 的 PID/PPID 快照；重复 PID 或畸形行使整份快照失效。
fn parse_process_parents(output: &str) -> Option<HashMap<u32, u32>> {
    let mut parents = HashMap::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let pid = fields.next()?.parse::<u32>().ok()?;
        let parent_pid = fields.next()?.parse::<u32>().ok()?;
        if fields.next().is_some() || parents.insert(pid, parent_pid).is_some() {
            return None;
        }
    }
    (!parents.is_empty()).then_some(parents)
}

/// 共同监听的所有进程都必须属于同一个已验证宿主根。
fn listener_owners_belong_to_root(
    owners: &BTreeSet<u32>,
    root_pid: u32,
    parents: &HashMap<u32, u32>,
) -> bool {
    !owners.is_empty()
        && owners
            .iter()
            .all(|&owner_pid| process_descends_from(owner_pid, root_pid, parents))
}

/// 沿不可循环的父进程链确认 listener owner 归属于指定已验证根进程。
fn process_descends_from(owner_pid: u32, root_pid: u32, parents: &HashMap<u32, u32>) -> bool {
    if !parents.contains_key(&owner_pid) || !parents.contains_key(&root_pid) {
        return false;
    }
    let mut current = owner_pid;
    for _ in 0..parents.len() {
        if current == root_pid {
            return true;
        }
        if current == 0 {
            return false;
        }
        let Some(parent) = parents.get(&current) else {
            return false;
        };
        current = *parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 唯一 owner 可同时监听 IPv4 与 IPv6 回环地址。
    #[test]
    fn listener_owner_accepts_one_process_on_loopback_only() {
        let output = "p42\nn127.0.0.1:9442\nn[::1]:9442\n";
        assert_eq!(
            loopback_listener_owners(output, 9442),
            Some(BTreeSet::from([42]))
        );
    }

    /// 同一回环端口的多个监听者（如 Codex 自带的协作进程）都保留下来，
    /// 交由后续的根进程后代校验决定是否可信，而不是在这里直接拒绝。
    #[test]
    fn listener_owner_keeps_every_loopback_process_when_several_listen() {
        assert_eq!(
            loopback_listener_owners("p42\nn127.0.0.1:9442\np43\nn[::1]:9442\n", 9442),
            Some(BTreeSet::from([42, 43]))
        );
    }

    /// ChatGPT 主进程和可信子进程共用端口可通过；混入其它树或缺失 PID 必须拒绝。
    #[test]
    fn listener_owners_must_all_descend_from_the_verified_root() {
        let parents =
            parse_process_parents("10 1\n20 10\n30 20\n40 1\n").expect("valid process snapshot");
        assert!(listener_owners_belong_to_root(
            &BTreeSet::from([10, 20, 30]),
            10,
            &parents,
        ));
        assert!(!listener_owners_belong_to_root(
            &BTreeSet::from([10, 40]),
            10,
            &parents,
        ));
        assert!(!listener_owners_belong_to_root(
            &BTreeSet::from([10, 99]),
            10,
            &parents,
        ));
        assert!(!listener_owners_belong_to_root(
            &BTreeSet::new(),
            10,
            &parents,
        ));
    }

    /// 通配地址或缺失 owner 的监听记录必须保守拒绝。
    #[test]
    fn listener_owner_rejects_wildcard_and_orphan_rows() {
        assert_eq!(loopback_listener_owners("p42\nn*:9442\n", 9442), None);
        assert_eq!(loopback_listener_owners("n127.0.0.1:9442\n", 9442), None);
    }

    /// listener owner 可以经过多个中间进程归属于已验证 WorkBuddy 根。
    #[test]
    fn process_owner_must_descend_from_verified_root() {
        let parents = parse_process_parents("10 1\n20 10\n30 20\n").expect("valid snapshot");
        assert!(process_descends_from(30, 10, &parents));
        assert!(process_descends_from(10, 10, &parents));
        assert!(!process_descends_from(30, 11, &parents));
        assert!(!process_descends_from(99, 99, &parents));
    }

    /// 缺失父进程与循环链不能伪装成已验证根的后代。
    #[test]
    fn process_owner_rejects_missing_and_cyclic_ancestry() {
        let missing = parse_process_parents("20 10\n30 20\n").expect("valid snapshot");
        assert!(!process_descends_from(30, 9, &missing));
        let cyclic = parse_process_parents("20 30\n30 20\n").expect("valid snapshot");
        assert!(!process_descends_from(30, 10, &cyclic));
    }

    /// 畸形或重复 PID 会使整个进程快照失效，避免部分解析后误接受。
    #[test]
    fn process_parent_parser_rejects_malformed_or_duplicate_rows() {
        assert!(parse_process_parents("10 1 extra\n").is_none());
        assert!(parse_process_parents("10 1\n10 2\n").is_none());
        assert!(parse_process_parents("not-a-pid 1\n").is_none());
    }

    /// 输出保留上限必须同时报告截断，不能让 `ps` 的部分快照进入宿主身份判断。
    #[test]
    fn limited_reader_marks_truncated_process_snapshots() {
        crate::skins::test_block_on(async {
            let exact = vec![b'x'; COMMAND_OUTPUT_LIMIT];
            let complete = read_limited(exact.as_slice())
                .await
                .expect("内存输入应可读取");
            assert_eq!(complete.stdout.len(), COMMAND_OUTPUT_LIMIT);
            assert!(!complete.truncated);

            let oversized = vec![b'x'; COMMAND_OUTPUT_LIMIT + 1];
            let truncated = read_limited(oversized.as_slice())
                .await
                .expect("内存输入应可读取");
            assert_eq!(truncated.stdout.len(), COMMAND_OUTPUT_LIMIT);
            assert!(truncated.truncated);
        });
    }

    /// Codex 与 WorkBuddy 都必须把截断 `ps` 视为权威进程检查失败，而不是未运行。
    #[test]
    fn truncated_process_snapshot_fails_closed_for_each_host() {
        assert_eq!(
            ensure_complete_process_snapshot(SkinHostKind::Codex, true)
                .expect_err("Codex 截断快照必须拒绝")
                .code,
            "skin.codex_process_inspection_failed"
        );
        assert_eq!(
            ensure_complete_process_snapshot(SkinHostKind::WorkBuddy, true)
                .expect_err("WorkBuddy 截断快照必须拒绝")
                .code,
            "skin.workbuddy_process_inspection_failed"
        );
        assert!(ensure_complete_process_snapshot(SkinHostKind::Codex, false).is_ok());
    }

    /// Codex 与 WorkBuddy 的 owner 读取失败必须保留可分辨的稳定错误合同。
    #[test]
    fn endpoint_owner_failure_is_host_specific() {
        assert_eq!(
            endpoint_owner_inspection_failed(SkinHostKind::Codex).code,
            "skin.codex_cdp_owner_inspection_failed"
        );
        assert_eq!(
            endpoint_owner_inspection_failed(SkinHostKind::WorkBuddy).code,
            "skin.workbuddy_cdp_owner_inspection_failed"
        );
    }

    /// 退出观察必须有固定期限，并能区分仍存活与已不存在的 PID。
    #[test]
    fn process_exit_wait_is_bounded_and_observable() {
        crate::skins::test_block_on(async {
            let current_pid = std::process::id();
            let current_executable = std::env::current_exe().expect("测试进程路径应可读取");
            assert!(process_matches_executable(current_pid, &current_executable));
            assert!(
                !process_exits_within(current_pid, &current_executable, Duration::from_millis(1))
                    .await
            );
            assert!(
                process_exits_within(u32::MAX, &current_executable, Duration::from_millis(1)).await
            );
        });
    }

    /// 强制关闭在返回成功前必须等待每个 PID，并重新读取可信宿主进程集。
    #[test]
    fn force_close_contract_waits_and_rechecks_processes() {
        let source = include_str!("macos_process.rs");
        let start = source
            .find("pub(crate) async fn force_close_host")
            .expect("必须保留强制关闭入口");
        let body = &source[start..];
        let end = body
            .find("\n}\n\n/// 验证指定端口")
            .expect("必须保留强制关闭函数边界");
        let body = &body[..end];
        assert!(body.contains("process_exits_within"));
        assert!(body.matches("command_lines_for_verified").count() >= 2);
        assert!(body.matches("verify_host_executable").count() >= 2);
        assert!(body.contains("close_deadline.saturating_duration_since"));
        assert!(body.contains("force_close_timeout_error"));
    }

    /// 无界输出命令必须在截止时间后被终止回收，并返回可区分的超时。
    #[test]
    fn bounded_command_times_out_and_reaps_child() {
        crate::skins::test_block_on(async {
            let command = Command::new("/usr/bin/yes");
            let error = run_bounded_command(command, Duration::from_millis(30))
                .await
                .expect_err("无界命令必须超时");
            assert_eq!(error, BoundedCommandError::TimedOut);
        });
    }

    /// 非零退出必须保留状态，供调用层映射为可观察失败而非伪装成功。
    #[test]
    fn bounded_command_preserves_failure_status() {
        crate::skins::test_block_on(async {
            let output = run_bounded_command(Command::new("/usr/bin/false"), COMMAND_TIMEOUT)
                .await
                .expect("系统命令应可启动并回收");
            assert!(!output.status.success());
        });
    }

    /// 进程 owner 容量耗尽应映射为稳定 I/O 故障，不得误报 spawn 或超时。
    #[test]
    fn process_capacity_error_maps_to_bounded_io_failure() {
        assert_eq!(
            BoundedCommandError::from(OwnedProcessError::CapacityExceeded),
            BoundedCommandError::Io
        );
    }

    /// macOS 宿主发现与命令执行不得回退到 PATH、mdfind 或 plist 自报信息。
    #[test]
    fn host_process_source_uses_only_fixed_system_boundaries() {
        let source = include_str!("macos_process.rs");
        let platform = include_str!("platform/mod.rs");
        assert!(!source.contains("Command::new(\"ps\")"));
        assert!(!source.contains("Command::new(\"lsof\")"));
        assert!(!source.contains("Command::new(\"open\")"));
        assert!(!platform.contains("/usr/bin/mdfind"));
        assert!(!platform.contains("/usr/bin/plutil"));
    }
}
