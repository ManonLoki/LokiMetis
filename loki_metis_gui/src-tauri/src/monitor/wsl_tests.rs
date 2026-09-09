//! WSL 路径、取消、私有临时文件和 stdio owner 的隔离回归。

use super::*;

/// 两种 WSL UNC 前缀都能解析并安全拼接嵌套辅助文件。
#[test]
fn recognizes_both_wsl_unc_prefixes_and_nested_files() {
    let modern = WslDirectory::parse(r"\\wsl.localhost\Ubuntu-24.04\home\user\.openclaw")
        .expect("modern WSL UNC");
    assert_eq!(
        modern.join("extensions/lokimetis/package.json").display(),
        "/home/user/.openclaw/extensions/lokimetis/package.json"
    );
    let legacy = WslDirectory::parse(r"\\wsl$\Arch\home\user\.codex").expect("legacy WSL UNC");
    assert_eq!(
        legacy.join("hooks.json").display(),
        "/home/user/.codex/hooks.json"
    );
}

/// 不完整、穿越或普通网络 UNC 路径不会被误判为 WSL 目录。
#[test]
fn rejects_incomplete_or_traversing_wsl_unc_paths() {
    assert!(WslDirectory::parse(r"\\wsl.localhost\").is_none());
    assert!(WslDirectory::parse(r"\\wsl$\Ubuntu\home\..\.codex").is_none());
    assert!(WslDirectory::parse(r"\\server\share\.codex").is_none());
}

/// Windows relay 路径会在进入 wslpath 前统一为正斜杠。
#[test]
fn normalizes_windows_executable_for_wslpath() {
    assert_eq!(
        wslpath_input(r"C:\Program Files\LokiMetis\loki_metis_gui.exe"),
        "C:/Program Files/LokiMetis/loki_metis_gui.exe"
    );
    assert!(wsl_test_reports_missing(Some(1)));
    assert!(!wsl_test_reports_missing(Some(2)));
}

/// WSL 临时名不可预测且每次不同，不再暴露 PID 与递增序号。
#[test]
fn atomic_write_temporary_paths_are_unpredictable_and_unique() {
    let target = "/home/user/.grok/hooks/lokimetis.json";
    let first = unique_temporary_path(target);
    let second = unique_temporary_path(target);
    let suffix = first
        .strip_prefix(&format!("{target}.lokimetis."))
        .and_then(|value| value.strip_suffix(".tmp"))
        .expect("UUID temporary suffix");

    assert_eq!(suffix.len(), 32);
    assert!(
        suffix
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
    assert_ne!(first, second);
}

/// 私有写脚本必须以 0600 创建、拒绝覆盖，并且失败清理使用独立命令入口。
#[test]
fn atomic_write_uses_private_exclusive_creation_and_independent_cleanup() {
    assert!(WSL_PRIVATE_TEMP_WRITE_SCRIPT.contains("umask 077"));
    assert!(WSL_PRIVATE_TEMP_WRITE_SCRIPT.contains("set -C"));
    assert!(WSL_PRIVATE_TEMP_WRITE_SCRIPT.contains("> \"$1\""));
    let source = include_str!("wsl.rs");
    assert!(!source.contains("[\"tee\", &temporary_path]"));
    assert!(source.contains("run_wsl_cleanup"));
    assert!(source.contains("temporary.disarm()"));
}

/// 后台 writer 的取消令牌与整体截止时间必须沿目录传给每个 WSL 文件。
#[test]
fn propagates_hook_writer_cancellation_to_wsl_files() {
    let cancellation = Arc::new(AtomicBool::new(false));
    let directory = WslDirectory::parse(r"\\wsl$\Ubuntu\home\user\.codex")
        .expect("WSL directory")
        .with_cancellation(Some(Arc::clone(&cancellation)));
    let first = directory.join("hooks.json");
    let second = directory.join("extensions/lokimetis/package.json");
    let (_, deadline) = wsl_command_deadlines(&directory.deadline);
    let (_, reused) = wsl_command_deadlines(&second.deadline);

    assert!(Arc::ptr_eq(&directory.cancellation, &cancellation));
    assert!(Arc::ptr_eq(&first.cancellation, &cancellation));
    assert!(Arc::ptr_eq(&first.deadline, &directory.deadline));
    assert!(Arc::ptr_eq(&second.deadline, &directory.deadline));
    assert_eq!(deadline, reused);
    assert_eq!(first.deadline.get(), Some(&deadline));
}

/// 管道初始化早退与 finish 超时都移交 owner，并在没有新命令时自动回收。
#[test]
fn unfinished_stdio_tasks_are_reaped_during_idle() {
    let owner = wsl_io_thread_owner();
    assert!(owner.wait_until(Instant::now() + Duration::from_secs(1)));
    let release = Arc::new(AtomicBool::new(false));
    let spawn_blocked = |label, release: Arc<AtomicBool>| {
        OwnedWslIoTask::spawn(label, move || {
            while !release.load(std::sync::atomic::Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
        })
        .expect("stdio owner thread")
    };

    drop(spawn_blocked("wsl-setup-test", Arc::clone(&release)));
    let timed = spawn_blocked("wsl-timeout-test", Arc::clone(&release));
    assert_eq!(
        timed.finish(Instant::now() + Duration::from_millis(20)),
        Err(WslIoTaskError::TimedOut)
    );
    assert!(owner.retained_count() >= 2);

    release.store(true, std::sync::atomic::Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(1);
    while owner.retained_count() != 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(owner.retained_count(), 0);
    assert!(owner.wait_until(deadline));
}

/// WSL 适配器只能从 Windows System Known Folder 启动绝对路径，不能信任 PATH。
#[test]
fn wsl_process_identity_does_not_trust_path_lookup() {
    let source = include_str!("wsl_process.rs");
    assert!(source.contains("FOLDERID_System"));
    assert!(source.contains("windows_system_wsl_executable"));
    assert!(!source.contains("Command::new(\"wsl.exe\")"));
    assert!(!source.contains("child.wait()"));
    assert!(source.contains("retain_wsl_child(child)"));
    let owner = include_str!("wsl_process_owner.rs");
    assert!(owner.contains("static WSL_RETAINED_CHILDREN"));
    assert!(owner.contains("shutdown_retained_wsl_children_until"));
}
