// 本文件是 root_lifecycle_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证历史路径后来变成指向新候选的链接时不能参与物理去重或保留不安全路径。；验证同一物理 Claude Code 目录也在独立客户端 registry 内幂等登记。；验证主根重绑到字节级不同的路径（即使物理上是同一目录的词法别名）会

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
        evidence: loki_metris_core::RootCandidateEvidence::ClaudeTranscript,
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
// `loki_metris_core::local_index::root_registry` 的模块文档。
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
