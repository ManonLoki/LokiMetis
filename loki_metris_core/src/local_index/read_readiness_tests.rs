//! 用真实 SQLite 的最新扫描和根事实验证全部只读入口一致失败关闭。

use sea_orm::ConnectionTrait;
use super::LocalIndex;
use crate::{CoverageState, empty_coverage};

/// 三种入口必须在各自事务内得到同一资格，缓存 Complete 不能覆盖数据库事实。
async fn assert_readiness(index: &mut LocalIndex, expected: bool) {
    let snapshots = [
        index.usage_snapshot().await.unwrap(),
        index.usage_snapshot_since(i64::MIN).await.unwrap(),
        index.usage_view_snapshot_since(i64::MIN).await.unwrap(),
    ];
    for snapshot in snapshots {
        assert_eq!(snapshot.read_ready, expected);
        let mut cached = empty_coverage();
        cached.state = CoverageState::Complete;
        assert_eq!(snapshot.effective_coverage(cached).state,
            if expected { CoverageState::Complete } else { CoverageState::Partial });
        for state in [CoverageState::Failed, CoverageState::Cancelled] {
            let mut report = empty_coverage();
            report.state = state;
            assert_eq!(snapshot.effective_coverage(report).state, state);
        }
    }
}

/// 按 row_id 判断最新结果；旧成功不能跨过 running、失败、取消或警告。
#[tokio::test]
async fn latest_scan_and_all_enabled_roots_qualify_every_snapshot_entry() {
    let temp = tempfile::tempdir().unwrap();
    let mut writer = LocalIndex::open_in_app_data(temp.path(), 9).await.unwrap();
    writer.connection.execute_unprepared(
        "INSERT INTO source_roots (root_id, access_path, alias, enabled, discovery_method, last_coverage_state)
         VALUES ('root', X'01', 'Root', 1, 'registered', 'complete');"
    ).await.unwrap();
    let mut reader = LocalIndex::open_read_only_in_app_data(temp.path(), 9).await.unwrap();
    assert_readiness(&mut reader, false).await;
    let first = writer.begin_scan("quick", 100).await.unwrap();
    writer.finish_scan(&first, 101, "completed", false, 0, 0, 0).await.unwrap();
    assert_readiness(&mut reader, true).await;
    for (status, cancelled, warnings) in [
        ("failed", false, 0), ("cancelled", true, 0),
        ("completed", true, 0), ("completed", false, 1), ("unknown", false, 0),
    ] {
        // 时间可回拨，最新记录仍必须以数据库自增序号确定。
        let current = writer.begin_scan("quick", 1).await.unwrap();
        assert_readiness(&mut reader, false).await;
        writer.finish_scan(&current, 2, status, cancelled, 0, 0, warnings).await.unwrap();
        assert_readiness(&mut reader, false).await;
    }
    let recovered = writer.begin_scan("quick", 0).await.unwrap();
    writer.finish_scan(&recovered, 1, "completed", false, 0, 0, 0).await.unwrap();
    assert_readiness(&mut reader, true).await;
    writer.connection.execute_unprepared(
        "INSERT INTO source_roots (root_id, access_path, alias, enabled, discovery_method, last_coverage_state)
         VALUES ('other', X'02', 'Other', 1, 'registered', 'partial');"
    ).await.unwrap();
    assert_readiness(&mut reader, false).await;
    writer.connection.execute_unprepared("UPDATE source_roots SET enabled = 0 WHERE root_id = 'other'").await.unwrap();
    assert_readiness(&mut reader, true).await;
}
