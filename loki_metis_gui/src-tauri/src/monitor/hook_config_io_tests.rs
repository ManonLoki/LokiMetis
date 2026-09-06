//! Hook 配置跨进程锁和外部 writer 校正的隔离回归测试。

use std::sync::mpsc;

use super::*;
use loki_metis_core::{generate_hook_config, hook_config_filename};
use tempfile::tempdir;

/// 构造指定工具写入共享 hooks.json 的生成文件集合。
fn generated_set(tool: AiTool, directory: &Path) -> Vec<(PathBuf, HookConfigPreview)> {
    vec![(
        directory.join(hook_config_filename(tool)),
        generate_hook_config(tool, Path::new("/opt/LokiMetis")).expect("generate"),
    )]
}

#[test]
/// 两个采用同一锁的 writer 会串行合并，共享文件最终保留双方受管条目。
fn two_writers_coexist_without_losing_managed_entries() {
    let root = tempdir().expect("temp");
    let first_root = root.path().to_owned();
    let second_root = root.path().to_owned();
    let (start_tx, start_rx) = mpsc::channel();
    let first = thread::spawn(move || {
        start_tx.send(()).expect("start");
        reconcile_local_config_set(
            &first_root,
            AiTool::Codex,
            generated_set(AiTool::Codex, &first_root),
        )
        .expect("first writer");
    });
    start_rx.recv().expect("first started");
    let second = thread::spawn(move || {
        reconcile_local_config_set(
            &second_root,
            AiTool::Cursor,
            generated_set(AiTool::Cursor, &second_root),
        )
        .expect("second writer");
    });
    first.join().expect("first thread");
    second.join().expect("second thread");

    let content = fs::read_to_string(root.path().join("hooks.json")).expect("shared config");
    assert!(content.contains("LokiMetis:tool=codex"));
    assert!(content.contains("LokiMetis:tool=cursor"));
}

#[test]
/// 外部 writer 在首次提交后覆盖文件时，有限写后重读会合并其字段并恢复 Hook。
fn reconciliation_preserves_external_write_after_first_commit() {
    let root = tempdir().expect("temp");
    let target = root.path().join("hooks.json");
    let target_for_observer = target.clone();
    let mut injected = false;
    let changed = reconcile_local_config_set_with_observer(
        root.path(),
        AiTool::Codex,
        generated_set(AiTool::Codex, root.path()),
        move |_, written| {
            if !injected && !written.is_empty() {
                super::super::atomic_file::write_monitor_file_atomically(
                    &target_for_observer,
                    br#"{"externalWriter":true}"#,
                    "error.hooks.writeFailed",
                )
                .expect("external replacement");
                injected = true;
            }
        },
    )
    .expect("reconciled write");

    assert!(changed);
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(target).expect("final config"))
            .expect("final JSON");
    assert_eq!(value["externalWriter"], true);
    assert!(value["hooks"].get("SessionStart").is_some());
}

#[test]
/// 操作系统锁在 guard Drop 后自动释放；常驻文件无需不安全的 stale 删除协议。
fn operating_system_lock_is_exclusive_and_recovers_on_drop() {
    let root = tempdir().expect("temp");
    let lock_path = root.path().join(LOCK_FILENAME);
    let first = HookConfigFileLock::acquire(root.path()).expect("first lock");
    let second = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("second handle");
    assert!(matches!(second.try_lock(), Err(TryLockError::WouldBlock)));

    drop(first);
    second.try_lock().expect("lock after owner drop");
    second.unlock().expect("explicit test unlock");
    assert!(lock_path.exists());
}
