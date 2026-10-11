//! Hook 配置生命周期的定向回归测试。

use crate::monitor::hook_test_support::HookCommandText as _;
use super::*;
use tempfile::tempdir;

/// 为指定工具生成、校验并原子写入本机或 WSL 配置。
fn write_hook_config(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
) -> Result<HookConfigWriteResult, HookError> {
    write_hook_config_with_cancellation(settings, tool, relay_executable, home_directory, None)
}

/// 非阻塞检查常驻 Hook 锁当前是否已由 worker 释放。
fn hook_config_lock_is_available(directory: &Path) -> bool {
    let path = directory.join(".lokimetis-hook-config.lock");
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
    else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            true
        }
        Err(std::fs::TryLockError::WouldBlock | std::fs::TryLockError::Error(_)) => false,
    }
}

/// 构造把指定工具隔离到临时目录的监控设置。
fn settings_for(tool: AiTool, directory: &Path) -> MonitorSettings {
    let mut settings = MonitorSettings::default();
    settings.enabled_ai_tools = vec![tool];
    settings
        .hook_directories
        .set(tool, directory.to_string_lossy().into_owned());
    settings
}

#[test]
/// 已有管理标识不能掩盖缺失事件或移动后的旧可执行路径。
fn managed_config_repairs_stale_executable_and_missing_event() {
    let root = tempdir().expect("temp");
    let settings = settings_for(AiTool::Codex, &root.path().join("codex"));
    let first = write_hook_config(
        &settings,
        AiTool::Codex,
        Path::new("/old/LokiMetris"),
        root.path(),
    )
    .expect("first write");
    assert!(first.config_changed);
    let config_path = PathBuf::from(&first.filename);
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).expect("read first"))
            .expect("valid JSON");
    value["hooks"]
        .as_object_mut()
        .expect("hooks object")
        .remove("Stop");
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&value).expect("serialize stale config"),
    )
    .expect("write stale config");

    let repaired = write_hook_config(
        &settings,
        AiTool::Codex,
        Path::new("/new/LokiMetris"),
        root.path(),
    )
    .expect("repair");
    assert!(repaired.config_changed);
    let repaired_content = std::fs::read_to_string(config_path).expect("read repaired");
    assert!(repaired_content.hook_command_text().contains("/new/LokiMetris"));
    assert!(!repaired_content.hook_command_text().contains("/old/LokiMetris"));
    let repaired_value: serde_json::Value =
        serde_json::from_str(&repaired_content).expect("repaired JSON");
    assert!(repaired_value["hooks"].get("Stop").is_some());
}

#[test]
/// 自动补写只处理启用项，并保留已取消选择工具的现有文件。
fn automatic_repair_only_touches_enabled_tools_and_never_deletes_disabled_config() {
    let root = tempdir().expect("temp");
    let codex_dir = root.path().join("codex");
    let claude_dir = root.path().join("claude");
    let mut settings = settings_for(AiTool::Codex, &codex_dir);
    settings.hook_directories.set(
        AiTool::ClaudeCode,
        claude_dir.to_string_lossy().into_owned(),
    );
    std::fs::create_dir_all(&claude_dir).expect("claude dir");
    let claude_path = claude_dir.join(hook_config_filename(AiTool::ClaudeCode));
    std::fs::write(&claude_path, "user-owned-disabled-config").expect("claude config");

    repair_enabled_hook_configs(&settings, Path::new("/opt/LokiMetris"), root.path());

    assert!(codex_dir.join(hook_config_filename(AiTool::Codex)).exists());
    assert_eq!(
        std::fs::read_to_string(claude_path).expect("disabled config"),
        "user-owned-disabled-config"
    );
}

#[test]
/// 自动补写边界不能信任内存设置直传值，隐藏协议即使被塞入集合也不得创建配置。
fn automatic_repair_ignores_direct_hidden_tool_input() {
    let root = tempdir().expect("temp");
    let open_code_dir = root.path().join("opencode");
    let settings = settings_for(AiTool::OpenCode, &open_code_dir);

    repair_enabled_hook_configs(&settings, Path::new("/opt/LokiMetris"), root.path());

    assert!(!open_code_dir.exists());
}

#[test]
/// 显式空选择不会产生写入，也不会清理此前配置。
fn empty_enabled_set_performs_no_write_or_delete() {
    let root = tempdir().expect("temp");
    let codex_dir = root.path().join("codex");
    let mut settings = settings_for(AiTool::Codex, &codex_dir);
    std::fs::create_dir_all(&codex_dir).expect("codex dir");
    let config_path = codex_dir.join(hook_config_filename(AiTool::Codex));
    std::fs::write(&config_path, "leave-me-alone").expect("existing config");
    settings.enabled_ai_tools.clear();

    repair_enabled_hook_configs(&settings, Path::new("/opt/LokiMetris"), root.path());

    assert_eq!(
        std::fs::read_to_string(config_path).expect("preserved config"),
        "leave-me-alone"
    );
}

#[test]
/// 相同 relay 与事件集合的重复写入保持幂等。
fn repeated_write_is_idempotent() {
    let root = tempdir().expect("temp");
    let settings = settings_for(AiTool::Codex, root.path());
    let executable = Path::new("/opt/LokiMetris/loki_metris_gui");
    assert!(
        write_hook_config(&settings, AiTool::Codex, executable, root.path())
            .expect("first")
            .config_changed
    );
    assert!(
        !write_hook_config(&settings, AiTool::Codex, executable, root.path())
            .expect("second")
            .config_changed
    );
}

#[test]
/// 独立插件的主文件和协议声明的辅助文件必须作为完整集合写入。
fn standalone_tools_write_every_declared_auxiliary_config() {
    for tool in [AiTool::Hermes, AiTool::OpenClaw] {
        let root = tempdir().expect("temp");
        let settings = settings_for(tool, root.path());
        let auxiliary = generate_hook_auxiliary_configs(tool);
        assert!(!auxiliary.is_empty(), "{tool:?} auxiliary contract");

        let result = write_hook_config(&settings, tool, Path::new("/opt/LokiMetris"), root.path())
            .expect("write standalone config set");

        assert!(result.config_changed);
        assert!(root.path().join(hook_config_filename(tool)).exists());
        for preview in auxiliary {
            assert!(
                root.path().join(preview.filename).exists(),
                "missing auxiliary file for {tool:?}"
            );
        }
    }
}

#[test]
/// 自动补写线程能够处理请求，并由应用生命周期显式回收。
fn owned_worker_processes_enabled_request_and_shuts_down() {
    let root = tempdir().expect("temp");
    let config_path = root.path().join(hook_config_filename(AiTool::Codex));
    let settings = settings_for(AiTool::Codex, root.path());
    let writer = HookConfigWriter::start(PathBuf::from("/opt/LokiMetris"), root.path().to_owned());

    writer.request_enabled(settings);
    for _ in 0..100 {
        if config_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    writer.shutdown();

    assert!(config_path.exists());
    assert!(
        std::fs::read_to_string(config_path)
            .expect("worker config")
            .hook_command_text().contains("/opt/LokiMetris")
    );
}

#[test]
/// 禁用占位不得创建 worker，也不得响应后续设置快照，供性能证据进程零 Hook 写入复用。
fn disabled_writer_ignores_enabled_request_without_creating_config() {
    let root = tempdir().expect("temp");
    let config_path = root.path().join(hook_config_filename(AiTool::Codex));
    let writer = HookConfigWriter::disabled();

    assert!(writer.shared.is_none());
    assert!(writer.worker.lock().expect("worker state").is_none());
    writer.request_enabled(settings_for(AiTool::Codex, root.path()));
    writer.shutdown();

    assert!(!config_path.exists());
}

#[test]
/// 显式写入由具名生命周期 worker 执行，并通过异步通道返回真实结果。
fn owned_worker_executes_explicit_write_and_returns_result() {
    let root = tempdir().expect("temp");
    let config_directory = root.path().join("app-config");
    let hook_directory = root.path().join("codex");
    let settings = settings_for(AiTool::Codex, &hook_directory);
    super::super::settings::save_monitor_settings(&config_directory, &settings)
        .expect("save monitor settings");
    let writer = HookConfigWriter::start(PathBuf::from("/opt/LokiMetris"), root.path().to_owned());

    let result = block_on(writer.write_config(config_directory, AiTool::Codex))
        .expect("explicit worker write");
    writer.shutdown();

    assert!(result.config_changed);
    assert_eq!(
        PathBuf::from(result.filename),
        hook_directory.join("hooks.json")
    );
    assert!(
        std::fs::read_to_string(hook_directory.join("hooks.json"))
            .expect("explicit config")
            .hook_command_text().contains("/opt/LokiMetris")
    );
}

#[test]
/// 显式等待超时只取消受管请求，worker 句柄仍由 owner 持有。
fn explicit_write_timeout_cancels_without_detaching_worker() {
    let root = tempdir().expect("temp");
    let config_directory = root.path().join("app-config");
    let hook_directory = root.path().join("codex");
    let settings = settings_for(AiTool::Codex, &hook_directory);
    super::super::settings::save_monitor_settings(&config_directory, &settings)
        .expect("save monitor settings");
    let writer = Arc::new(HookConfigWriter::start(
        PathBuf::from("/opt/LokiMetris"),
        root.path().to_owned(),
    ));
    let write_guard = HOOK_CONFIG_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("hook write lock");

    let request_writer = Arc::clone(&writer);
    let request = std::thread::spawn(move || {
        let started = Instant::now();
        let result = block_on(request_writer.write_config_with_deadlines(
            config_directory,
            AiTool::Codex,
            Duration::from_millis(20),
            Duration::from_millis(100),
        ));
        (started.elapsed(), result)
    });

    let (elapsed, result) = request.join().expect("request thread");
    let error = result.expect_err("cancelled explicit request must fail closed");
    assert!(elapsed < Duration::from_millis(500));
    assert_eq!(error.code, "error.hooks.writeFailed");
    assert_eq!(
        error.params.get("detail").map(String::as_str),
        Some(HOOK_WRITE_CANCELLED_DETAIL)
    );
    assert!(writer.worker.lock().expect("worker owner").is_some());
    assert!(!hook_directory.join("hooks.json").exists());

    drop(write_guard);
    writer.shutdown();
    std::thread::sleep(Duration::from_millis(50));
    assert!(!hook_directory.join("hooks.json").exists());
    assert!(writer.worker.lock().expect("joined worker").is_none());
}

#[test]
/// worker 不让出时，IPC 仍须在同一最终绝对截止时间返回并保留取消。
fn explicit_response_wait_has_a_hard_cancellation_deadline() {
    let cancellation = Arc::new(AtomicBool::new(false));
    let (response, mut receiver) = oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = blocked.recv();
        drop(response);
    });
    let started = Instant::now();
    let response_deadline = tokio::time::Instant::now() + Duration::from_millis(20);
    let cancellation_deadline = response_deadline + Duration::from_millis(30);

    let error = block_on(await_explicit_hook_write_response(
        &mut receiver,
        Arc::clone(&cancellation),
        response_deadline,
        cancellation_deadline,
    ))
    .expect_err("blocked response must reach the hard deadline");

    assert!(started.elapsed() >= Duration::from_millis(45));
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(cancellation.load(Ordering::Acquire));
    assert!(
        !worker.is_finished(),
        "worker ownership must remain observable"
    );
    assert_eq!(error.code, "error.hooks.writeFailed");
    assert_eq!(
        error.params.get("detail").map(String::as_str),
        Some(HOOK_WRITE_CANCEL_DEADLINE_DETAIL)
    );
    drop(receiver);
    release.send(()).expect("release blocked worker");
    worker.join().expect("owned worker must be reaped");
}

#[test]
/// 入队前必须回收已取消项、答复稳定错误，并把释放的容量交给新请求。
fn enqueue_reclaims_cancelled_requests_before_capacity_check() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let (cancelled_response, cancelled_receiver) = oneshot::channel();
    let mut state = HookWriterState::default();
    state.explicit_requests.push_back(ExplicitHookWriteRequest {
        config_directory: PathBuf::from("/cancelled"),
        tool: AiTool::Codex,
        cancellation: cancelled,
        response: cancelled_response,
    });
    let (replacement_response, _replacement_receiver) = oneshot::channel();

    enqueue_explicit_hook_write(
        &mut state,
        ExplicitHookWriteRequest {
            config_directory: PathBuf::from("/replacement"),
            tool: AiTool::Codex,
            cancellation: Arc::new(AtomicBool::new(false)),
            response: replacement_response,
        },
    )
    .expect("cancelled slot must be reusable");

    assert_eq!(state.explicit_requests.len(), 1);
    let error = block_on(cancelled_receiver)
        .expect("cancelled response")
        .expect_err("cancelled queued request must fail");
    assert_eq!(error.code, "error.hooks.writeFailed");
    assert_eq!(
        error.params.get("detail").map(String::as_str),
        Some(HOOK_WRITE_CANCELLED_DETAIL)
    );
}

#[test]
/// 显式请求队列达到硬上限后必须立即拒绝新请求。
fn explicit_request_queue_applies_bounded_backpressure() {
    let mut state = HookWriterState::default();
    for index in 0..MAX_EXPLICIT_HOOK_WRITE_QUEUE {
        let (response, _receiver) = oneshot::channel();
        enqueue_explicit_hook_write(
            &mut state,
            ExplicitHookWriteRequest {
                config_directory: PathBuf::from(format!("/queued/{index}")),
                tool: AiTool::Codex,
                cancellation: Arc::new(AtomicBool::new(false)),
                response,
            },
        )
        .expect("queue slot");
    }
    let (overflow_response, _overflow_receiver) = oneshot::channel();
    let error = enqueue_explicit_hook_write(
        &mut state,
        ExplicitHookWriteRequest {
            config_directory: PathBuf::from("/overflow"),
            tool: AiTool::Codex,
            cancellation: Arc::new(AtomicBool::new(false)),
            response: overflow_response,
        },
    )
    .expect_err("full queue must reject new work");

    assert_eq!(state.explicit_requests.len(), MAX_EXPLICIT_HOOK_WRITE_QUEUE);
    assert_eq!(error.code, "error.hooks.writeFailed");
    assert_eq!(
        error.params.get("detail").map(String::as_str),
        Some(HOOK_WRITE_QUEUE_FULL_DETAIL)
    );
}

#[test]
/// IPC future 被丢弃时 cancellation guard 必须立刻通知仍受 owner 管理的请求。
fn explicit_write_guard_propagates_future_cancellation() {
    let cancellation = Arc::new(AtomicBool::new(false));
    let guard = HookWriteCancellationGuard::new(Arc::clone(&cancellation));
    drop(guard);
    assert!(cancellation.load(Ordering::Acquire));
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - tests_shutdown.rs：应用退出必须同时取消在途与排队显式写入，并立即答复尚未执行的调用。；不可中断的本机 I/O worker 超过期限时不得继续阻塞应用退出回调。；未显式 shutdown 的空闲 writer 在 Drop 后也必须让线程终态并释放共享状态。
include!("tests_shutdown.rs");
