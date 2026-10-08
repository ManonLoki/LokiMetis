use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};

use super::super::LocalErrorKind;
use super::shared::statement;
use super::*;
use crate::{Confidence, TokenUsage, UsageCall};

/// 创建只含调用表的内存 schema，隔离验证批次事务与冲突语义。
async fn test_connection() -> DatabaseConnection {
    let connection = Database::connect(ConnectOptions::new("sqlite::memory:"))
        .await
        .expect("in-memory database opens");
    connection
        .execute_unprepared(
            "CREATE TABLE usage_calls (
               source_id TEXT NOT NULL,
               generation INTEGER NOT NULL,
               logical_call_id TEXT NOT NULL,
               occurred_at_epoch_ms INTEGER NOT NULL,
               model TEXT,
               reasoning_effort TEXT,
               project_key TEXT,
               thread_key TEXT NOT NULL,
               project_label TEXT,
               thread_label TEXT,
               input_tokens INTEGER NOT NULL,
               cached_input_tokens INTEGER NOT NULL,
               cache_write_input_tokens INTEGER,
               output_tokens INTEGER NOT NULL,
               reasoning_output_tokens INTEGER NOT NULL,
               total_tokens INTEGER NOT NULL,
               total_is_derived INTEGER NOT NULL,
               confidence TEXT NOT NULL,
               cached_input_available INTEGER NOT NULL,
               reasoning_output_available INTEGER NOT NULL,
               adapter_consistency_key TEXT,
               PRIMARY KEY(source_id, generation, logical_call_id)
             );",
        )
        .await
        .expect("usage call schema is created");
    connection
}

/// 构造不含正文或真实路径的规范化调用。
fn call(
    logical_call_id: &str,
    thread_key: &str,
    confidence: Confidence,
    input: u64,
    cached: Option<u64>,
    cache_write: Option<u64>,
    output: u64,
) -> UsageCall {
    UsageCall {
        logical_call_id: logical_call_id.to_owned(),
        occurred_at_epoch_ms: 1_000,
        model: Some("fixture-model".to_owned()),
        reasoning_effort: None,
        project_key: Some("fixture-project".to_owned()),
        thread_key: thread_key.to_owned(),
        project_label: None,
        thread_label: None,
        usage: TokenUsage::new_with_availability(
            input,
            cached,
            cache_write,
            output,
            Some(0),
            Some(input + output),
        )
        .expect("fixture usage is valid"),
        adapter_consistency_key: None,
        confidence,
        provenance: Vec::new(),
    }
}

/// 在独立事务中插入 Codex 测试批次，模拟并发提交边界。
async fn insert_batch_in_own_transaction(
    connection: &DatabaseConnection,
    source_id: &str,
    generation: u64,
    batch: &mut Vec<UsageCall>,
) -> Result<u64, LocalError> {
    let transaction = connection.begin().await?;
    let added = insert_usage_batch(&transaction, source_id, generation, batch).await?;
    transaction.commit().await?;
    Ok(added)
}

/// 在独立事务中插入 Claude 测试批次，验证物理库隔离后的提交语义。
async fn insert_claude_batch_in_own_transaction(
    connection: &DatabaseConnection,
    source_id: &str,
    generation: u64,
    batch: &mut Vec<UsageCall>,
) -> Result<ClaudeBatchOutcome, LocalError> {
    let transaction = connection.begin().await?;
    let outcome = insert_claude_usage_batch(&transaction, source_id, generation, batch).await?;
    transaction.commit().await?;
    Ok(outcome)
}

/// 验证批次写入仍保持冲突更新与 exact 优先规则。
#[tokio::test]
async fn batch_preserves_exact_and_derived_rules() {
    let connection = test_connection().await;
    let mut empty = Vec::new();
    assert_eq!(
        insert_batch_in_own_transaction(&connection, "source", 1, &mut empty)
            .await
            .expect("empty batch is accepted"),
        0
    );

    let mut derived = vec![call(
        "derived",
        "thread",
        Confidence::Derived,
        10,
        Some(2),
        Some(1),
        1,
    )];
    assert_eq!(
        insert_batch_in_own_transaction(&connection, "source", 1, &mut derived)
            .await
            .expect("derived call is inserted"),
        1
    );

    let mut refreshed = vec![call(
        "derived",
        "thread",
        Confidence::Derived,
        12,
        Some(3),
        Some(1),
        2,
    )];
    assert_eq!(
        insert_batch_in_own_transaction(&connection, "source", 1, &mut refreshed)
            .await
            .expect("duplicate logical call is refreshed"),
        0
    );
    let refreshed_row = connection
        .query_one(statement(
            "SELECT input_tokens FROM usage_calls WHERE logical_call_id = 'derived'",
            vec![],
        ))
        .await
        .expect("refreshed row query succeeds")
        .expect("refreshed row exists");
    assert_eq!(refreshed_row.try_get_by_index::<i64>(0).unwrap(), 12);

    let mut exact = vec![call(
        "exact",
        "thread",
        Confidence::Exact,
        20,
        Some(4),
        Some(2),
        3,
    )];
    assert_eq!(
        insert_batch_in_own_transaction(&connection, "source", 1, &mut exact)
            .await
            .expect("exact call replaces derived thread"),
        1
    );
    let mut late_derived = vec![call(
        "late-derived",
        "thread",
        Confidence::Derived,
        30,
        Some(5),
        Some(2),
        4,
    )];
    assert_eq!(
        insert_batch_in_own_transaction(&connection, "source", 1, &mut late_derived)
            .await
            .expect("late derived call is skipped"),
        0
    );
    let summary_row = connection
        .query_one(statement(
            "SELECT COUNT(*), MIN(confidence) FROM usage_calls",
            vec![],
        ))
        .await
        .expect("canonical rows query succeeds")
        .expect("canonical rows are readable");
    assert_eq!(summary_row.try_get_by_index::<i64>(0).unwrap(), 1);
    assert_eq!(
        summary_row.try_get_by_index::<String>(1).unwrap(),
        "exact".to_owned()
    );
}

/// 验证 Claude 批次只接受逐字段单调观察并保留最后可信事实。
#[tokio::test]
async fn claude_batch_rejects_regressions() {
    let connection = test_connection().await;
    let mut initial = vec![call(
        "message",
        "thread",
        Confidence::Exact,
        10,
        Some(2),
        Some(1),
        1,
    )];
    let outcome = insert_claude_batch_in_own_transaction(&connection, "source", 1, &mut initial)
        .await
        .expect("initial Claude call is inserted");
    assert_eq!((outcome.added, outcome.regressed), (1, 0));

    let mut monotonic = vec![call(
        "message",
        "thread",
        Confidence::Exact,
        12,
        Some(3),
        Some(1),
        2,
    )];
    let outcome = insert_claude_batch_in_own_transaction(&connection, "source", 1, &mut monotonic)
        .await
        .expect("monotonic Claude call is updated");
    assert_eq!((outcome.added, outcome.regressed), (0, 0));

    let mut regressed = vec![
        call(
            "message",
            "thread",
            Confidence::Exact,
            11,
            Some(3),
            Some(1),
            1,
        ),
        call("message", "thread", Confidence::Exact, 13, None, Some(1), 3),
    ];
    let outcome = insert_claude_batch_in_own_transaction(&connection, "source", 1, &mut regressed)
        .await
        .expect("regressions are isolated warnings");
    assert_eq!((outcome.added, outcome.regressed), (0, 2));
    let trusted_row = connection
        .query_one(statement(
            "SELECT input_tokens, output_tokens FROM usage_calls",
            vec![],
        ))
        .await
        .expect("trusted row query succeeds")
        .expect("trusted row remains readable");
    assert_eq!(trusted_row.try_get_by_index::<i64>(0).unwrap(), 12);
    assert_eq!(trusted_row.try_get_by_index::<i64>(1).unwrap(), 2);
}

/// 验证转换溢出与中途失败不会留下部分提交。
#[tokio::test]
async fn batch_keeps_overflow_classification_and_rolls_back() {
    let connection = test_connection().await;
    let mut overflow = vec![call(
        "overflow",
        "thread",
        Confidence::Exact,
        1,
        Some(0),
        Some(0),
        1,
    )];
    let error = insert_batch_in_own_transaction(&connection, "source", u64::MAX, &mut overflow)
        .await
        .expect_err("generation outside SQLite range is rejected");
    assert_eq!(error.kind(), LocalErrorKind::Overflow);

    connection
        .execute_unprepared(
            "CREATE TRIGGER reject_second_call
               BEFORE INSERT ON usage_calls
               WHEN NEW.logical_call_id = 'reject'
             BEGIN
               SELECT RAISE(ABORT, 'fixture failure');
             END;",
        )
        .await
        .expect("failure trigger is installed");
    let mut batch = vec![
        call(
            "first",
            "thread-a",
            Confidence::Exact,
            1,
            Some(0),
            Some(0),
            1,
        ),
        call(
            "reject",
            "thread-b",
            Confidence::Exact,
            1,
            Some(0),
            Some(0),
            1,
        ),
    ];
    let error = insert_batch_in_own_transaction(&connection, "source", 1, &mut batch)
        .await
        .expect_err("trigger aborts the transaction");
    assert_eq!(error.kind(), LocalErrorKind::Database);
    let count_row = connection
        .query_one(statement("SELECT COUNT(*) FROM usage_calls", vec![]))
        .await
        .expect("row count query succeeds")
        .expect("row count is readable");
    assert_eq!(count_row.try_get_by_index::<i64>(0).unwrap(), 0);
}

/// 验证一致性检查点严格按来源和 generation 隔离，并恢复最大顺序键。
#[tokio::test]
async fn latest_adapter_consistency_key_is_source_and_generation_scoped() {
    let connection = test_connection().await;
    let mut older = call(
        "older",
        "thread-a",
        Confidence::Exact,
        10,
        Some(2),
        Some(0),
        1,
    );
    older.adapter_consistency_key =
        Some("codex-v6:00000000000000000001:10:2:0:1:0:11:0".to_owned());
    let mut newer = call(
        "newer",
        "thread-b",
        Confidence::Exact,
        20,
        Some(4),
        Some(0),
        2,
    );
    let expected = "codex-v6:00000000000000000002:20:4:0:2:0:22:0".to_owned();
    newer.adapter_consistency_key = Some(expected.clone());
    let mut batch = vec![newer, older];
    insert_batch_in_own_transaction(&connection, "source", 3, &mut batch)
        .await
        .expect("checkpoint calls are inserted");
    let index = LocalIndex {
        connection,
        parser_version: 6,
        database_path: std::path::PathBuf::new(),
    };

    assert_eq!(
        index
            .latest_adapter_consistency_key("source", 3)
            .await
            .expect("latest checkpoint is readable"),
        Some(expected)
    );
    assert_eq!(
        index
            .latest_adapter_consistency_key("source", 4)
            .await
            .expect("other generation is readable"),
        None
    );
    assert_eq!(
        index
            .latest_adapter_consistency_key("other", 3)
            .await
            .expect("other source is readable"),
        None
    );
}
