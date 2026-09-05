use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::tempdir;

// 本文件测的是 discovery/quick.rs：快速发现只检查默认主目录、
// CODEX_HOME 环境变量、用户已登记根这三类固定候选，不做任何磁盘递归
// 遍历。
use super::*;
use crate::backend::local_index::{CancellationToken, RegisteredRoot};

/// 验证快速发现只检查三个明确候选，并按规范化路径去重。
#[test]
fn quick_discovery_finds_known_roots_without_recursive_search() {
    let temp = tempdir().expect("temporary root is available");
    let codex_root = temp.path().join(".codex");
    write_signature_rollout(&codex_root.join("sessions/rollout-known.jsonl"));
    // `auth.json` 这个文件本身内容无关紧要——它是特意放在候选目录里的
    // “诱饵”，用来确认发现流程压根不会去读取、解析或校验这个文件；
    // 本产品的边界之一就是永远不读 auth.json（不接触真实登录凭据）。
    File::create(codex_root.join("auth.json")).expect("privacy bait file is created");

    // 三类候选（home_dir 的默认 `.codex`、codex_home 环境变量、
    // registered_roots 用户登记）故意都指向同一个物理路径 codex_root——
    // 断言 `result.roots.len() == 1` 验证的正是"同一个物理目录不管从
    // 几个入口被发现，去重后也只会出现一次"。
    let result = discover_quick(
        &DiscoveryInputs {
            home_dir: Some(temp.path().to_path_buf()),
            codex_home: Some(codex_root.clone()),
            registered_roots: vec![RegisteredRoot {
                root_id: Some("root-known".to_owned()),
                path: codex_root,
                alias: "测试根".to_owned(),
                enabled: true,
            }],
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(result.roots[0].alias, "测试根");
    assert_eq!(result.roots[0].root_id, "root-known");
    assert_eq!(result.directories_scanned, 0);
}

/// 验证从未登记过的默认 `.codex` 也会进入首次与周期 Quick。
#[test]
fn quick_discovery_finds_unregistered_default_root() {
    let temp = tempdir().expect("temporary home is available");
    let root = temp.path().join(".codex");
    write_signature_rollout(&root.join("sessions/rollout-default.jsonl"));

    let result = discover_quick(
        &DiscoveryInputs {
            home_dir: Some(temp.path().to_path_buf()),
            ..DiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].discovery_method,
        DiscoveryMethod::DefaultHome
    );
    assert_eq!(result.roots[0].alias, ".codex");
}

/// 验证位于默认目录之外、仅由 `CODEX_HOME` 指定的根可被精确发现。
#[test]
fn quick_discovery_finds_unregistered_environment_root() {
    let temp = tempdir().expect("temporary root is available");
    let root = temp.path().join("custom-codex-home");
    write_signature_rollout(&root.join("sessions/rollout-environment.jsonl"));

    let result = discover_quick(
        &DiscoveryInputs {
            codex_home: Some(root),
            ..DiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].discovery_method,
        DiscoveryMethod::Environment
    );
    assert_eq!(result.roots[0].alias, "custom-codex-home");
    assert_eq!(result.directories_scanned, 0);
}

/// 可选默认根尚未创建时，不把有效环境根的正常发现误报为 Partial。
#[test]
fn missing_optional_default_does_not_degrade_environment_root() {
    let temp = tempdir().expect("temporary home is available");
    let environment_root = temp.path().join("custom-codex-home");
    write_signature_rollout(&environment_root.join("sessions/rollout-environment.jsonl"));

    let result = discover_quick(
        &DiscoveryInputs {
            home_dir: Some(temp.path().join("home-without-codex")),
            codex_home: Some(environment_root),
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

/// 已创建但尚无有效 rollout 的可选默认根同样不构成覆盖缺口。
#[test]
fn unsigned_optional_default_does_not_degrade_environment_root() {
    let temp = tempdir().expect("temporary home is available");
    let default_sessions = temp.path().join("home/.codex/sessions");
    fs::create_dir_all(&default_sessions).expect("empty default sessions are created");
    fs::write(
        default_sessions.join("rollout-unsigned.jsonl"),
        b"{\"type\":\"event_msg\"}\n",
    )
    .expect("unsigned default fixture is written");
    let environment_root = temp.path().join("custom-codex-home");
    write_signature_rollout(&environment_root.join("sessions/rollout-environment.jsonl"));

    let result = discover_quick(
        &DiscoveryInputs {
            home_dir: Some(temp.path().join("home")),
            codex_home: Some(environment_root),
            registered_roots: Vec::new(),
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Complete);
    assert_eq!(result.coverage.skipped_count, 0);
    assert_eq!(result.roots.len(), 1);
}

/// 用户登记根必须在共享预算中先于自动候选，且重复路径保持登记身份。
#[test]
fn quick_candidates_prioritize_registered_then_environment_and_default() {
    let inputs = DiscoveryInputs {
        home_dir: Some(PathBuf::from("/synthetic/home")),
        codex_home: Some(PathBuf::from("/synthetic/environment")),
        registered_roots: vec![RegisteredRoot {
            root_id: Some("root-registered".to_owned()),
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
        Some("root-registered")
    );
    assert_eq!(candidates[1].method, DiscoveryMethod::Environment);
    assert_eq!(candidates[2].method, DiscoveryMethod::DefaultHome);
}

/// 后续自动候选耗尽预算，也不能让已登记有效根在同一轮被饿死。
#[test]
fn automatic_candidate_budget_exhaustion_keeps_registered_root() {
    let temp = tempdir().expect("temporary roots are available");
    let registered = temp.path().join("registered");
    let exhausting = temp.path().join("exhausting");
    write_signature_rollout(&registered.join("sessions/rollout-valid.jsonl"));
    fs::create_dir_all(exhausting.join("sessions/nested")).expect("budget fixture is created");
    let inputs = DiscoveryInputs {
        codex_home: Some(exhausting),
        registered_roots: vec![RegisteredRoot {
            root_id: Some("root-priority".to_owned()),
            path: registered,
            alias: "优先登记根".to_owned(),
            enabled: true,
        }],
        ..DiscoveryInputs::default()
    };
    let options = FullDiscoveryOptions {
        max_signature_directories: 1,
        ..FullDiscoveryOptions::default()
    };

    let result = discover_candidates(
        quick_candidates(&inputs),
        &CancellationToken::new(),
        &options,
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert_eq!(result.roots.len(), 1);
    assert_eq!(result.roots[0].root_id, "root-priority");
    assert!(
        !result
            .unconfirmed_root_ids
            .contains(&"root-priority".to_owned())
    );
}

/// 验证默认、环境与历史登记都只是候选，空目录不能绕过 rollout 签名。
#[test]
fn quick_discovery_rejects_unsigned_known_roots() {
    let temp = tempdir().expect("temporary root is available");
    let invalid = temp.path().join("remembered-but-empty");
    fs::create_dir_all(invalid.join("sessions")).expect("empty remembered area is created");

    let result = discover_quick(
        &DiscoveryInputs {
            home_dir: Some(temp.path().to_path_buf()),
            codex_home: Some(invalid.clone()),
            registered_roots: vec![RegisteredRoot {
                root_id: Some("root-invalid".to_owned()),
                path: invalid.clone(),
                alias: "历史空根".to_owned(),
                enabled: true,
            }],
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert_eq!(
        result.confirmed_invalid_root_ids,
        ["root-invalid".to_owned()]
    );
    assert_eq!(result.coverage.state, CoverageState::Partial);
}

/// 验证 logs、SQLite-only 与 Claude projects 结构都不能通过 Codex 快速签名。
#[test]
fn quick_discovery_rejects_non_codex_data_directories() {
    let temp = tempdir().expect("temporary root is available");
    let logs = temp.path().join("logs");
    fs::create_dir_all(&logs).expect("logs directory is created");
    fs::write(logs.join("logs_2.sqlite"), b"not-a-codex-session").expect("sqlite bait is written");
    let claude = temp.path().join(".claude");
    fs::create_dir_all(claude.join("projects/project-a"))
        .expect("Claude projects fixture is created");
    fs::write(
        claude.join("projects/project-a/00000000-0000-0000-0000-000000000001.jsonl"),
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n",
    )
    .expect("Claude transcript bait is written");

    for candidate in [logs, claude] {
        let result = discover_quick(
            &DiscoveryInputs {
                registered_roots: vec![RegisteredRoot {
                    root_id: Some("root-non-codex".to_owned()),
                    path: candidate,
                    alias: "非 Codex 目录".to_owned(),
                    enabled: true,
                }],
                ..DiscoveryInputs::default()
            },
            &CancellationToken::new(),
        );
        assert!(result.roots.is_empty());
        assert_eq!(
            result.confirmed_invalid_root_ids,
            ["root-non-codex".to_owned()]
        );
    }
}

/// 验证辅助根按物理目录名拒绝，而用户可修改的安全别名不会误伤合法根或绕过门禁。
#[test]
fn quick_discovery_rejects_physical_logs_root_even_with_valid_rollout() {
    let temp = tempdir().expect("temporary root is available");
    let valid = temp.path().join(".codex");
    let forbidden = temp.path().join("logs");
    write_signature_rollout(&valid.join("sessions/rollout-valid.jsonl"));
    write_signature_rollout(&forbidden.join("sessions/rollout-bait.jsonl"));

    let result = discover_quick(
        &DiscoveryInputs {
            registered_roots: vec![
                RegisteredRoot {
                    root_id: Some("root-valid".to_owned()),
                    path: valid,
                    alias: "logs".to_owned(),
                    enabled: true,
                },
                RegisteredRoot {
                    root_id: Some("root-logs".to_owned()),
                    path: forbidden,
                    alias: "看似正常".to_owned(),
                    enabled: true,
                },
            ],
            ..DiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(result.roots[0].root_id, "root-valid");
    assert_eq!(result.roots[0].alias, "logs");
    assert_eq!(result.confirmed_invalid_root_ids, ["root-logs"]);
}

/// 验证快速扫描在读取任何候选内容前拒绝网络根，并清除已确认违反策略的登记项。
#[test]
fn quick_discovery_rejects_registered_network_root() {
    let result = discover_quick(
        &DiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("root-network".to_owned()),
                path: synthetic_network_path().join(".codex"),
                alias: "网络根".to_owned(),
                enabled: true,
            }],
            ..DiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert_eq!(result.confirmed_invalid_root_ids, ["root-network"]);
    assert_eq!(result.network_skipped_count, 1);
    assert_eq!(result.coverage.state, CoverageState::Partial);
}

/// 验证快速签名开始前收到取消时不把任何未检查候选列为确认失效。
#[test]
fn quick_discovery_cancellation_does_not_confirm_invalid_roots() {
    let temp = tempdir().expect("temporary root is available");
    fs::create_dir_all(temp.path().join(".codex/sessions")).expect("candidate area is created");
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let result = discover_quick(
        &DiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("root-cancelled".to_owned()),
                path: temp.path().join(".codex"),
                alias: "取消候选".to_owned(),
                enabled: true,
            }],
            ..DiscoveryInputs::default()
        },
        &cancellation,
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert!(result.roots.is_empty());
    assert!(result.confirmed_invalid_root_ids.is_empty());
}

/// 验证快速发现拒绝符号链接根，不会穿越到真实会话目录。
#[cfg(unix)]
#[test]
fn quick_discovery_rejects_symlink_root() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("temporary root is available");
    let real = temp.path().join("real");
    fs::create_dir_all(real.join("sessions")).expect("real fixture root is created");
    let linked = temp.path().join("linked");
    symlink(&real, &linked).expect("symlink fixture is created");

    let result = discover_quick(
        &DiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("root-linked".to_owned()),
                path: linked,
                alias: "链接根".to_owned(),
                enabled: true,
            }],
            ..DiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert_eq!(result.symlink_skipped_count, 1);
}

/// 验证候选位于链接父目录下时也不会在规范化过程中穿越边界。
#[cfg(unix)]
#[test]
fn quick_discovery_rejects_symlink_ancestor() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("temporary root is available");
    let real_parent = temp.path().join("real-parent");
    fs::create_dir_all(real_parent.join("codex/sessions")).expect("real nested root is created");
    let linked_parent = temp.path().join("linked-parent");
    symlink(&real_parent, &linked_parent).expect("parent symlink is created");

    let result = discover_quick(
        &DiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: Some("root-linked-parent".to_owned()),
                path: linked_parent.join("codex"),
                alias: "链接父目录根".to_owned(),
                enabled: true,
            }],
            ..DiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert_eq!(result.symlink_skipped_count, 1);
}

/// 写入只含最小允许字段的首条会话记录，不包含正文或用户路径。
fn write_signature_rollout(path: &Path) {
    fs::create_dir_all(path.parent().expect("fixture has a parent"))
        .expect("fixture parent is created");
    let mut file = File::create(path).expect("rollout fixture is created");
    writeln!(
        file,
        "{{\"timestamp\":\"2026-07-31T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"synthetic-session\"}}}}"
    )
    .expect("signature line is written");
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
