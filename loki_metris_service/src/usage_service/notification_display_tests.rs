//! 真实独立摘要 worker 与 SQLite 索引回归，文件及配置全部位于临时目录。

use super::*;
use loki_metris_host::local_index::{self, DiscoveryInputs, DiscoveryMethod, LocalIndex};
use loki_metris_core::{AiTool, SourceClientKind};
use std::{path::Path, time::Instant};

/// 有界等待真实后台结果，不通过 sleep 时长推定已完成。
fn wait(service: &UsageService, expected: impl Fn(TodayUsageDisplay) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let value = service.today_display();
        if expected(value) {
            return;
        }
        assert!(Instant::now() < deadline, "摘要未到达目标状态：{value:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 原始记录时间由当前瞬间生成，仅注册测试根；不扫描真实 HOME。
fn indexed_service(directory: &Path, tokens: u64) -> Arc<UsageService> {
    let source = directory.join("source");
    std::fs::create_dir_all(source.join("sessions")).unwrap();
    let now = jiff::Timestamp::now().to_string();
    let meta = serde_json::json!({"timestamp":now,"type":"session_meta","payload":{"id":"today-display-test"}});
    let count = serde_json::json!({"timestamp":now,"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":tokens,"cached_input_tokens":0,"output_tokens":0,"total_tokens":tokens}}}});
    std::fs::write(
        source.join("sessions/rollout-display.jsonl"),
        format!("{meta}\n{count}\n"),
    )
    .unwrap();
    let service = UsageService::in_test_directory(directory.join("data")).unwrap();
    service
        .save_enabled_ai_tools(vec![AiTool::Codex])
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    let discovered = local_index::discover_quick(
        &DiscoveryInputs {
            home_dir: None,
            codex_home: Some(source.clone()),
            registered_roots: vec![],
        },
        &ScanCancellation::new(),
    );
    let id = discovered.roots.first().unwrap().root_id.clone();
    {
        let runtime = service.runtime.lock().unwrap();
        runtime.as_ref().unwrap().block_on(async {
            let data = loki_metris_core::source_client_app_data_dir(
                &service.context.directory,
                SourceClientKind::Codex,
            );
            let mut index =
                LocalIndex::open_in_app_data(&data, SourceClientKind::Codex.parser_version())
                    .await
                    .unwrap();
            index
                .register_confirmed_root_fields(
                    &id,
                    &source,
                    "今日测试根",
                    DiscoveryMethod::Registered,
                )
                .await
                .unwrap();
        });
    }
    let result = service
        .mutate_source(
            SourceClientKind::Codex,
            super::super::SourceOperation::Reindex { root_id: id },
        )
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    assert!(matches!(
        result,
        super::super::SourceOperationResult::Reindexed
    ));
    service
}

/// 通知关闭仍读取真实 All；来源重置当场撤旧值，失败撤值，恢复及退出均可观察。
#[test]
fn today_display_real_worker_handles_reset_failure_recovery_and_close() {
    let root = tempfile::tempdir().unwrap();
    let service = indexed_service(root.path(), 110);
    service.configure_notifications(false, NotificationPermission::Denied);
    service.today_visible(true);
    wait(&service, |v| {
        matches!(
            v,
            TodayUsageDisplay::Value {
                total_tokens: 110,
                ..
            }
        )
    });
    assert!(service.take_notification().is_none());
    let blocked = super::super::block_pet_mutations(&service);
    service
        .context
        .notification_source_epoch
        .fetch_add(1, Ordering::AcqRel);
    service.refresh_today();
    assert_eq!(service.today_display(), TodayUsageDisplay::Pending);
    let deadline = Instant::now() + Duration::from_secs(3);
    while super::super::pet_background_task_count(&service) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(service.today_display(), TodayUsageDisplay::Pending);
    blocked.release();
    wait(&service, |v| {
        matches!(
            v,
            TodayUsageDisplay::Value {
                total_tokens: 110,
                ..
            }
        )
    });
    // 第二次在旧读取已进入真实 store 之后切代，覆盖在途结果的拒收路径。
    let blocked = super::super::block_pet_mutations(&service);
    service.refresh_today();
    let deadline = Instant::now() + Duration::from_secs(3);
    while super::super::pet_background_task_count(&service) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    service
        .context
        .notification_source_epoch
        .fetch_add(1, Ordering::AcqRel);
    assert_eq!(service.today_display(), TodayUsageDisplay::Pending);
    service.refresh_today();
    blocked.release();
    wait(&service, |v| {
        matches!(
            v,
            TodayUsageDisplay::Value {
                total_tokens: 110,
                ..
            }
        )
    });
    // 保持同一 epoch，将真实 SQLite 临时替换为损坏库；旧快照仍在但必须撤值。
    let epoch = service
        .context
        .notification_source_epoch
        .load(Ordering::Acquire);
    let data = loki_metris_core::source_client_app_data_dir(
        &service.context.directory,
        SourceClientKind::Codex,
    );
    let database = {
        let runtime = service.runtime.lock().unwrap();
        runtime.as_ref().unwrap().block_on(async {
            LocalIndex::open_read_only_in_app_data(&data, SourceClientKind::Codex.parser_version())
                .await
                .unwrap()
                .database_path()
                .file_name()
                .unwrap()
                .to_owned()
        })
    };
    let backup = root.path().join("index-backup");
    std::fs::rename(&data, &backup).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join(database), b"not a sqlite database").unwrap();
    service.refresh_today();
    wait(&service, |v| v == TodayUsageDisplay::Unavailable);
    assert_eq!(
        service
            .context
            .notification_source_epoch
            .load(Ordering::Acquire),
        epoch
    );
    assert_eq!(
        service
            .notification_progress()
            .snapshot
            .as_ref()
            .unwrap()
            .total_tokens,
        110
    );
    std::fs::remove_dir_all(&data).unwrap();
    std::fs::rename(&backup, &data).unwrap();
    service.refresh_today();
    wait(&service, |v| {
        matches!(
            v,
            TodayUsageDisplay::Value {
                total_tokens: 110,
                ..
            }
        )
    });
    service
        .save_enabled_ai_tools(vec![])
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    wait(&service, |v| v == TodayUsageDisplay::Unavailable);
    service
        .save_enabled_ai_tools(vec![AiTool::Codex])
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    wait(&service, |v| {
        matches!(
            v,
            TodayUsageDisplay::Value {
                total_tokens: 110,
                ..
            }
        )
    });
    service.finish();
    assert_eq!(service.today_display(), TodayUsageDisplay::Unavailable);
}

/// 未配置的启用来源不隐藏已索引总量，但必须收紧通知所需完整度。
#[test]
fn today_display_unconfigured_enabled_agent_keeps_partial_indexed_total() {
    let root = tempfile::tempdir().unwrap();
    let service = indexed_service(root.path(), 110);
    service
        .save_enabled_ai_tools(vec![AiTool::Codex, AiTool::ClaudeCode])
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    service.configure_notifications(false, NotificationPermission::Denied);
    service.today_visible(true);
    wait(&service, |v| {
        matches!(
            v,
            TodayUsageDisplay::Value {
                total_tokens: 110,
                completeness: loki_metris_core::Completeness::Partial,
                ..
            }
        )
    });
    let progress = service.notification_progress();
    let snapshot = progress.snapshot.as_ref().unwrap();
    assert!(!snapshot.eligible(current_epoch_ms(), &snapshot.day, &snapshot.source));
    assert!(service.take_notification().is_none());
    service.finish();
}

/// 损坏通知历史只影响发送账本，不能把成功读到的可信零改成摘要失败。
#[test]
fn today_display_zero_survives_notification_ledger_error() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("data")).unwrap();
    std::fs::write(
        root.path().join("data/usage-notification-state.json"),
        b"{broken",
    )
    .unwrap();
    let service = indexed_service(root.path(), 0);
    service.configure_notifications(false, NotificationPermission::Denied);
    service.today_visible(true);
    wait(&service, |v| {
        matches!(
            v,
            TodayUsageDisplay::Value {
                total_tokens: 0,
                ..
            }
        )
    });
    assert!(service.notification_progress().error.is_some());
    assert!(service.take_notification().is_none());
    service.finish();
}

/// 在 UI 派发边界模拟已经持久预约的队列；同源读取失败要归还预约，不能发旧值或丢档。
#[test]
fn today_display_failed_read_defers_already_reserved_request() {
    let root = tempfile::tempdir().unwrap();
    let mut service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    let request = MilestoneRequest {
        id: "reserved-id".into(),
        day_key: "fixture".into(),
        source: "all".into(),
        reserved_ms: current_epoch_ms(),
        bucket: 1,
        total_tokens: 110_000_000,
    };
    let (requests, rx) = mpsc::channel(1);
    let (receipts, mut outcomes) = mpsc::channel(1);
    let mut progress = Progress::default();
    progress.today.failed = true;
    let (tx, projected) = watch::channel(progress);
    let monitor = &mut Arc::get_mut(&mut service).unwrap().notifications;
    monitor.progress = projected;
    monitor.requests = Mutex::new(rx);
    monitor.receipts = receipts;
    requests.try_send((0, request.clone())).unwrap();
    assert!(service.take_notification().is_none());
    assert!(matches!(outcomes.try_recv().unwrap(), Receipt::Unsubmitted(id) if id == request.id));
    tx.send_modify(|p| p.today.failed = false);
    requests.try_send((0, request.clone())).unwrap();
    assert_eq!(service.take_notification().unwrap().id, request.id);
    service.finish();
}
