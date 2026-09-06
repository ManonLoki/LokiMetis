//! 验证本机性能证据通道的路径、权限、载荷和顺序拒绝边界。

use super::*;

/// 构造最小合法的 renderer 能力指标。
fn renderer_metric(sequence: u64) -> PerformanceEvidenceMetric {
    PerformanceEvidenceMetric::RendererCapabilities {
        sequence,
        long_task_supported: true,
    }
}

/// 默认状态不创建文件且拒绝写入。
#[test]
fn disabled_state_rejects_all_writes() {
    let state = PerformanceEvidenceState::disabled();
    assert!(!state.is_enabled());
    assert_eq!(
        state.record(renderer_metric(1)),
        Err(PerformanceEvidenceError::Disabled)
    );
}

/// 合法临时目标只输出固定 JSONL 字段，并持续执行递增序号约束。
#[test]
fn valid_temporary_target_writes_privacy_safe_jsonl() {
    let root = tempfile::tempdir().expect("temp root");
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target.clone(), root.path().to_path_buf())
        .expect("enabled state");

    state.record(renderer_metric(1)).expect("first metric");
    state
        .record(PerformanceEvidenceMetric::Interaction {
            sequence: 2,
            target: "navigation-label-dashboard".to_string(),
            duration_ms: 32.5,
        })
        .expect("interaction metric");
    state
        .record(PerformanceEvidenceMetric::MainWindowReady {
            sequence: 3,
            wall_time_ms: 1_800_000_000_000.0,
            monotonic_time_ms: 128.0,
        })
        .expect("ready metric");
    state
        .record(PerformanceEvidenceMetric::LongTask {
            sequence: 4,
            start_time_ms: 64.0,
            duration_ms: 75.0,
        })
        .expect("long task metric");

    let lines = fs::read_to_string(&target).expect("read evidence");
    let parsed = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0]["kind"], "renderer-capabilities");
    assert_eq!(parsed[0]["sequence"], 1);
    assert_eq!(parsed[0]["longTaskSupported"], true);
    assert_eq!(parsed[1]["target"], "navigation-label-dashboard");
    assert_eq!(parsed[2]["kind"], "main-window-ready");
    assert_eq!(parsed[3]["kind"], "long-task");
    assert!(!lines.contains(root.path().to_string_lossy().as_ref()));
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&target)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

/// serde 边界拒绝额外字段，防止前端把非指标数据混入证据文件。
#[test]
fn payload_with_unknown_fields_is_rejected_during_deserialization() {
    let payload = serde_json::json!({
        "kind": "interaction",
        "sequence": 1,
        "target": "generic",
        "durationMs": 12.0,
        "pageTitle": "must not be accepted"
    });
    assert!(serde_json::from_value::<PerformanceEvidenceMetric>(payload).is_err());
}

/// 目标必须位于声明的系统临时目录之内。
#[test]
fn target_outside_system_temporary_directory_is_rejected() {
    let allowed = tempfile::tempdir().expect("allowed root");
    let outside = tempfile::tempdir().expect("outside root");
    let result = PerformanceEvidenceState::from_path(
        outside.path().join("performance.jsonl"),
        allowed.path().to_path_buf(),
    );
    assert!(matches!(result, Err(PerformanceEvidenceError::InvalidPath)));
}

/// 已存在的普通文件不能被复用或覆盖。
#[test]
fn existing_target_is_rejected_without_overwrite() {
    let root = tempfile::tempdir().expect("temp root");
    let target = root.path().join("performance.jsonl");
    fs::write(&target, "keep").expect("seed target");
    let result = PerformanceEvidenceState::from_path(target.clone(), root.path().to_path_buf());
    assert!(matches!(result, Err(PerformanceEvidenceError::InvalidPath)));
    assert_eq!(fs::read_to_string(target).expect("original target"), "keep");
}

/// 符号链接目标及符号链接父目录都必须被拒绝。
#[cfg(unix)]
#[test]
fn symbolic_link_target_and_parent_are_rejected() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temp root");
    let real_parent = root.path().join("real");
    fs::create_dir(&real_parent).expect("real parent");
    let existing = real_parent.join("existing.jsonl");
    fs::write(&existing, "keep").expect("seed target");
    let target_link = root.path().join("target-link.jsonl");
    symlink(&existing, &target_link).expect("target link");
    assert!(matches!(
        PerformanceEvidenceState::from_path(target_link, root.path().to_path_buf()),
        Err(PerformanceEvidenceError::InvalidPath)
    ));

    let parent_link = root.path().join("parent-link");
    symlink(&real_parent, &parent_link).expect("parent link");
    assert!(matches!(
        PerformanceEvidenceState::from_path(
            parent_link.join("new.jsonl"),
            root.path().to_path_buf()
        ),
        Err(PerformanceEvidenceError::InvalidPath)
    ));
}

/// 非有限值、越界耗时和非白名单目标都不能进入证据文件。
#[test]
fn invalid_numeric_ranges_and_targets_are_rejected() {
    let root = tempfile::tempdir().expect("temp root");
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target, root.path().to_path_buf())
        .expect("enabled state");

    assert_eq!(
        state.record(PerformanceEvidenceMetric::Interaction {
            sequence: 1,
            target: "private-page-title".to_string(),
            duration_ms: 1.0,
        }),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
    assert_eq!(
        state.record(PerformanceEvidenceMetric::LongTask {
            sequence: 1,
            start_time_ms: 4.0,
            duration_ms: 49.9,
        }),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
    assert_eq!(
        state.record(PerformanceEvidenceMetric::MainWindowReady {
            sequence: 1,
            wall_time_ms: f64::NAN,
            monotonic_time_ms: 4.0,
        }),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
}

/// 成功落盘后拒绝重复或倒退序号，避免并发 IPC 伪造事件顺序。
#[test]
fn non_increasing_sequence_is_rejected() {
    let root = tempfile::tempdir().expect("temp root");
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target, root.path().to_path_buf())
        .expect("enabled state");
    state.record(renderer_metric(2)).expect("first metric");
    assert_eq!(
        state.record(renderer_metric(2)),
        Err(PerformanceEvidenceError::SequenceRejected)
    );
    assert_eq!(
        state.record(renderer_metric(1)),
        Err(PerformanceEvidenceError::SequenceRejected)
    );
}
