//! 验证本机性能证据通道的路径、权限、载荷和顺序拒绝边界。

use super::*;

/// 创建符合生产证据通道私有父目录要求的临时目录。
fn private_tempdir() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp root");
    #[cfg(unix)]
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
        .expect("private temp permissions");
    root
}

/// 构造最小合法的 renderer 能力指标。
fn renderer_metric() -> PerformanceEvidenceMetric {
    PerformanceEvidenceMetric::RendererCapabilities {
        long_task_supported: true,
        timing_source: RendererTimingSource::PerformanceObserver,
    }
}

/// 构造最小合法的主窗口就绪指标。
fn main_window_ready_metric() -> PerformanceEvidenceMetric {
    PerformanceEvidenceMetric::MainWindowReady {
        wall_time_ms: 1_800_000_000_000.0,
        monotonic_time_ms: 128.0,
    }
}

/// 默认状态不创建文件且拒绝写入。
#[test]
fn disabled_state_rejects_all_writes() {
    let state = PerformanceEvidenceState::disabled();
    assert!(!state.is_enabled());
    assert_eq!(
        state.record(renderer_metric()),
        Err(PerformanceEvidenceError::Disabled)
    );
}

/// 合法临时目标只输出固定 JSONL 字段，并持续执行递增序号约束。
#[test]
fn valid_temporary_target_writes_privacy_safe_jsonl() {
    let root = private_tempdir();
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target.clone(), root.path().to_path_buf())
        .expect("enabled state");

    assert_eq!(state.record(renderer_metric()).expect("first metric"), 1);
    assert_eq!(
        state
            .record(PerformanceEvidenceMetric::Interaction {
                target: "navigation-dashboard".to_string(),
                result: "dashboard-page".to_string(),
                duration_ms: 32.5,
            })
            .expect("interaction metric"),
        2
    );
    assert_eq!(
        state
            .record(PerformanceEvidenceMetric::MainWindowReady {
                wall_time_ms: 1_800_000_000_000.0,
                monotonic_time_ms: 128.0,
            })
            .expect("ready metric"),
        3
    );
    assert_eq!(
        state
            .record(PerformanceEvidenceMetric::RendererBlockingInterval {
                timing_source: RendererTimingSource::PerformanceObserver,
                start_time_ms: 64.0,
                duration_ms: 75.0,
            })
            .expect("blocking metric"),
        4
    );

    let lines = fs::read_to_string(&target).expect("read evidence");
    let parsed = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json line"))
        .collect::<Vec<_>>();
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0]["kind"], "renderer-capabilities");
    assert_eq!(parsed[0]["sequence"], 1);
    assert_eq!(parsed[0]["longTaskSupported"], true);
    assert_eq!(parsed[1]["target"], "navigation-dashboard");
    assert_eq!(parsed[1]["result"], "dashboard-page");
    assert_eq!(parsed[2]["kind"], "main-window-ready");
    assert_eq!(parsed[3]["kind"], "renderer-blocking-interval");
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
        "target": "navigation-dashboard",
        "result": "dashboard-page",
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
    let root = private_tempdir();
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

    let root = private_tempdir();
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
    let root = private_tempdir();
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target, root.path().to_path_buf())
        .expect("enabled state");
    state
        .record(renderer_metric())
        .expect("renderer capabilities");

    assert_eq!(
        state.record(PerformanceEvidenceMetric::Interaction {
            target: "private-page-title".to_string(),
            result: "dashboard-page".to_string(),
            duration_ms: 1.0,
        }),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
    assert_eq!(
        state.record(PerformanceEvidenceMetric::Interaction {
            target: "navigation-dashboard".to_string(),
            result: "settings-page".to_string(),
            duration_ms: 1.0,
        }),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
    assert_eq!(
        state.record(PerformanceEvidenceMetric::RendererBlockingInterval {
            timing_source: RendererTimingSource::PerformanceObserver,
            start_time_ms: 4.0,
            duration_ms: 49.9,
        }),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
    assert_eq!(
        state.record(PerformanceEvidenceMetric::MainWindowReady {
            wall_time_ms: f64::NAN,
            monotonic_time_ms: 4.0,
        }),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
}

/// Rust 统一分配序号，结束握手幂等同步且结束后拒绝新指标。
#[test]
fn rust_assigns_sequence_and_finalization_is_idempotent() {
    let root = private_tempdir();
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target.clone(), root.path().to_path_buf())
        .expect("enabled state");
    assert_eq!(state.record(renderer_metric()).expect("first metric"), 1);
    assert_eq!(
        state
            .record(main_window_ready_metric())
            .expect("ready metric"),
        2
    );
    let first = state.finish().expect("first finalization");
    let second = state.finish().expect("idempotent finalization");
    assert_eq!(first, second);
    assert_eq!(first.final_sequence, 3);
    assert_eq!(first.record_count, 2);
    assert_eq!(
        state.record(renderer_metric()),
        Err(PerformanceEvidenceError::Finalized)
    );
    let lines = fs::read_to_string(target).expect("finalized evidence");
    assert_eq!(lines.lines().count(), 3);
    assert!(lines.contains("\"kind\":\"session-finalized\""));
}

/// renderer capabilities 必须且只能作为首条记录出现。
#[test]
fn duplicate_renderer_capabilities_are_rejected() {
    let root = private_tempdir();
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target.clone(), root.path().to_path_buf())
        .expect("enabled state");

    assert_eq!(state.record(renderer_metric()).expect("first metric"), 1);
    assert_eq!(
        state.record(renderer_metric()),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
    assert_eq!(
        fs::read_to_string(target)
            .expect("capabilities evidence")
            .lines()
            .count(),
        1
    );
}

/// 主窗口就绪记录在同一会话中只能成功落盘一次。
#[test]
fn duplicate_main_window_ready_is_rejected() {
    let root = private_tempdir();
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target.clone(), root.path().to_path_buf())
        .expect("enabled state");

    state
        .record(renderer_metric())
        .expect("renderer capabilities");
    assert_eq!(
        state
            .record(main_window_ready_metric())
            .expect("first ready metric"),
        2
    );
    assert_eq!(
        state.record(main_window_ready_metric()),
        Err(PerformanceEvidenceError::InvalidMetric)
    );
    assert_eq!(
        fs::read_to_string(target)
            .expect("ready evidence")
            .lines()
            .count(),
        2
    );
}

/// 未确认主窗口就绪时不得写入会话结束记录。
#[test]
fn finalization_without_main_window_ready_is_rejected() {
    let root = private_tempdir();
    let target = root.path().join("performance.jsonl");
    let state = PerformanceEvidenceState::from_path(target.clone(), root.path().to_path_buf())
        .expect("enabled state");

    state
        .record(renderer_metric())
        .expect("renderer capabilities");
    assert_eq!(state.finish(), Err(PerformanceEvidenceError::InvalidMetric));
    let evidence = fs::read_to_string(target).expect("unfinished evidence");
    assert_eq!(evidence.lines().count(), 1);
    assert!(!evidence.contains("session-finalized"));
}

/// Unix 测试证据父目录必须为私有 0700，避免其他用户替换已校验路径。
#[cfg(unix)]
#[test]
fn permissive_parent_directory_is_rejected() {
    let root = tempfile::tempdir().expect("temp root");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755))
        .expect("set permissive mode");
    let result = PerformanceEvidenceState::from_path(
        root.path().join("performance.jsonl"),
        root.path().to_path_buf(),
    );
    assert!(matches!(result, Err(PerformanceEvidenceError::InvalidPath)));
}
