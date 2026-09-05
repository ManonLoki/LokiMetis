use std::fs;
use std::path::{Path, PathBuf};

use loki_metis_core::CoverageState;
use tempfile::tempdir;

// 是 discovery/quick/tests.rs 与 discovery/full_device/tests.rs 两份
// Codex 测试思路针对 Claude Code 客户端的镜像版本，验证只认可
// `projects/<project>/<session>.jsonl` 结构签名这条 Claude 专属规则。
use super::*;

// Claude session 文件名必须是合法 UUID（is_uuid_jsonl_name 的硬性要求），
// 固定用一个语义清晰的全零 UUID 占位，比每次随机生成更便于在失败时
// 从测试输出里辨认。
/// 全部夹具共用的固定合法 session UUID。
const SESSION_ID: &str = "00000000-0000-0000-0000-000000000001";

/// 验证默认、环境与已登记根同时命中签名时，各自只贡献一次确认结果。
#[test]
fn quick_discovery_accepts_default_environment_and_registered_signature_once() {
    let temp = tempdir().expect("temporary home is available");
    let root = temp.path().join(".claude");
    write_transcript(&root.join(format!("projects/project-a/{SESSION_ID}.jsonl")));

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            home_dir: Some(temp.path().to_path_buf()),
            claude_config_dir: Some(root.clone()),
            registered_roots: vec![RegisteredRoot {
                root_id: Some("claude-known".to_owned()),
                path: root,
                alias: "Claude 测试根".to_owned(),
                enabled: true,
            }],
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(result.roots[0].root_id, "claude-known");
    assert_eq!(result.roots[0].alias, "Claude 测试根");
}

/// 验证未登记的默认 `~/.claude` 根命中签名后也能被自动确认。
#[test]
fn quick_discovery_accepts_unregistered_default_root() {
    let temp = tempdir().expect("temporary home is available");
    let root = temp.path().join(".claude");
    write_transcript(&root.join(format!("projects/project-a/{SESSION_ID}.jsonl")));

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            home_dir: Some(temp.path().to_path_buf()),
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].discovery_method,
        DiscoveryMethod::DefaultHome
    );
    assert_eq!(result.roots[0].alias, ".claude");
}

/// 验证未登记的环境变量指定根命中签名后也能被自动确认。
#[test]
fn quick_discovery_accepts_unregistered_environment_root() {
    let temp = tempdir().expect("temporary root is available");
    let root = temp.path().join("custom-claude-home");
    write_transcript(&root.join(format!("projects/project-a/{SESSION_ID}.jsonl")));

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            claude_config_dir: Some(root),
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].discovery_method,
        DiscoveryMethod::Environment
    );
    assert_eq!(result.roots[0].alias, "custom-claude-home");
}

/// 验证默认根不存在时不会拖累环境根的覆盖状态与确认结果。
#[test]
fn missing_optional_default_does_not_degrade_environment_root() {
    let temp = tempdir().expect("temporary roots are available");
    let environment_root = temp.path().join("custom-claude-home");
    write_transcript(&environment_root.join(format!("projects/project-a/{SESSION_ID}.jsonl")));

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            home_dir: Some(temp.path().join("home-without-claude")),
            claude_config_dir: Some(environment_root),
            registered_roots: Vec::new(),
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Complete);
    assert_eq!(result.coverage.skipped_count, 0);
    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].discovery_method,
        DiscoveryMethod::Environment
    );
}

/// 验证默认根存在但未通过结构签名时不会拖累环境根的覆盖状态与确认结果。
#[test]
fn unsigned_optional_default_does_not_degrade_environment_root() {
    let temp = tempdir().expect("temporary roots are available");
    let default_transcript = temp.path().join(format!(
        "home/.claude/projects/project-a/{SESSION_ID}.jsonl"
    ));
    fs::create_dir_all(
        default_transcript
            .parent()
            .expect("default transcript has a parent"),
    )
    .expect("default project is created");
    fs::write(default_transcript, b"{\"type\":\"assistant\"}\n")
        .expect("unsigned default fixture is written");
    let environment_root = temp.path().join("custom-claude-home");
    write_transcript(&environment_root.join(format!("projects/project-a/{SESSION_ID}.jsonl")));

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            home_dir: Some(temp.path().join("home")),
            claude_config_dir: Some(environment_root),
            registered_roots: Vec::new(),
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Complete);
    assert_eq!(result.coverage.skipped_count, 0);
    assert_eq!(result.roots.len(), 1);
}

/// 验证候选生成顺序固定为：已登记根、环境根、默认根。
#[test]
fn quick_candidates_prioritize_registered_then_environment_and_default() {
    let inputs = ClaudeDiscoveryInputs {
        home_dir: Some(PathBuf::from("/synthetic/home")),
        claude_config_dir: Some(PathBuf::from("/synthetic/environment")),
        registered_roots: vec![RegisteredRoot {
            root_id: Some("claude-registered".to_owned()),
            path: PathBuf::from("/synthetic/registered"),
            alias: "用户别名".to_owned(),
            enabled: true,
        }],
    };

    let candidates = quick_candidates(&inputs);

    assert_eq!(candidates.len(), 3);
    assert_eq!(candidates[0].method, DiscoveryMethod::Registered);
    assert_eq!(
        candidates[0].existing_root_id.as_deref(),
        Some("claude-registered")
    );
    assert_eq!(candidates[1].method, DiscoveryMethod::Environment);
    assert_eq!(candidates[2].method, DiscoveryMethod::DefaultHome);
}

/// 验证自动候选耗尽签名预算时不会波及已登记根的正常确认。
#[test]
fn automatic_candidate_budget_exhaustion_keeps_registered_root() {
    let temp = tempdir().expect("temporary roots are available");
    let registered = temp.path().join("registered");
    let exhausting = temp.path().join("exhausting");
    write_transcript(&registered.join(format!("projects/project-a/{SESSION_ID}.jsonl")));
    fs::create_dir_all(exhausting.join("projects/project-b")).expect("budget fixture is created");
    let inputs = ClaudeDiscoveryInputs {
        claude_config_dir: Some(exhausting),
        registered_roots: vec![RegisteredRoot {
            root_id: Some("claude-priority".to_owned()),
            path: registered,
            alias: "优先登记根".to_owned(),
            enabled: true,
        }],
        ..ClaudeDiscoveryInputs::default()
    };
    let options = FullDiscoveryOptions {
        max_signature_directories: 2,
        ..FullDiscoveryOptions::default()
    };

    let result = inspect_candidates(
        quick_candidates(&inputs),
        &CancellationToken::new(),
        &options,
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert_eq!(result.roots.len(), 1);
    assert_eq!(result.roots[0].root_id, "claude-priority");
    assert!(
        !result
            .unconfirmed_root_ids
            .contains(&"claude-priority".to_owned())
    );
}

/// 验证 subagent 目录布局下的 transcript 也能被识别并保留其证据类型。
#[test]
fn quick_discovery_accepts_subagent_layout() {
    let temp = tempdir().expect("temporary root is available");
    let root = temp.path().join("custom-claude");
    write_transcript(&root.join(format!(
        "projects/project-a/{SESSION_ID}/subagents/agent-synthetic.jsonl"
    )));

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("claude-subagent".to_owned()),
                path: root,
                alias: "Claude 子代理根".to_owned(),
                enabled: true,
            }],
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].evidence,
        loki_metis_core::RootCandidateEvidence::ClaudeSubagent
    );
}

/// 验证 subagent 目录下不符合 `agent-*.jsonl` 命名的文件不被采纳为证据。
#[test]
fn quick_discovery_rejects_non_agent_subagent_filename() {
    let temp = tempdir().expect("temporary root is available");
    let root = temp.path().join("custom-claude");
    write_transcript(&root.join(format!(
        "projects/project-a/{SESSION_ID}/subagents/ordinary.jsonl"
    )));

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("ordinary-subagent".to_owned()),
                path: root,
                alias: "无效子代理根".to_owned(),
                enabled: true,
            }],
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert_eq!(result.confirmed_invalid_root_ids, ["ordinary-subagent"]);
}

/// 验证辅助目录（logs/sessions/cache 等）与未签名目录一律被拒绝确认。
#[test]
fn quick_discovery_rejects_auxiliary_and_unsigned_directories() {
    let temp = tempdir().expect("temporary root is available");
    let candidates = ["logs", "sessions", "cache", "empty", "ordinary"];
    fs::create_dir_all(temp.path().join("logs")).expect("logs directory is created");
    fs::write(temp.path().join("logs/debug.jsonl"), b"{}\n").expect("log bait is written");
    fs::create_dir_all(temp.path().join("sessions")).expect("sessions directory is created");
    fs::create_dir_all(temp.path().join("cache")).expect("cache directory is created");
    fs::create_dir_all(temp.path().join("empty/projects")).expect("empty projects is created");
    let ordinary = temp
        .path()
        .join(format!("ordinary/projects/project-a/{SESSION_ID}.jsonl"));
    fs::create_dir_all(ordinary.parent().expect("ordinary fixture has parent"))
        .expect("ordinary parent is created");
    fs::write(ordinary, b"{\"type\":\"assistant\",\"message\":{}}\n")
        .expect("ordinary transcript bait is written");

    for candidate in candidates {
        let result = discover_claude_quick(
            &ClaudeDiscoveryInputs {
                registered_roots: vec![RegisteredRoot {
                    root_id: Some(format!("invalid-{candidate}")),
                    path: temp.path().join(candidate),
                    alias: "无效目录".to_owned(),
                    enabled: true,
                }],
                ..ClaudeDiscoveryInputs::default()
            },
            &CancellationToken::new(),
        );
        assert!(result.roots.is_empty(), "{candidate} must be rejected");
        assert_eq!(result.confirmed_invalid_root_ids.len(), 1);
    }
}

/// 验证 Claude 快速扫描不读取网络挂载候选，并确认登记根违反本地卷策略。
#[test]
fn quick_discovery_rejects_registered_network_root() {
    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("claude-network".to_owned()),
                path: synthetic_network_path().join(".claude"),
                alias: "网络根".to_owned(),
                enabled: true,
            }],
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert_eq!(result.confirmed_invalid_root_ids, ["claude-network"]);
    assert_eq!(result.network_skipped_count, 1);
    assert_eq!(result.coverage.state, CoverageState::Partial);
}

/// 验证全设备发现在命中根后停止下钻，不会把根内嵌套 logs 目录当成新根。
#[test]
fn full_discovery_stops_at_root_and_never_registers_nested_logs() {
    let temp = tempdir().expect("synthetic volume is available");
    let root = temp.path().join("profile/.claude");
    write_transcript(&root.join(format!("projects/project-a/{SESSION_ID}.jsonl")));
    write_transcript(
        &root.join("logs/projects/project-b/00000000-0000-0000-0000-000000000002.jsonl"),
    );

    let result = discover_claude_full_device_with_progress(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
        |_| {},
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].path,
        fs::canonicalize(root).expect("Claude root canonicalizes")
    );
}

/// 验证取消信号命中时，尚未探测的已登记根既不确认也不判定为无效。
#[test]
fn signature_budget_and_cancellation_never_confirm_uninspected_registered_roots() {
    let temp = tempdir().expect("temporary root is available");
    let root = temp.path().join(".claude");
    write_transcript(&root.join(format!("projects/project-a/{SESSION_ID}.jsonl")));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let cancelled = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("remembered".to_owned()),
                path: root,
                alias: "历史根".to_owned(),
                enabled: true,
            }],
            ..ClaudeDiscoveryInputs::default()
        },
        &cancellation,
    );
    assert_eq!(cancelled.coverage.state, CoverageState::Cancelled);
    assert!(cancelled.confirmed_invalid_root_ids.is_empty());
}

/// 验证候选根内的 `projects` 是符号链接时被整体拒绝，不跟随链接探测。
#[cfg(unix)]
#[test]
fn quick_discovery_rejects_linked_projects_directory() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("temporary root is available");
    let target = temp.path().join("real-projects");
    write_transcript(&target.join(format!("project-a/{SESSION_ID}.jsonl")));
    let root = temp.path().join(".claude");
    fs::create_dir_all(&root).expect("candidate root is created");
    symlink(&target, root.join("projects")).expect("projects symlink is created");

    let result = discover_claude_quick(
        &ClaudeDiscoveryInputs {
            home_dir: Some(temp.path().to_path_buf()),
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );
    assert!(result.roots.is_empty());
}

/// 测试辅助：写入一份最小合法的 Claude transcript 用量夹具文件。
fn write_transcript(path: &Path) {
    fs::create_dir_all(path.parent().expect("fixture has parent"))
        .expect("transcript parent is created");
    fs::write(
        path,
        format!(
            "{{\"type\":\"user\",\"sessionId\":\"{SESSION_ID}\",\"timestamp\":\"2026-07-31T08:00:00Z\"}}\n{{\"type\":\"assistant\",\"sessionId\":\"{SESSION_ID}\",\"timestamp\":\"2026-07-31T08:00:01Z\",\"message\":{{\"id\":\"msg_synthetic\",\"model\":\"claude-synthetic\",\"content\":\"privacy bait\",\"usage\":{{\"input_tokens\":10,\"cache_read_input_tokens\":4,\"cache_creation_input_tokens\":2,\"output_tokens\":3}}}}}}\n"
        ),
    )
    .expect("transcript fixture is written");
}

/// 构造一个跨平台可用的合成网络路径，供拒绝网络位置的测试复用。
fn synthetic_network_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(r"\\server\share")
    }
    #[cfg(not(target_os = "windows"))]
    {
        PathBuf::from("/net/server/share")
    }
}
