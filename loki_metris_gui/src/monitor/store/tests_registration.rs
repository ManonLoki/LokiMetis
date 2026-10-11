// 通过真实配置文件和生命周期 worker 验证首次注册权限，不访问用户的默认 Agent 根。

/// 隔离夹具预先注册旧可执行路径，后台后续只能维护这个已拥有根。
fn seed_owned_config(tool: AiTool, directory: &Path, executable: &str) {
    let path = directory.join(hook_config_filename(tool));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let generated = generate_hook_config(tool, Path::new(executable)).unwrap();
    std::fs::write(path, generated.content).unwrap();
}

/// 等待真实 worker 修复 sentinel；超时给出文件内容而不是通过不存在判断伪造执行。
fn wait_for_repaired_hook(path: &Path, executable: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if std::fs::read_to_string(path).is_ok_and(|text| text.hook_command_text().contains(executable)) { return; }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("worker 未修复 sentinel: {:?}", std::fs::read_to_string(path));
}

/// 已注册根的校正结束后才注入下一次外部修改；有界等待不创建未注册根。
fn wait_for_hook_unlock(directory: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if hook_config_lock_is_available(directory) { return; }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("sentinel 校正未释放锁");
}

/// 已注册 Claude sentinel 证明首次请求和周期维护均执行，未注册 Codex 根始终零写入。
#[test]
fn automatic_worker_never_registers_missing_or_foreign_primary() {
    for foreign in [None, Some(r#"{"hooks":{"SessionStart":[{"command":"user-hook"}]},"owner":"external"}"#)] {
        let root = tempdir().unwrap();
        let codex = root.path().join("codex");
        let claude = root.path().join("claude");
        let codex_path = codex.join(hook_config_filename(AiTool::Codex));
        if let Some(text) = foreign { std::fs::create_dir_all(&codex).unwrap(); std::fs::write(&codex_path, text).unwrap(); }
        seed_owned_config(AiTool::ClaudeCode, &claude, "/old/LokiMetris");
        let mut settings = settings_for(AiTool::Codex, &codex);
        settings.enabled_ai_tools.push(AiTool::ClaudeCode);
        settings.hook_directories.set(AiTool::ClaudeCode, claude.to_string_lossy().into_owned());
        let writer = HookConfigWriter::start_with_interval(PathBuf::from("/new/LokiMetris"), root.path().to_owned(), Duration::from_millis(50));
        writer.request_enabled(settings);
        let sentinel = claude.join(hook_config_filename(AiTool::ClaudeCode));
        wait_for_repaired_hook(&sentinel, "/new/LokiMetris");
        // 锁释放后重新种入旧 owned 内容，第二次修复只能来自真实周期。
        wait_for_hook_unlock(&claude);
        seed_owned_config(AiTool::ClaudeCode, &claude, "/older/LokiMetris");
        wait_for_repaired_hook(&sentinel, "/new/LokiMetris");
        writer.shutdown();
        assert!(!codex.join(".lokimetis-hook-config.lock").exists());
        match foreign {
            Some(text) => assert_eq!(std::fs::read_to_string(&codex_path).unwrap(), text),
            None => assert!(!codex.exists()),
        }
    }
}

/// WorkBuddy 两个默认根独立授权，国内版已有注册不能授权国际版首次创建。
#[test]
fn automatic_workbuddy_repair_does_not_register_the_other_root() {
    let root = tempdir().unwrap();
    let mut settings = MonitorSettings::default();
    settings.enabled_ai_tools = vec![AiTool::WorkBuddy];
    let roots = hook_config_target_directories(AiTool::WorkBuddy, &settings.hook_directories, root.path());
    assert_eq!(roots.len(), 2);
    seed_owned_config(AiTool::WorkBuddy, &roots[0], "/old/LokiMetris");
    repair_enabled_hook_configs(&settings, Path::new("/new/LokiMetris"), root.path());
    assert!(std::fs::read_to_string(roots[0].join(hook_config_filename(AiTool::WorkBuddy))).unwrap().hook_command_text().contains("/new/LokiMetris"));
    assert!(!roots[1].exists());
}

/// 切换到未注册新目录时旧目录停止维护，新目录也不会被隐式注册。
#[test]
fn changing_to_unregistered_directory_requires_explicit_write() {
    let root = tempdir().unwrap();
    let old = root.path().join("old");
    let new = root.path().join("new");
    seed_owned_config(AiTool::Codex, &old, "/old/LokiMetris");
    let writer = HookConfigWriter::start_with_interval(PathBuf::from("/new/LokiMetris"), root.path().to_owned(), Duration::from_millis(30));
    writer.request_enabled(settings_for(AiTool::Codex, &old));
    let old_path = old.join(hook_config_filename(AiTool::Codex));
    wait_for_repaired_hook(&old_path, "/new/LokiMetris");
    writer.request_enabled(settings_for(AiTool::Codex, &new));
    std::thread::sleep(Duration::from_millis(120));
    std::fs::write(&old_path, "external after switching").unwrap();
    std::thread::sleep(Duration::from_millis(120));
    writer.shutdown();
    assert!(!new.exists());
    assert_eq!(std::fs::read_to_string(old_path).unwrap(), "external after switching");
}

/// 删除整个已注册主文件表示撤销，多个真实周期后仍保持不存在。
#[test]
fn automatic_worker_preserves_deleted_primary() {
    let root = tempdir().unwrap();
    seed_owned_config(AiTool::Codex, root.path(), "/old/LokiMetris");
    let claude = root.path().join("claude");
    seed_owned_config(AiTool::ClaudeCode, &claude, "/old/LokiMetris");
    let target = root.path().join(hook_config_filename(AiTool::Codex));
    let sentinel = claude.join(hook_config_filename(AiTool::ClaudeCode));
    let mut settings = settings_for(AiTool::Codex, root.path());
    settings.enabled_ai_tools.push(AiTool::ClaudeCode);
    settings.hook_directories.set(AiTool::ClaudeCode, claude.to_string_lossy().into_owned());
    let writer = HookConfigWriter::start_with_interval(PathBuf::from("/new/LokiMetris"), root.path().to_owned(), Duration::from_millis(30));
    writer.request_enabled(settings);
    wait_for_repaired_hook(&target, "/new/LokiMetris");
    wait_for_repaired_hook(&sentinel, "/new/LokiMetris");
    std::fs::remove_file(&target).unwrap();
    for _ in 0..2 {
        wait_for_hook_unlock(&claude);
        seed_owned_config(AiTool::ClaudeCode, &claude, "/older/LokiMetris");
        wait_for_repaired_hook(&sentinel, "/new/LokiMetris");
    }
    writer.shutdown();
    assert!(!target.exists());
}
