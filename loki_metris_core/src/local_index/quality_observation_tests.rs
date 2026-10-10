//! 验证批量投影保留旧版和未提交来源，私有格式仅由 adapter 回调解释。

use sea_orm::ConnectionTrait;

use super::LocalIndex;
use crate::PersistedSourceQuality;

/// 一次查询返回全部状态与内容无关质量，不能因旧版本或占位而丢失修复入口。
#[tokio::test]
async fn quality_observations_include_old_and_uncommitted_generations() {
    let temp = tempfile::tempdir().expect("isolated app data");
    let index = LocalIndex::open_in_app_data(temp.path(), 9).await.expect("index opens");
    index.connection.execute_unprepared(
        "INSERT INTO source_roots (root_id, access_path, alias, enabled, discovery_method)
         VALUES ('root', X'01', 'Root', 1, 'registered');
         INSERT INTO source_files
           (source_id, root_id, relative_label, file_identity, archived, observed_size,
            modified_at_epoch_ms, parsed_offset, trailing_bytes, oversized_tail,
            parser_version, generation, ready, thread_key, call_sequence,
            token_snapshots_ready, adapter_state)
         VALUES
           ('old', 'root', 'sessions/old.jsonl', 'old', 0, 10, 400, 10, 0, 0, 8, 1, 1, 'old', 1, 1, 'quality-7'),
           ('current', 'root', 'sessions/current.jsonl', 'current', 0, 20, 500, 20, 0, 0, 9, 1, 1, 'current', 1, 1, 'quality-0'),
           ('pending', 'root', 'sessions/pending.jsonl', 'pending', 0, 0, 0, 0, 0, 0, 9, 0, 0, 'pending', 0, 0, NULL),
           ('unread', 'root', 'sessions/unread.jsonl', 'unread', 0, 100, 700, 50, 0, 0, 9, 1, 1, 'unread', 1, 1, 'quality-0'),
           ('tail', 'root', 'sessions/tail.jsonl', 'tail', 0, 100, 800, 90, 10, 0, 9, 1, 1, 'tail', 1, 1, 'quality-0'),
           ('archived', 'root', 'archived_sessions/other.jsonl', 'archived', 1, 30, 600, 30, 0, 0, 9, 1, 1, 'archived', 1, 1, 'quality-0');"
    ).await.expect("isolated source facts inserted");
    let observations = index.source_file_observations_for_root_with_quality("root", false, |state| {
        match state {
            Some("quality-7") => PersistedSourceQuality::Known { warning_count: 7 },
            Some("quality-0") => PersistedSourceQuality::Known { warning_count: 0 },
            _ => PersistedSourceQuality::Unknown,
        }
    }).await.expect("quality projection loads");
    assert_eq!(observations.len(), 5);
    let old = observations["sessions/old.jsonl"];
    assert!(!old.parser_compatible);
    assert!(old.checkpoint_complete);
    assert_eq!(old.persisted_quality, Some(PersistedSourceQuality::Known { warning_count: 7 }));
    let current = observations["sessions/current.jsonl"];
    assert_eq!((current.observed_size, current.modified_at_epoch_ms), (20, 500));
    assert!(current.parser_compatible && current.checkpoint_complete);
    let pending = observations["sessions/pending.jsonl"];
    assert!(pending.parser_compatible);
    assert!(!pending.checkpoint_complete);
    assert_eq!(pending.persisted_quality, Some(PersistedSourceQuality::Unknown));
    assert!(!observations["sessions/unread.jsonl"].checkpoint_complete);
    assert!(observations["sessions/tail.jsonl"].checkpoint_complete);
    let legacy = index.source_file_observations_for_root("root", false).await.expect("legacy adapter projection");
    assert_eq!(legacy.len(), 5);
    assert!(legacy.values().all(|observation| observation.persisted_quality.is_none()));
}
