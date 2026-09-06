//! 按已保存天数在启动时清理派生用量，不触碰原始会话或数据根。

use loki_metis_core::{LocalIndex, SourceClientKind, source_client_usage_index_path};
use tauri::{AppHandle, Manager};

use crate::runtime::{AppRuntimeState, now_epoch_ms};

/// 进程启动后安排一次后台清理；初始化未完成则不读索引。
pub(crate) fn spawn_retention_cleanup(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppRuntimeState>();
        if let Err(error) =
            prune_derived_usage_for_saved_retention(state.inner(), now_epoch_ms()).await
        {
            tracing::warn!(%error, "startup retention cleanup did not finish");
        }
    });
}

/// 用当前已保存天数与设备当地民用日删除窗外派生调用和累计快照。
pub(crate) async fn prune_derived_usage_for_saved_retention(
    state: &AppRuntimeState,
    observed_at_epoch_ms: i64,
) -> Result<(), String> {
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
    for client in [
        SourceClientKind::Codex,
        SourceClientKind::ClaudeCode,
        SourceClientKind::GrokBuildCli,
    ] {
        prune_client_derived_usage(&state.app_data_dir, client, cutoff).await;
    }
    Ok(())
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
        insert_retention_fixture(&database_path, cutoff.saturating_sub(1), cutoff).await;

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
        insert_retention_fixture(&database_path, cutoff.saturating_sub(1), cutoff).await;

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

    /// 往已迁移库写入一根、一条窗外调用和一条窗内调用。
    async fn insert_retention_fixture(
        database_path: &std::path::Path,
        outside_epoch_ms: i64,
        inside_epoch_ms: i64,
    ) {
        let app_data_dir = database_path
            .parent()
            .expect("database has an app-data parent");
        let mut index =
            LocalIndex::open_in_app_data(app_data_dir, SourceClientKind::Codex.parser_version())
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
                parser = SourceClientKind::Codex.parser_version(),
                outside = outside_epoch_ms,
                inside = inside_epoch_ms,
            ))
            .await
            .expect("fixture is inserted");
    }
}
