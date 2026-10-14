// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：应用退出必须同时取消在途与排队显式写入，并立即答复尚未执行的调用。；不可中断的本机 I/O worker 超过期限时不得继续阻塞应用退出回调。；未显式 shutdown 的空闲 writer 在 Drop 后也必须让线程终态并释放共享状态。

#[test]
/// 应用退出必须同时取消在途与排队显式写入，并立即答复尚未执行的调用。
fn shutdown_cancels_active_and_pending_explicit_writes() {
    let active = Arc::new(AtomicBool::new(false));
    let pending = Arc::new(AtomicBool::new(false));
    let (response, receiver) = oneshot::channel();
    let mut state = HookWriterState {
        settings: Some(MonitorSettings::default()),
        repair_requested: true,
        explicit_requests: VecDeque::from([ExplicitHookWriteRequest {
            config_directory: PathBuf::from("/not/read/during-this-test"),
            tool: AiTool::Codex,
            cancellation: Arc::clone(&pending),
            response,
        }]),
        active_explicit_cancellation: Some(Arc::clone(&active)),
        shutting_down: false,
    };

    begin_hook_writer_shutdown(&mut state);

    assert!(state.shutting_down);
    assert!(state.settings.is_none());
    assert!(!state.repair_requested);
    assert!(state.explicit_requests.is_empty());
    assert!(active.load(Ordering::Acquire));
    assert!(pending.load(Ordering::Acquire));
    let result = block_on(receiver).expect("shutdown response");
    assert_eq!(
        result
            .expect_err("pending request must fail on shutdown")
            .code,
        "error.hooks.writeFailed"
    );
}

#[test]
/// 不可中断的本机 I/O worker 超过期限时不得继续阻塞应用退出回调。
fn hook_writer_join_respects_shutdown_deadline() {
    let worker = std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(200));
    });
    let started = std::time::Instant::now();

    let worker = join_worker_until(worker, std::time::Duration::from_millis(20))
        .expect_err("超时后必须返回仍受管的线程句柄");
    assert!(started.elapsed() < std::time::Duration::from_millis(150));
    worker.join().expect("测试 worker 最终必须回收");

    // 源码已拆成同目录分片，契约检查要看到完整文本。
    let source = [include_str!("mod.rs"), include_str!("store_config_writer.rs")].join("\n");
    assert!(source.contains("retained_hook_writer_owner().retain(worker_handle)"));
}

/// 未显式 shutdown 的空闲 writer 在 Drop 后也必须让线程终态并释放共享状态。
#[test]
fn hook_writer_drop_reaps_idle_worker() {
    let root = tempdir().expect("temp");
    let writer = HookConfigWriter::start(PathBuf::from("/opt/LokiMetris"), root.path().to_owned());
    let shared = Arc::downgrade(writer.shared.as_ref().expect("writer shared state"));

    drop(writer);

    let deadline = Instant::now() + Duration::from_secs(1);
    while shared.upgrade().is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(shared.upgrade().is_none());
}

#[test]
/// 外部 writer 完整撤销本应用注册后，跨多个真实维护周期也不得重新加入。
fn owned_worker_preserves_explicit_external_removal() {
    let root = tempdir().expect("temp");
    let config_path = root.path().join(hook_config_filename(AiTool::Codex));
    let mut settings = settings_for(AiTool::Codex, root.path());
    seed_owned_config(AiTool::Codex, root.path(), "/old/LokiMetris");
    let claude = root.path().join("claude");
    seed_owned_config(AiTool::ClaudeCode, &claude, "/old/LokiMetris");
    settings.enabled_ai_tools.push(AiTool::ClaudeCode);
    settings.hook_directories.set(AiTool::ClaudeCode, claude.to_string_lossy().into_owned());
    let sentinel = claude.join(hook_config_filename(AiTool::ClaudeCode));
    let writer = HookConfigWriter::start_with_interval(
        PathBuf::from("/opt/LokiMetris"),
        root.path().to_owned(),
        std::time::Duration::from_millis(200),
    );

    writer.request_enabled(settings);
    wait_for_repaired_hook(&config_path, "/opt/LokiMetris");
    wait_for_repaired_hook(&sentinel, "/opt/LokiMetris");
    wait_for_hook_unlock(root.path());

    let external = r#"{
      "hooks": {
        "SessionStart": [{"hooks": [{"type": "command", "command": "external-final-write"}]}]
      }
    }"#;
    std::fs::write(&config_path, external).expect("external final replacement");
    assert!(
        !std::fs::read_to_string(&config_path)
            .expect("external config")
            .hook_command_text().contains("LokiMetris:tool=codex")
    );

    for _ in 0..2 {
        wait_for_hook_unlock(&claude);
        seed_owned_config(AiTool::ClaudeCode, &claude, "/older/LokiMetris");
        wait_for_repaired_hook(&sentinel, "/opt/LokiMetris");
    }
    writer.shutdown();

    assert_eq!(std::fs::read_to_string(config_path).expect("external removal preserved"), external);
}

#[test]
/// 更新设置快照后只自愈新目录，旧目录即使丢失 marker 也不再被后台改写。
fn owned_worker_replaces_the_previous_directory_snapshot() {
    let root = tempdir().expect("temp");
    let old_directory = root.path().join("old");
    let new_directory = root.path().join("new");
    let old_path = old_directory.join(hook_config_filename(AiTool::Codex));
    let new_path = new_directory.join(hook_config_filename(AiTool::Codex));
    seed_owned_config(AiTool::Codex, &old_directory, "/old/LokiMetris");
    seed_owned_config(AiTool::Codex, &new_directory, "/old/LokiMetris");
    let writer = HookConfigWriter::start_with_interval(
        PathBuf::from("/opt/LokiMetris"),
        root.path().to_owned(),
        std::time::Duration::from_millis(100),
    );

    writer.request_enabled(settings_for(AiTool::Codex, &old_directory));
    wait_for_repaired_hook(&old_path, "/opt/LokiMetris");

    writer.request_enabled(settings_for(AiTool::Codex, &new_directory));
    wait_for_repaired_hook(&new_path, "/opt/LokiMetris");

    let external_only = r#"{"hooks":{"SessionStart":[]},"owner":"external"}"#;
    std::fs::write(&old_path, external_only).expect("replace old config");
    std::thread::sleep(std::time::Duration::from_millis(350));
    writer.shutdown();

    assert_eq!(
        std::fs::read_to_string(old_path).expect("old config remains external"),
        external_only
    );
    assert!(
        std::fs::read_to_string(new_path)
            .expect("new config remains managed")
            .hook_command_text().contains("LokiMetris:tool=codex")
    );
}

/// 新产品写入独立Grok文件，不读取、迁移或清理参考App的旧文件。
#[test]
fn grok_write_preserves_reference_legacy_file() {
    let root = tempdir().expect("temp");
    let settings = settings_for(AiTool::Grok, root.path());
    let legacy_path = root.path().join("hooks/aimonitor.json");
    std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    let legacy = "user-owned and not even valid JSON: LokiMetis:tool=grok";
    std::fs::write(&legacy_path, legacy).unwrap();
    let result = write_hook_config(
        &settings,
        AiTool::Grok,
        Path::new("/opt/LokiMetris"),
        root.path(),
    )
    .unwrap();
    assert!(result.config_changed);
    assert_eq!(std::fs::read_to_string(&legacy_path).unwrap(), legacy);
    assert!(
        !write_hook_config(
            &settings,
            AiTool::Grok,
            Path::new("/opt/LokiMetris"),
            root.path()
        )
        .unwrap()
        .config_changed
    );
    assert_eq!(std::fs::read_to_string(legacy_path).unwrap(), legacy);
}

/// 每项异步回归拥有自己的runtime，不依赖旧框架的全局宿主。
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}
