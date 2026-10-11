//! 隔离验证不可变响应事实的重放、冲突与旧协议前缀保留。

use super::observed::insert_observed_usage_batch;
use super::shared::statement;
use super::tests::{call, test_connection};
use crate::local_index::{LocalError, LocalErrorKind};
use crate::{Confidence, UsageCall};
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};

/// 模拟独立扫描批次的事务提交；出错时显式回滚便于复读。
async fn observed(
    connection: &DatabaseConnection,
    calls: Vec<UsageCall>,
) -> Result<u64, LocalError> {
    observed_generation(connection, 1, calls).await
}

/// 每代独立事务模拟真实重建，失败显式回滚，不泄漏暂存代。
async fn observed_generation(
    connection: &DatabaseConnection,
    generation: u64,
    calls: Vec<UsageCall>,
) -> Result<u64, LocalError> {
    observed_filtered_generation(connection, generation, i64::MIN, calls).await
}

/// 将保留下界交给 core，在同一事务内核对首次时间、冲突与实际插入。
async fn observed_filtered_generation(
    connection: &DatabaseConnection,
    generation: u64,
    cutoff: i64,
    mut calls: Vec<UsageCall>,
) -> Result<u64, LocalError> {
    let transaction = connection.begin().await?;
    let result =
        insert_observed_usage_batch(&transaction, "source", generation, cutoff, &mut calls).await;
    match result {
        Ok(added) => {
            transaction.commit().await?;
            Ok(added)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

/// 严格写入还读取已公开 generation；用最小来源表隔离 core 事务测试。
async fn observed_connection() -> DatabaseConnection {
    let connection = test_connection().await;
    connection
        .execute_unprepared(
            "CREATE TABLE source_files (source_id TEXT PRIMARY KEY, generation INTEGER NOT NULL)",
        )
        .await
        .unwrap();
    connection
}

/// 构造完整响应账单，身份可跨批重复。
fn response(id: &str, input: u64, cached: Option<u64>, write: Option<u64>) -> UsageCall {
    call(id, "owner", Confidence::Exact, input, cached, write, 2)
}

/// 同值重放在同批、跨批均幂等，附属展示或检查点变化不覆盖首次账单。
#[tokio::test]
async fn repeated_observed_response_is_idempotent() {
    let connection = observed_connection().await;
    let original = response("response-a", 10, Some(0), Some(0));
    assert_eq!(
        observed(&connection, vec![original.clone(), original.clone()])
            .await
            .unwrap(),
        1
    );
    let mut repeated = original;
    repeated.model = Some("changed-label".into());
    repeated.adapter_consistency_key = Some("changed-checkpoint".into());
    assert_eq!(observed(&connection, vec![repeated]).await.unwrap(), 0);
}

/// 同批矛盾回滚整批；跨批矛盾保留原行及原始用量。
#[tokio::test]
async fn conflicting_observed_response_never_overwrites_or_partly_commits() {
    let connection = observed_connection().await;
    let original = response("response-a", 10, Some(0), Some(0));
    let conflicting = response("response-a", 20, Some(0), Some(0));
    assert_eq!(
        observed(&connection, vec![original.clone(), conflicting.clone()])
            .await
            .unwrap_err()
            .kind(),
        LocalErrorKind::InvalidUsage
    );
    assert_eq!(
        observed(&connection, vec![original]).await.unwrap(),
        1,
        "failed batch inserted nothing"
    );
    assert_eq!(
        observed(
            &connection,
            vec![response("response-b", 30, Some(0), Some(0)), conflicting]
        )
        .await
        .unwrap_err()
        .kind(),
        LocalErrorKind::InvalidUsage
    );
    let row = connection
        .query_one(statement(
            "SELECT count(*),sum(input_tokens) FROM usage_calls",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get_by_index::<i64>(0).unwrap(), 1);
    assert_eq!(row.try_get_by_index::<i64>(1).unwrap(), 10);
}

/// 可选未知与已知零、缓存写入及推算标记都属于账单事实，不能静默覆盖。
#[tokio::test]
async fn observed_conflicts_include_component_availability_and_derived_total() {
    let connection = observed_connection().await;
    let original = response("response-a", 10, None, None);
    observed(&connection, vec![original.clone()]).await.unwrap();
    let mut variants = vec![
        response("response-a", 10, Some(0), None),
        response("response-a", 10, None, Some(0)),
    ];
    let mut derived = original.clone();
    derived.usage.total_is_derived = true;
    variants.push(derived);
    let mut reasoning = original;
    reasoning.usage.reasoning_output_tokens = None;
    variants.push(reasoning);
    for conflicting in variants {
        assert_eq!(
            observed(&connection, vec![conflicting])
                .await
                .unwrap_err()
                .kind(),
            LocalErrorKind::InvalidUsage
        );
    }
}

/// 现代精确调用不得删除旧协议中属于另一调用的 Derived 前缀。
#[tokio::test]
async fn observed_response_keeps_legacy_derived_prefix() {
    let connection = observed_connection().await;
    let transaction = connection.begin().await.unwrap();
    let mut prefix = vec![call(
        "legacy-prefix",
        "owner",
        Confidence::Derived,
        7,
        Some(0),
        Some(0),
        1,
    )];
    super::codex::insert_usage_batch(&transaction, "source", 1, &mut prefix)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(
        observed(
            &connection,
            vec![response("response-a", 10, Some(0), Some(0))]
        )
        .await
        .unwrap(),
        1
    );
    let row = connection
        .query_one(statement(
            "SELECT count(*),sum(total_tokens) FROM usage_calls",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get_by_index::<i64>(0).unwrap(), 2);
    assert_eq!(row.try_get_by_index::<i64>(1).unwrap(), 20);
}

/// 新代同值可重放；同 ID 的变化不能借重建绕过旧代事实一致性。
#[tokio::test]
async fn observed_rebuild_checks_published_bill_without_replacing_it() {
    let connection = observed_connection().await;
    let original = response("response-a", 10, Some(0), Some(0));
    observed(&connection, vec![original.clone()]).await.unwrap();
    connection
        .execute_unprepared("INSERT INTO source_files VALUES ('source',1)")
        .await
        .unwrap();
    let mut replay = original;
    replay.occurred_at_epoch_ms = 2_000;
    assert_eq!(
        observed_generation(&connection, 2, vec![replay])
            .await
            .unwrap(),
        1
    );
    let row = connection
        .query_one(statement(
            "SELECT occurred_at_epoch_ms FROM usage_calls WHERE generation=2",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get_by_index::<i64>(0).unwrap(),
        1_000,
        "rebuild replay cannot move the same response to a different day"
    );
    assert_eq!(
        observed_generation(
            &connection,
            3,
            vec![response("response-a", 20, Some(0), Some(0))]
        )
        .await
        .unwrap_err()
        .kind(),
        LocalErrorKind::InvalidUsage
    );
    assert_eq!(
        observed_generation(
            &connection,
            2,
            vec![response("response-b", 30, Some(0), Some(0))]
        )
        .await
        .unwrap(),
        1
    );
    let row = connection
        .query_one(statement(
            "SELECT count(*),sum(input_tokens) FROM usage_calls WHERE generation=3",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get_by_index::<i64>(0).unwrap(), 0);
}

/// 首次时间决定保留期；重放不能丢掉保留账单、延长保留期或隐藏矛盾事实。
#[tokio::test]
async fn observed_retention_uses_original_time_before_filtering() {
    let connection = observed_connection().await;
    let original = response("response-a", 10, Some(0), Some(0));
    observed(&connection, vec![original.clone()]).await.unwrap();
    connection
        .execute_unprepared("INSERT INTO source_files VALUES ('source',1)")
        .await
        .unwrap();
    let mut early = original.clone();
    early.occurred_at_epoch_ms = 500;
    assert_eq!(
        observed_filtered_generation(&connection, 2, 750, vec![early])
            .await
            .unwrap(),
        1
    );
    let mut late = original;
    late.occurred_at_epoch_ms = 1_500;
    assert_eq!(
        observed_filtered_generation(&connection, 3, 1_250, vec![late])
            .await
            .unwrap(),
        0
    );
    let mut conflict = response("response-a", 20, Some(0), Some(0));
    conflict.occurred_at_epoch_ms = 500;
    assert_eq!(
        observed_filtered_generation(&connection, 3, 1_250, vec![conflict])
            .await
            .unwrap_err()
            .kind(),
        LocalErrorKind::InvalidUsage
    );
    let row = connection
        .query_one(statement(
            "SELECT count(*),min(occurred_at_epoch_ms) FROM usage_calls WHERE generation=2",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get_by_index::<i64>(0).unwrap(), 1);
    assert_eq!(row.try_get_by_index::<i64>(1).unwrap(), 1_000);
}
