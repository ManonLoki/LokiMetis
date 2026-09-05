// 本文件覆盖数据根从登记到移除的完整生命周期：清空索引、停用/重新
// 启用、重命名、设为/清除主目录、确认失效后自动退出索引等场景。
// 几乎每个用例都遵循同一个断言习惯——操作完成后既检查“预期发生的
// 变化确实发生了”，也检查“不该变的东西（比如原始 rollout 文件、
// 未涉及的其他数据根）确实没有被动到”，呼应产品“绝不修改原始客户端
// 文件”的边界承诺。
use loki_metis_core::{
    Completeness, Confidence, Freshness, LocalIndexState, MetricFact, MetricScope, ProviderKind,
};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{create_root, discover_registered, scan, token_line, write_rollout};
use crate::backend::local_index::{
    CancellationToken, ClaudeDiscoveredRoot, DiscoveryMethod, LocalIndex, PARSER_VERSION,
    RegisterDiscoveredRoot,
};

/// 在隔离 app-data 内按测试固定 parser 版本打开索引。
fn open_index(app_data_dir: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(app_data_dir, PARSER_VERSION)).expect("index opens")
}

/// 验证清空只删除本产品派生索引，保留数据根登记与原始 rollout。
#[test]
fn clear_index_preserves_registry_and_original_rollout() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-clear.jsonl");
    write_rollout(
        &rollout,
        "session-clear",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    assert!(
        block_on(index.set_primary_root(Some(&roots[0].root_id))).expect("root becomes primary")
    );

    block_on(index.clear_index()).expect("product index is cleared");

    assert_eq!(
        block_on(index.aggregate())
            .expect("empty aggregate loads")
            .call_count,
        0
    );
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("cleared snapshot loads")
            .index_state,
        LocalIndexState::NotScanned
    );
    assert_eq!(
        block_on(index.list_sources())
            .expect("registry remains")
            .len(),
        1
    );
    assert!(block_on(index.list_sources()).expect("primary remains")[0].is_primary);
    assert_eq!(
        block_on(index.primary_root())
            .expect("primary root loads")
            .and_then(|root| root.root_id),
        Some(roots[0].root_id.clone())
    );
    assert!(
        rollout.exists(),
        "original Codex rollout must remain untouched"
    );
}

/// 验证数据根可停用并从本产品索引移除，整个过程不修改原始 rollout。
#[test]
fn disables_and_removes_only_product_root_records() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-root-management.jsonl");
    write_rollout(
        &rollout,
        "session-root-management",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let root_id = roots[0].root_id.clone();
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());

    assert!(
        block_on(index.set_root_enabled(&root_id, false)).expect("registered root is disabled")
    );
    assert!(
        block_on(index.known_roots())
            .expect("known roots load")
            .is_empty()
    );
    assert_eq!(
        block_on(index.aggregate())
            .expect("disabled aggregate loads")
            .call_count,
        0
    );
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("disabled snapshot loads")
            .index_state,
        LocalIndexState::NotScanned
    );
    assert!(rollout.exists(), "disabling never changes the source file");

    assert!(
        block_on(index.set_root_alias(&root_id, "重命名数据根"))
            .expect("registered root alias is updated")
    );
    assert_eq!(
        block_on(index.list_sources()).expect("renamed root loads")[0].alias,
        "重命名数据根"
    );
    block_on(index.register_root(&roots[0])).expect("rediscovery preserves registry choices");
    let rediscovered = block_on(index.list_sources()).expect("rediscovered root loads");
    assert!(!rediscovered[0].enabled);
    assert_eq!(rediscovered[0].alias, "重命名数据根");
    assert_eq!(
        block_on(index.aggregate())
            .expect("rediscovered disabled aggregate loads")
            .call_count,
        0
    );

    assert!(
        block_on(index.set_root_enabled(&root_id, true)).expect("registered root is re-enabled")
    );
    assert_eq!(
        block_on(index.aggregate())
            .expect("restored aggregate loads")
            .call_count,
        1
    );

    assert!(block_on(index.remove_root(&root_id)).expect("registered root is removed"));
    assert_eq!(
        block_on(index.aggregate())
            .expect("aggregate reloads")
            .call_count,
        0
    );
    assert!(
        block_on(index.list_sources())
            .expect("registry reloads")
            .is_empty()
    );
    assert!(rollout.exists(), "removing the index never deletes rollout");
}

/// 验证主数据根只能有一个，切换、停用、移除与路径重绑都会原子失效官方快照。
#[test]
fn primary_root_lifecycle_is_unique_and_invalidates_provider_snapshots() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let first_path = create_root(source_temp.path(), "first");
    let second_path = create_root(source_temp.path(), "second");
    write_rollout(
        &first_path.join("sessions/rollout-first.jsonl"),
        "session-first",
        &[token_line("2026-07-30T10:01:00Z", "call-first", 10, 2)],
    );
    write_rollout(
        &second_path.join("sessions/rollout-second.jsonl"),
        "session-second",
        &[token_line("2026-07-30T10:02:00Z", "call-second", 20, 4)],
    );
    let roots = discover_registered(&[first_path, second_path]);
    let first_id = roots[0].root_id.clone();
    let second_id = roots[1].root_id.clone();
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());

    assert!(block_on(index.set_primary_root(Some(&first_id))).expect("first root becomes primary"));
    assert!(
        !block_on(index.set_primary_root(Some(&first_id)))
            .expect("setting the same primary is idempotent")
    );
    assert_eq!(
        block_on(index.list_sources())
            .expect("sources load")
            .iter()
            .filter(|root| root.is_primary)
            .count(),
        1
    );

    seed_provider_snapshot(&mut index);
    assert!(block_on(index.set_primary_root(Some(&second_id))).expect("primary switches"));
    assert_eq!(provider_snapshot_count(&index), 0);
    assert_eq!(
        block_on(index.primary_root())
            .expect("primary loads")
            .and_then(|root| root.root_id),
        Some(second_id.clone())
    );

    seed_provider_snapshot(&mut index);
    assert!(
        block_on(index.set_root_enabled(&first_id, false)).expect("non-primary root is disabled")
    );
    assert_eq!(
        provider_snapshot_count(&index),
        1,
        "non-primary mutations must preserve the current account snapshot"
    );
    assert!(
        !block_on(index.set_primary_root(Some(&first_id)))
            .expect("disabled root cannot replace the current primary")
    );
    assert_eq!(
        block_on(index.primary_root())
            .expect("old primary is preserved")
            .and_then(|root| root.root_id),
        Some(second_id.clone())
    );

    assert!(block_on(index.set_root_enabled(&second_id, false)).expect("primary root is disabled"));
    assert!(
        block_on(index.primary_root())
            .expect("primary is cleared")
            .is_none()
    );
    assert_eq!(provider_snapshot_count(&index), 0);

    assert!(block_on(index.set_root_enabled(&second_id, true)).expect("second root is re-enabled"));
    assert!(
        block_on(index.set_primary_root(Some(&second_id)))
            .expect("second root becomes primary again")
    );
    seed_provider_snapshot(&mut index);
    assert_eq!(
        block_on(index.remove_roots(std::slice::from_ref(&second_id)))
            .expect("primary is removed in a batch"),
        1
    );
    assert!(
        block_on(index.primary_root())
            .expect("primary is cleared after batch removal")
            .is_none()
    );
    assert_eq!(provider_snapshot_count(&index), 0);
}

/// 验证已选主根被同一内部 ID 重绑到另一访问位置时不会静默继承账号上下文。
#[test]
fn primary_root_path_rebinding_clears_primary_and_provider_snapshot() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let first_path = create_root(source_temp.path(), "first");
    let rebound_path = create_root(source_temp.path(), "rebound");
    write_rollout(
        &first_path.join("sessions/rollout-first.jsonl"),
        "session-first",
        &[token_line("2026-07-30T10:01:00Z", "call-first", 10, 2)],
    );
    write_rollout(
        &rebound_path.join("sessions/rollout-rebound.jsonl"),
        "session-rebound",
        &[token_line("2026-07-30T10:02:00Z", "call-rebound", 20, 4)],
    );
    let roots = discover_registered(&[first_path, rebound_path]);
    let mut index = open_index(app_temp.path());
    block_on(index.register_root(&roots[0])).expect("first root registers");
    assert!(
        block_on(index.set_primary_root(Some(&roots[0].root_id)))
            .expect("first root becomes primary")
    );
    seed_provider_snapshot(&mut index);

    let mut rebound = roots[1].clone();
    rebound.root_id = roots[0].root_id.clone();
    block_on(index.register_root(&rebound)).expect("same internal id can be safely rebound");

    assert!(
        block_on(index.primary_root())
            .expect("primary is cleared")
            .is_none()
    );
    assert_eq!(provider_snapshot_count(&index), 0);
}

/// 验证同一物理 Codex 目录以不同词法路径再次登记时保持单行和用户现有选择。
#[test]
fn physical_codex_directory_registration_is_idempotent() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    write_rollout(
        &root_path.join("sessions/rollout-idempotent.jsonl"),
        "session-idempotent",
        &[token_line("2026-07-30T10:01:00Z", "call-idempotent", 10, 2)],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let mut index = open_index(app_temp.path());
    block_on(index.register_root(&roots[0])).expect("first root registers");
    block_on(index.set_root_alias(&roots[0].root_id, "用户保留别名")).expect("root alias changes");
    block_on(index.set_root_enabled(&roots[0].root_id, false)).expect("root is disabled");

    let mut duplicate = roots[0].clone();
    duplicate.root_id = "root-duplicate-physical-alias".to_owned();
    duplicate.path = root_path.join("..").join("root");
    duplicate.alias = "不得覆盖的别名".to_owned();
    let changed = block_on(index.register_root_if_new(&duplicate))
        .expect("physical identity comparison succeeds");
    let records = block_on(index.list_sources()).expect("source records load");

    assert!(!changed);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].root_id, roots[0].root_id);
    assert_eq!(records[0].alias, "用户保留别名");
    assert!(!records[0].enabled);
}

/// 验证扫描使用的普通登记只按稳定 ID upsert，不会因跨 ID 物理匹配而静默漏行。
#[test]
fn ordinary_scan_registration_keeps_distinct_root_ids() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "scan-root");
    write_rollout(
        &root_path.join("sessions/rollout-scan-id.jsonl"),
        "session-scan-id",
        &[token_line("2026-07-30T10:01:00Z", "call-scan-id", 10, 2)],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let mut second = roots[0].clone();
    second.root_id = "root-scan-second-identity".to_owned();
    second.path = root_path.join("..").join("scan-root");
    let mut index = open_index(app_temp.path());

    block_on(index.register_root(&roots[0])).expect("first ID registers");
    block_on(index.register_root(&second)).expect("ordinary scan registration keeps its own ID");

    let records = block_on(index.list_sources()).expect("source records load");
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .any(|record| record.root_id == roots[0].root_id)
    );
    assert!(
        records
            .iter()
            .any(|record| record.root_id == second.root_id)
    );
}

/// 验证签名完成后候选若在登记前消失，手动幂等事务会无写入失败而非记录陈旧路径。
#[test]
fn manual_registration_rejects_candidate_identity_failure() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "disappearing-root");
    write_rollout(
        &root_path.join("sessions/rollout-disappearing.jsonl"),
        "session-disappearing",
        &[token_line(
            "2026-07-30T10:01:00Z",
            "call-disappearing",
            10,
            2,
        )],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    std::fs::remove_dir_all(&root_path).expect("candidate disappears after signature validation");
    let mut index = open_index(app_temp.path());

    let error = block_on(index.register_root_if_new(&roots[0]))
        .expect_err("identity failure aborts manual registration");

    assert_eq!(
        error.kind(),
        crate::backend::local_index::LocalErrorKind::SourceUnavailable
    );
    assert!(
        block_on(index.list_sources())
            .expect("source records load")
            .is_empty()
    );
}

/// 验证历史路径后来变成指向新候选的链接时不能参与物理去重或保留不安全路径。
#[cfg(unix)]
#[test]
fn historical_symlink_path_cannot_suppress_a_safe_candidate() {
    use std::os::unix::fs::symlink;

    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let legacy_path = create_root(source_temp.path(), "legacy-root");
    let candidate_path = create_root(source_temp.path(), "safe-candidate");
    for (path, suffix) in [(&legacy_path, "legacy"), (&candidate_path, "candidate")] {
        write_rollout(
            &path.join(format!("sessions/rollout-{suffix}.jsonl")),
            &format!("session-{suffix}"),
            &[token_line(
                "2026-07-30T10:01:00Z",
                &format!("call-{suffix}"),
                10,
                2,
            )],
        );
    }
    let roots = discover_registered(&[legacy_path.clone(), candidate_path.clone()]);
    let canonical_legacy = std::fs::canonicalize(&legacy_path).expect("legacy path canonicalizes");
    let canonical_candidate =
        std::fs::canonicalize(&candidate_path).expect("candidate path canonicalizes");
    let legacy = roots
        .iter()
        .find(|root| root.path == canonical_legacy)
        .expect("legacy root is discovered")
        .clone();
    let candidate = roots
        .iter()
        .find(|root| root.path == canonical_candidate)
        .expect("candidate root is discovered")
        .clone();
    let mut index = open_index(app_temp.path());
    block_on(index.register_root(&legacy)).expect("legacy root registers");
    std::fs::remove_dir_all(&legacy_path).expect("legacy directory is removed");
    symlink(&candidate_path, &legacy_path).expect("historical path becomes a link");

    assert!(
        block_on(index.register_root_if_new(&candidate))
            .expect("unsafe historical path is ignored for dedupe")
    );
    let records = block_on(index.list_sources()).expect("source records load");
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .any(|record| record.root_id == candidate.root_id)
    );
}

/// 验证同一物理 Claude Code 目录也在独立客户端 registry 内幂等登记。
#[test]
fn physical_claude_directory_registration_is_idempotent() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "claude-root");
    let first = ClaudeDiscoveredRoot {
        path: root_path.clone(),
        root_id: "claude-root-first".to_owned(),
        alias: "Claude 用户别名".to_owned(),
        discovery_method: DiscoveryMethod::Registered,
        evidence: loki_metis_core::RootCandidateEvidence::ClaudeTranscript,
    };
    let mut duplicate = first.clone();
    duplicate.path = root_path.join("..").join("claude-root");
    duplicate.root_id = "claude-root-duplicate".to_owned();
    duplicate.alias = "不得覆盖的 Claude 别名".to_owned();
    let mut index = open_index(app_temp.path());
    block_on(index.register_claude_root(&first)).expect("first Claude root registers");

    let changed = block_on(index.register_claude_root_if_new(&duplicate))
        .expect("physical identity comparison succeeds");
    let records = block_on(index.list_sources()).expect("source records load");

    assert!(!changed);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].root_id, first.root_id);
    assert_eq!(records[0].alias, first.alias);
}

/// 验证主根重绑到字节级不同的路径（即使物理上是同一目录的词法别名）会
/// 保守清除主标记与官方快照。
// `register_root_fields`（core）判断“主根路径是否变了”只做字节级比较，
// 不再像旧实现那样识别“同一物理目录的不同词法路径”——那部分模糊比较
// 依赖平台卷分类，故意没有跟着存储层一起搬进 runtime-neutral 的 core，
// 留在 GUI adapter。方向仍然是安全的：宁可多清一次缓存，也不让旧账号
// 事实带着新路径继续展示。细节见
// `loki_metis_core::local_index::root_registry` 的模块文档。
#[test]
fn byte_different_primary_path_clears_primary_and_provider_snapshot() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "primary");
    write_rollout(
        &root_path.join("sessions/rollout-primary-alias.jsonl"),
        "session-primary-alias",
        &[token_line(
            "2026-07-30T10:01:00Z",
            "call-primary-alias",
            10,
            2,
        )],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let mut index = open_index(app_temp.path());
    block_on(index.register_root(&roots[0])).expect("primary root registers");
    block_on(index.set_primary_root(Some(&roots[0].root_id))).expect("root becomes primary");
    seed_provider_snapshot(&mut index);

    let mut alias = roots[0].clone();
    alias.path = root_path.join("..").join("primary");
    block_on(index.register_root(&alias)).expect("byte-different path re-registers safely");

    assert!(
        block_on(index.primary_root())
            .expect("primary root loads")
            .is_none()
    );
    assert_eq!(provider_snapshot_count(&index), 0);
}

/// 写入一条只含规范化本机字段的合成快照，供主根生命周期测试观察失效行为。
fn seed_provider_snapshot(index: &mut LocalIndex) {
    let fact = MetricFact::new(
        0_u64,
        ProviderKind::RolloutJsonl,
        MetricScope::DeviceObserved,
        1,
        Freshness::Fresh,
        Completeness::Complete,
        Confidence::Exact,
        None,
    );
    let normalized_json = serde_json::to_string(&fact).expect("local fact serializes");
    block_on(index.save_normalized_snapshot(1, 2, &normalized_json))
        .expect("provider snapshot is seeded");
}

/// 返回当前规范化 provider 快照数量。
fn provider_snapshot_count(index: &LocalIndex) -> u64 {
    block_on(index.provider_snapshot_count()).expect("provider snapshot count loads")
}
