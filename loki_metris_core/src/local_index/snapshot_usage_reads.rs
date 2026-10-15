// 本文件是 snapshot.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：用有界聚合与 EXISTS 查询保持已覆盖持久化不变量的 fail-closed 边界，；从指定数据库快照读取当前 generation 调用并由 core 执行 canonical 去重。；从指定时间下界读取当前 generation 调用，避免视图装载窗口外全历史。

/// 用有界聚合与 EXISTS 查询保持已覆盖持久化不变量的 fail-closed 边界，
/// 不把调用或快照实体化。
async fn validate_current_usage_integrity<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
) -> Result<(), LocalError> {
    let row = connection
        .query_one_raw(statement(
            "SELECT
               (SELECT COALESCE(MAX(
                  CASE
                    WHEN (
                      typeof(u.logical_call_id) <> 'text'
                      OR typeof(u.occurred_at_epoch_ms) <> 'integer'
                      OR (u.model IS NOT NULL AND typeof(u.model) <> 'text')
                      OR (u.reasoning_effort IS NOT NULL
                          AND typeof(u.reasoning_effort) <> 'text')
                      OR (u.project_key IS NOT NULL AND typeof(u.project_key) <> 'text')
                      OR typeof(u.thread_key) <> 'text'
                      OR (u.project_label IS NOT NULL
                          AND typeof(u.project_label) <> 'text')
                      OR (u.thread_label IS NOT NULL
                          AND typeof(u.thread_label) <> 'text')
                      OR typeof(u.input_tokens) <> 'integer' OR u.input_tokens < 0
                      OR typeof(u.cached_input_tokens) <> 'integer'
                         OR u.cached_input_tokens < 0
                      OR (u.cache_write_input_tokens IS NOT NULL
                          AND (typeof(u.cache_write_input_tokens) <> 'integer'
                               OR u.cache_write_input_tokens < 0))
                      OR typeof(u.output_tokens) <> 'integer' OR u.output_tokens < 0
                      OR typeof(u.reasoning_output_tokens) <> 'integer'
                         OR u.reasoning_output_tokens < 0
                      OR typeof(u.total_tokens) <> 'integer' OR u.total_tokens < 0
                      OR typeof(u.total_is_derived) <> 'integer'
                      OR typeof(u.confidence) <> 'text'
                         OR u.confidence NOT IN ('exact', 'derived', 'suspected')
                      OR typeof(u.cached_input_available) <> 'integer'
                      OR typeof(u.reasoning_output_available) <> 'integer'
                      OR (u.adapter_consistency_key IS NOT NULL
                          AND typeof(u.adapter_consistency_key) <> 'text')
                      OR typeof(f.source_id) <> 'text'
                      OR typeof(f.root_id) <> 'text'
                      OR typeof(f.relative_label) <> 'text'
                      OR typeof(f.archived) <> 'integer'
                    ) THEN 2
                    WHEN (
                      (u.cached_input_available <> 0
                       AND u.cached_input_tokens > u.input_tokens)
                      OR (u.reasoning_output_available <> 0
                          AND u.reasoning_output_tokens > u.output_tokens)
                      OR (u.total_is_derived = 0
                          AND (u.total_tokens < u.input_tokens
                               OR u.total_tokens - u.input_tokens < u.output_tokens))
                    ) THEN 1
                    ELSE 0
                  END
                ), 0)
                FROM usage_calls u
                JOIN source_files f ON f.source_id = u.source_id
                JOIN source_roots r ON r.root_id = f.root_id
                WHERE f.ready = 1 AND f.parser_version = ?1
                  AND r.enabled = 1 AND f.generation = u.generation),
               EXISTS(
                 SELECT 1 FROM usage_token_snapshots s
                 JOIN source_files f ON f.source_id = s.source_id
                 JOIN source_roots r ON r.root_id = f.root_id
                 WHERE f.ready = 1 AND f.parser_version = ?1
                   AND r.enabled = 1 AND f.generation = s.generation
                   AND (
                     typeof(s.thread_key) <> 'text'
                     OR typeof(s.occurred_at_epoch_ms) <> 'integer'
                     OR typeof(s.total_tokens) <> 'integer' OR s.total_tokens < 0
                     OR typeof(s.logical_call_id) <> 'text'
                     OR (s.model IS NOT NULL AND typeof(s.model) <> 'text')
                     OR (s.reasoning_effort IS NOT NULL
                         AND typeof(s.reasoning_effort) <> 'text')
                     OR (s.project_key IS NOT NULL AND typeof(s.project_key) <> 'text')
                     OR typeof(f.source_id) <> 'text'
                     OR typeof(f.root_id) <> 'text'
                     OR typeof(f.relative_label) <> 'text'
                     OR typeof(f.archived) <> 'integer'
                   )
               )",
            vec![i64::from(parser_version).into()],
        ))
        .await?
        .ok_or_else(|| {
            LocalError::new(LocalErrorKind::Database, "usage integrity is unreadable")
        })?;
    let call_integrity_kind: i64 = row.try_get_by_index(0)?;
    let malformed_snapshot: bool = row.try_get_by_index(1)?;
    if call_integrity_kind == 2 || malformed_snapshot {
        return Err(LocalError::new(
            LocalErrorKind::Database,
            "stored usage row is invalid",
        ));
    }
    if call_integrity_kind == 1 {
        return Err(LocalError::new(
            LocalErrorKind::InvalidUsage,
            "local token usage is invalid",
        ));
    }
    if call_integrity_kind != 0 {
        return Err(LocalError::new(
            LocalErrorKind::Database,
            "usage integrity result is invalid",
        ));
    }
    Ok(())
}

/// 从指定数据库快照读取当前 generation 调用并由 core 执行 canonical 去重。
pub(crate) async fn load_canonical_calls<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
) -> Result<CanonicalUsageSet, LocalError> {
    load_canonical_calls_since(connection, parser_version, i64::MIN).await
}

/// 从指定时间下界读取当前 generation 调用，避免视图装载窗口外全历史。
pub(crate) async fn load_canonical_calls_since<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
    cutoff_epoch_ms: i64,
) -> Result<CanonicalUsageSet, LocalError> {
    let canonical =
        load_canonical_usage_calls_since(connection, parser_version, cutoff_epoch_ms).await?;
    let snapshot_rows = connection
        .query_all_raw(statement(
            "SELECT s.thread_key, s.occurred_at_epoch_ms, s.total_tokens, s.logical_call_id,
                    s.model, s.reasoning_effort, s.project_key,
                    f.source_id, f.root_id, f.relative_label, f.archived
             FROM usage_token_snapshots s
             JOIN source_files f ON f.source_id = s.source_id
             JOIN source_roots r ON r.root_id = f.root_id
             WHERE f.ready = 1 AND f.parser_version = ?1
               AND r.enabled = 1
               AND f.generation = s.generation
               AND s.occurred_at_epoch_ms >= ?2
             ORDER BY s.occurred_at_epoch_ms, s.logical_call_id, s.thread_key",
            vec![i64::from(parser_version).into(), cutoff_epoch_ms.into()],
        ))
        .await?;
    let mut snapshots = Vec::with_capacity(snapshot_rows.len());
    for row in &snapshot_rows {
        snapshots.push(row_to_session_snapshot(row)?);
    }
    drop(snapshot_rows);
    Ok(attach_session_snapshots(canonical, snapshots))
}

/// 从指定时间下界只读取并规范化调用，不实体化兼容性会话快照。
async fn load_canonical_usage_calls_since<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
    cutoff_epoch_ms: i64,
) -> Result<CanonicalUsageSet, LocalError> {
    let rows = connection
        .query_all_raw(statement(
            "SELECT u.logical_call_id, u.occurred_at_epoch_ms, u.model,
                    u.reasoning_effort, u.project_key, u.thread_key,
                    u.project_label, u.thread_label,
                    u.input_tokens, u.cached_input_tokens, u.cache_write_input_tokens,
                    u.output_tokens, u.reasoning_output_tokens, u.total_tokens,
                    u.total_is_derived, u.confidence,
                    u.cached_input_available, u.reasoning_output_available,
                    u.adapter_consistency_key,
                    f.source_id, f.root_id, f.relative_label, f.archived
             FROM usage_calls u
             JOIN source_files f ON f.source_id = u.source_id
             JOIN source_roots r ON r.root_id = f.root_id
             WHERE f.ready = 1 AND f.parser_version = ?1
               AND r.enabled = 1
               AND f.generation = u.generation
               AND u.occurred_at_epoch_ms >= ?2
             ORDER BY u.occurred_at_epoch_ms, u.logical_call_id, f.root_id, f.source_id",
            vec![i64::from(parser_version).into(), cutoff_epoch_ms.into()],
        ))
        .await?;
    let mut calls = Vec::with_capacity(rows.len());
    // 解码后释放该行列值载荷，避免整批原始行与领域调用同时保留。
    for row in rows {
        calls.push(row_to_usage_call(&row)?);
    }
    Ok(canonicalize_usage_calls(calls))
}

/// 将数据库行严格解码为会话 Token 快照，拒绝越界或损坏数值。
fn row_to_session_snapshot(row: &sea_orm::QueryResult) -> Result<SessionTokenSnapshot, LocalError> {
    Ok(SessionTokenSnapshot {
        thread_key: row.try_get_by_index(0)?,
        occurred_at_epoch_ms: row.try_get_by_index(1)?,
        cumulative_total_tokens: from_sql_u64(row.try_get_by_index(2)?)?,
        logical_call_id: row.try_get_by_index(3)?,
        model: row.try_get_by_index(4)?,
        reasoning_effort: row.try_get_by_index(5)?,
        project_key: row.try_get_by_index(6)?,
        provenance: vec![SourceProvenance {
            source_id: row.try_get_by_index(7)?,
            root_id: row.try_get_by_index(8)?,
            relative_label: row.try_get_by_index(9)?,
            archived: row.try_get_by_index(10)?,
        }],
    })
}
