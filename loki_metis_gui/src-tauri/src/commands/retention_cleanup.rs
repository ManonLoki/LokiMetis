//! 按已保存天数在启动时清理派生用量，不触碰原始会话或数据根。

use loki_metis_core::{LocalIndex, SourceClientKind, source_client_usage_index_path};
use tauri::{AppHandle, Manager};

use crate::backend::local_index::ScanPermit;
use crate::runtime::{AppRuntimeState, BackgroundTaskShutdown, now_epoch_ms};

/// 进程启动后安排一次后台清理；初始化未完成则不读索引。
pub(crate) fn spawn_retention_cleanup(app_handle: AppHandle) {
    let task_app_handle = app_handle.clone();
    if let Err(error) = app_handle
        .state::<AppRuntimeState>()
        .background_tasks
        .spawn(
            "startup-retention-cleanup",
            move |mut shutdown| async move {
                let state = task_app_handle.state::<AppRuntimeState>();
                if let Err(error) = prune_derived_usage_for_saved_retention_with_shutdown(
                    state.inner(),
                    now_epoch_ms(),
                    Some(&mut shutdown),
                )
                .await
                {
                    tracing::warn!(%error, "startup retention cleanup did not finish");
                }
            },
        )
    {
        tracing::warn!(%error, "startup retention cleanup was rejected during shutdown");
    }
}

/// 用当前已保存天数与设备当地民用日删除窗外派生调用和累计快照。
#[cfg(test)]
pub(crate) async fn prune_derived_usage_for_saved_retention(
    state: &AppRuntimeState,
    observed_at_epoch_ms: i64,
) -> Result<(), String> {
    prune_derived_usage_for_saved_retention_with_shutdown(state, observed_at_epoch_ms, None).await
}

/// 启动任务额外观察应用关闭，避免退出后继续打开下一个索引。
async fn prune_derived_usage_for_saved_retention_with_shutdown(
    state: &AppRuntimeState,
    observed_at_epoch_ms: i64,
    mut shutdown: Option<&mut BackgroundTaskShutdown>,
) -> Result<(), String> {
    if shutdown
        .as_deref()
        .is_some_and(BackgroundTaskShutdown::is_cancelled)
    {
        return Ok(());
    }
    if !state.initialization_completed().await {
        return Ok(());
    }
    let days = state.retention_days().await;
    // 自动清理不是页面工作状态；始终按设备当地民用日执行。
    let time_standard = loki_metis_core::TimeStandard::Local;
    let cutoff = loki_metis_core::retention_cutoff_epoch_ms(
        days,
        observed_at_epoch_ms,
        &time_standard,
        &jiff::tz::TimeZone::system(),
    )
    .map_err(|_| "自动清理无法计算保留窗口。".to_owned())?;

    // 三个 registry 项共享同一个 coordinator。启动清理必须可靠地排在当前 writer
    // 后面，并在一个许可内处理全部索引，避免客户端之间释放许可后被周期扫描插队。
    let Some(write_permit) = wait_for_shared_writer(state, shutdown.as_deref_mut()).await? else {
        return Ok(());
    };
    let writer_cancellation = write_permit.cancellation_token();
    for client in [
        SourceClientKind::Codex,
        SourceClientKind::ClaudeCode,
        SourceClientKind::GrokBuildCli,
    ] {
        if writer_cancellation.is_cancelled()
            || shutdown
                .as_deref()
                .is_some_and(BackgroundTaskShutdown::is_cancelled)
        {
            break;
        }
        prune_client_derived_usage(&state.app_data_dir, client, cutoff).await;
    }
    Ok(())
}

/// 等待共享索引 writer；应用关闭必须能打断排队，不能把启动清理遗留到退出之后。
async fn wait_for_shared_writer(
    state: &AppRuntimeState,
    shutdown: Option<&mut BackgroundTaskShutdown>,
) -> Result<Option<ScanPermit>, String> {
    let coordinator = state.local_scan.get(SourceClientKind::Codex);
    let permit = match shutdown {
        Some(shutdown) => {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => return Ok(None),
                permit = coordinator.start_when_available() => permit,
            }
        }
        None => coordinator.start_when_available().await,
    };
    permit
        .map(Some)
        .map_err(|_| "自动清理无法取得本机索引写入许可。".to_owned())
}

/// 打开已有物理库并按发生时间裁剪派生用量；文件不存在则跳过。
async fn prune_client_derived_usage(
    app_data_dir: &std::path::Path,
    client: SourceClientKind,
    cutoff_epoch_ms: i64,
) {
    let database_path = source_client_usage_index_path(app_data_dir, client);
    if !database_path.is_file() {
        return;
    }
    let client_dir = loki_metis_core::source_client_app_data_dir(app_data_dir, client);
    let result = async {
        let mut index = LocalIndex::open_in_app_data(&client_dir, client.parser_version()).await?;
        index.prune_usage_before(cutoff_epoch_ms).await
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(
            ?client,
            kind = ?error.kind(),
            "startup retention cleanup skipped one local index"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy_store::save_settings;
    use crate::runtime::AppRuntimeState;
    use loki_metis_core::{RetentionDays, SourceClientKind, source_client_app_data_dir};
    use sea_orm::{ConnectOptions, ConnectionTrait, Database};
    use tempfile::tempdir;

    /// 窗外派生调用必须删除，窗内调用、数据根与来源 checkpoint 必须保留。
    #[tokio::test]
    async fn startup_prune_removes_only_derived_usage_before_saved_retention() {
        let temp = tempdir().expect("isolated app-data exists");
        let settings = crate::privacy_store::LocalPrivacySettings {
            initialization_completed: true,
            retention_days: RetentionDays::new(90).expect("90 days is approved"),
            ..Default::default()
        };
        save_settings(temp.path(), &settings).expect("settings persist");

        let observed = 1_775_000_000_000_i64;
        let cutoff = loki_metis_core::retention_cutoff_epoch_ms(
            settings.retention_days,
            observed,
            &loki_metis_core::TimeStandard::Local,
            &jiff::tz::TimeZone::system(),
        )
        .expect("cutoff exists");
        let client_dir = source_client_app_data_dir(temp.path(), SourceClientKind::Codex);
        let index =
            LocalIndex::open_in_app_data(&client_dir, SourceClientKind::Codex.parser_version())
                .await
                .expect("index opens");
        let database_path = index.database_path().to_path_buf();
        drop(index);
        insert_retention_fixture(
            &database_path,
            SourceClientKind::Codex,
            cutoff.saturating_sub(1),
            cutoff,
        )
        .await;

        let state = AppRuntimeState::new(temp.path().to_path_buf());
        prune_derived_usage_for_saved_retention(&state, observed)
            .await
            .expect("startup prune runs");

        let index =
            LocalIndex::open_in_app_data(&client_dir, SourceClientKind::Codex.parser_version())
                .await
                .expect("index reopens");
        let remaining = index.canonical_calls().await.expect("calls remain");
        assert_eq!(remaining.calls.len(), 1);
        assert_eq!(remaining.calls[0].logical_call_id, "inside");
        assert!(
            index
                .has_current_parser_usage()
                .await
                .expect("source checkpoint remains")
        );
        let roots = index.all_roots().await.expect("roots remain");
        assert_eq!(roots.len(), 1);
    }

    /// 无向导时启动清理仍按已保存天数删除窗外派生用量。
    #[tokio::test]
    async fn startup_prune_runs_without_wizard() {
        let temp = tempdir().expect("isolated app-data exists");
        let settings = crate::privacy_store::LocalPrivacySettings::default();
        save_settings(temp.path(), &settings).expect("incomplete settings persist");
        let observed = 1_775_000_000_000_i64;
        let cutoff = loki_metis_core::retention_cutoff_epoch_ms(
            settings.retention_days,
            observed,
            &loki_metis_core::TimeStandard::Local,
            &jiff::tz::TimeZone::system(),
        )
        .expect("cutoff exists");
        let client_dir = source_client_app_data_dir(temp.path(), SourceClientKind::Codex);
        let index =
            LocalIndex::open_in_app_data(&client_dir, SourceClientKind::Codex.parser_version())
                .await
                .expect("index opens");
        let database_path = index.database_path().to_path_buf();
        drop(index);
        insert_retention_fixture(
            &database_path,
            SourceClientKind::Codex,
            cutoff.saturating_sub(1),
            cutoff,
        )
        .await;

        let state = AppRuntimeState::new(temp.path().to_path_buf());
        prune_derived_usage_for_saved_retention(&state, observed)
            .await
            .expect("startup prune runs without wizard");

        let index =
            LocalIndex::open_in_app_data(&client_dir, SourceClientKind::Codex.parser_version())
                .await
                .expect("index reopens");
        assert_eq!(
            index
                .canonical_calls()
                .await
                .expect("calls remain")
                .calls
                .len(),
            1
        );
    }

    /// 初始 writer 忙时启动清理必须排队，并在同一许可内覆盖启用与禁用客户端。
    #[tokio::test]
    async fn startup_prune_waits_for_busy_writer_and_prunes_every_client() {
        let temp = tempdir().expect("isolated app-data exists");
        let settings = crate::privacy_store::LocalPrivacySettings {
            enabled_agents: loki_metis_core::EnabledAgents::empty()
                .with(SourceClientKind::Codex, true),
            ..Default::default()
        };
        save_settings(temp.path(), &settings).expect("settings persist");
        let observed = 1_775_000_000_000_i64;
        let cutoff = loki_metis_core::retention_cutoff_epoch_ms(
            settings.retention_days,
            observed,
            &loki_metis_core::TimeStandard::Local,
            &jiff::tz::TimeZone::system(),
        )
        .expect("cutoff exists");
        let clients = [
            SourceClientKind::Codex,
            SourceClientKind::ClaudeCode,
            SourceClientKind::GrokBuildCli,
        ];
        for client in clients {
            let client_dir = source_client_app_data_dir(temp.path(), client);
            let index = LocalIndex::open_in_app_data(&client_dir, client.parser_version())
                .await
                .expect("index opens");
            let database_path = index.database_path().to_path_buf();
            drop(index);
            insert_retention_fixture(&database_path, client, cutoff.saturating_sub(1), cutoff)
                .await;
        }

        let state = std::sync::Arc::new(AppRuntimeState::new(temp.path().to_path_buf()));
        let scan_permit = state
            .local_scan
            .get(SourceClientKind::Codex)
            .try_start()
            .expect("scan writer is occupied");
        let cleanup_state = std::sync::Arc::clone(&state);
        let mut cleanup = tokio::spawn(async move {
            prune_derived_usage_for_saved_retention(&cleanup_state, observed).await
        });

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut cleanup)
                .await
                .is_err(),
            "startup cleanup must remain queued behind the active writer"
        );
        drop(scan_permit);
        tokio::time::timeout(std::time::Duration::from_secs(2), cleanup)
            .await
            .expect("queued cleanup finishes after the writer is released")
            .expect("cleanup task joins")
            .expect("queued startup prune succeeds");

        for client in clients {
            let client_dir = source_client_app_data_dir(temp.path(), client);
            let index = LocalIndex::open_in_app_data(&client_dir, client.parser_version())
                .await
                .expect("index reopens");
            let remaining = index.canonical_calls().await.expect("calls remain");
            assert_eq!(remaining.calls.len(), 1, "{client:?} is pruned");
            assert_eq!(remaining.calls[0].logical_call_id, "inside");
        }
    }

    /// 应用关闭必须打断 writer 排队并让 owner 及时回收启动清理任务。
    #[tokio::test]
    async fn startup_prune_writer_wait_is_cancelled_by_shutdown() {
        let temp = tempdir().expect("isolated app-data exists");
        let settings = crate::privacy_store::LocalPrivacySettings::default();
        save_settings(temp.path(), &settings).expect("settings persist");
        let observed = 1_775_000_000_000_i64;
        let cutoff = loki_metis_core::retention_cutoff_epoch_ms(
            settings.retention_days,
            observed,
            &loki_metis_core::TimeStandard::Local,
            &jiff::tz::TimeZone::system(),
        )
        .expect("cutoff exists");
        let client_dir = source_client_app_data_dir(temp.path(), SourceClientKind::Codex);
        let index =
            LocalIndex::open_in_app_data(&client_dir, SourceClientKind::Codex.parser_version())
                .await
                .expect("index opens");
        let database_path = index.database_path().to_path_buf();
        drop(index);
        insert_retention_fixture(
            &database_path,
            SourceClientKind::Codex,
            cutoff.saturating_sub(1),
            cutoff,
        )
        .await;

        let state = std::sync::Arc::new(AppRuntimeState::new(temp.path().to_path_buf()));
        let scan_permit = state
            .local_scan
            .get(SourceClientKind::Codex)
            .try_start()
            .expect("scan writer is occupied");
        let cleanup_state = std::sync::Arc::clone(&state);
        let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
        state
            .background_tasks
            .spawn("retention-cleanup-test", move |mut shutdown| async move {
                let result = prune_derived_usage_for_saved_retention_with_shutdown(
                    &cleanup_state,
                    observed,
                    Some(&mut shutdown),
                )
                .await;
                let _ = result_sender.send(result);
            })
            .expect("cleanup task is owned");

        tokio::task::yield_now().await;
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            state.background_tasks.shutdown(),
        )
        .await
        .expect("shutdown cancels the queued cleanup");
        result_receiver
            .await
            .expect("cleanup reports its result")
            .expect("cancelled cleanup exits successfully");
        assert!(
            state.local_scan.get(SourceClientKind::Codex).is_running(),
            "cleanup cancellation must not release another task's writer permit"
        );
        drop(scan_permit);

        let index =
            LocalIndex::open_in_app_data(&client_dir, SourceClientKind::Codex.parser_version())
                .await
                .expect("index reopens");
        assert_eq!(
            index
                .canonical_calls()
                .await
                .expect("calls remain")
                .calls
                .len(),
            2,
            "shutdown cancellation must not begin retention writes"
        );
    }

    /// 往已迁移库写入一根、一条窗外调用和一条窗内调用。
    async fn insert_retention_fixture(
        database_path: &std::path::Path,
        client: SourceClientKind,
        outside_epoch_ms: i64,
        inside_epoch_ms: i64,
    ) {
        let app_data_dir = database_path
            .parent()
            .expect("database has an app-data parent");
        let mut index = LocalIndex::open_in_app_data(app_data_dir, client.parser_version())
            .await
            .expect("fixture index opens");
        index
            .register_confirmed_root_fields(
                "root-keep",
                &app_data_dir.join("retention-root"),
                "Keep",
                loki_metis_core::DiscoveryMethod::Registered,
            )
            .await
            .expect("fixture root registers");
        drop(index);
        let mut options = ConnectOptions::new("sqlite://placeholder.sqlite3");
        let database_path = database_path.to_path_buf();
        options.map_sqlx_sqlite_opts(move |sqlite_options| {
            sqlite_options
                .filename(&database_path)
                .create_if_missing(true)
        });
        let connection = Database::connect(options)
            .await
            .expect("migrated database reopens");
        connection
            .execute_unprepared(&format!(
                "UPDATE source_roots
                    SET last_coverage_state = 'complete', activation_state = 'ready'
                  WHERE root_id = 'root-keep';
                 INSERT INTO source_files
                   (source_id, root_id, relative_label, file_identity, archived,
                    observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                    oversized_tail, parser_version, generation, ready, thread_key,
                    project_key, model, reasoning_effort, call_sequence, token_snapshots_ready)
                 VALUES
                   ('source-keep', 'root-keep', 'sessions/test.jsonl', 'file', 0,
                    10, 200, 10, 0, 0, {parser}, 1, 1, 'thread', NULL, NULL, NULL, 2, 1);
                 INSERT INTO usage_calls
                   (source_id, generation, logical_call_id, occurred_at_epoch_ms,
                    model, reasoning_effort, project_key, thread_key, input_tokens,
                    cached_input_tokens, cache_write_input_tokens, output_tokens,
                    reasoning_output_tokens, total_tokens, total_is_derived, confidence)
                 VALUES
                   ('source-keep', 1, 'outside', {outside}, NULL, NULL, NULL, 'thread', 1, 0, NULL, 0, 0, 1, 0, 'exact'),
                   ('source-keep', 1, 'inside', {inside}, NULL, NULL, NULL, 'thread', 2, 0, NULL, 0, 0, 2, 0, 'exact');
                 INSERT INTO usage_token_snapshots
                   (source_id, generation, thread_key, occurred_at_epoch_ms,
                    logical_call_id, total_tokens)
                 VALUES
                   ('source-keep', 1, 'thread', {outside}, 'outside', 1),
                   ('source-keep', 1, 'thread', {inside}, 'inside', 2);",
                parser = client.parser_version(),
                outside = outside_epoch_ms,
                inside = inside_epoch_ms,
            ))
            .await
            .expect("fixture is inserted");
    }
}
