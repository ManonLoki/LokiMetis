use super::*;
use crate::backend::local_index::{LocalPathStatus, classify_local_path};
use tempfile::tempdir;

/// 验证 worker 少于卷数量时，轮转游标仍依次服务后续卷。
#[test]
fn fair_queue_rotates_beyond_worker_count() {
    let mut queues = (0..3)
        .map(|index| VolumeQueue {
            pending: VecDeque::from([PathBuf::from(format!("volume-{index}"))]),
            completed: false,
            traversal_root: PathBuf::from(format!("volume-{index}")),
        })
        .collect::<Vec<_>>();
    let mut cursor = 0;
    let order = (0..3)
        .map(|_| take_fair_jobs(&mut queues, 1, &mut cursor)[0].0)
        .collect::<Vec<_>>();
    assert_eq!(order, [0, 1, 2]);
}

/// 单卷存在足够目录时也能填满 worker，不会退化成串行扫描。
#[test]
fn single_volume_fills_available_workers() {
    let mut queues = vec![VolumeQueue {
        pending: VecDeque::from([
            PathBuf::from("a"),
            PathBuf::from("b"),
            PathBuf::from("c"),
            PathBuf::from("d"),
        ]),
        completed: false,
        traversal_root: PathBuf::from("root"),
    }];
    let mut cursor = 0;
    let jobs = take_fair_jobs(&mut queues, 4, &mut cursor);
    assert_eq!(jobs.len(), 4);
    assert!(jobs.iter().all(|(volume, _)| *volume == 0));
}

/// 高优先级目录在用户根之前入队，且缺失的可选目录不会制造扫描错误。
#[test]
fn priority_queue_places_existing_seeds_before_profile_root() {
    let temp = tempdir().unwrap();
    let preferred = temp.path().join("AppData/Roaming/AIManagerData");
    fs::create_dir_all(&preferred).unwrap();
    let queue = priority_queue(
        temp.path(),
        &[preferred.clone(), temp.path().join("missing")],
    );
    assert_eq!(
        queue.into_iter().collect::<Vec<_>>(),
        [preferred, temp.path().into()]
    );
}

/// 搜索根本身也在优先列表中时只入队一次，避免环境内容子树被重复遍历。
#[test]
fn priority_queue_deduplicates_traversal_root() {
    let temp = tempdir().unwrap();
    let queue = priority_queue(temp.path(), &[temp.path().to_path_buf()]);
    assert_eq!(queue.into_iter().collect::<Vec<_>>(), [temp.path()]);
}

/// 同一候选由多个文件命中时只实时通知前端一次。
#[test]
fn candidate_callback_only_reports_first_insertion() {
    let temp = tempdir().unwrap();
    let session = temp.path().join("root/sessions/2026");
    fs::create_dir_all(&session).unwrap();
    fs::write(session.join("rollout-a.jsonl"), b"unopened").unwrap();
    fs::write(session.join("rollout-b.jsonl"), b"unopened").unwrap();
    let coordinator = RootDiscoveryCoordinator::default();
    let delivered = std::cell::Cell::new(0_u64);
    discover_metadata_roots_in_with_callback(
        &coordinator,
        LocalVolumeRoots {
            search_roots: vec![temp.path().into()],
            ..LocalVolumeRoots::default()
        },
        Vec::new(),
        &|_| delivered.set(delivered.get() + 1),
    );
    assert_eq!(coordinator.candidates().len(), 1);
    assert_eq!(delivered.get(), 1);
}

/// 快速扫描只保留平台优先列表中已确认本地、非链接的现存目录。
#[test]
fn user_priority_scope_is_only_platform_priority_roots() {
    let priority_roots = platform_priority_roots();
    let scope = discovery_scope(RootDiscoveryScope::UserPriority);
    assert!(
        scope
            .search_roots
            .iter()
            .all(|path| priority_roots.contains(path))
    );
    assert!(scope.search_roots.iter().all(|path| {
        classify_local_path(path) == LocalPathStatus::ConfirmedLocal
            && validate_local_plain_directory(path).is_ok()
    }));
    #[cfg(target_os = "windows")]
    {
        let profile = current_user_home().expect("Windows user profile is available");
        assert!(
            !discovery_scope(RootDiscoveryScope::UserPriority)
                .search_roots
                .contains(&profile)
        );
    }
    #[cfg(target_os = "macos")]
    {
        let home = current_user_home().expect("macOS user home is available");
        assert!(
            !discovery_scope(RootDiscoveryScope::UserPriority)
                .search_roots
                .contains(&home)
        );
    }
}

/// 网络、未知或链接优先根在读取目录项前被排除；缺失可选根不制造告警。
#[cfg(unix)]
#[test]
fn priority_scope_rejects_unsafe_roots_before_traversal() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let local = temp.path().join("local");
    fs::create_dir_all(&local).unwrap();
    let linked = temp.path().join("linked");
    symlink(&local, &linked).unwrap();
    let missing = temp.path().join("missing");
    let network = PathBuf::from("//synthetic-server/share");

    let scope = scope::local_priority_scope(vec![
        local.clone(),
        linked.clone(),
        missing,
        network.clone(),
    ]);

    assert_eq!(scope.search_roots, [local]);
    assert_eq!(scope.network_skipped_count, 1);
    assert_eq!(scope.other_skipped_count, 1);
    assert!(scope.excluded_roots.contains(&linked));
    assert!(scope.excluded_roots.contains(&network));
}

/// 快速扫描只遍历给定优先目录内部，不会走进同级的用户目录其他路径。
#[test]
fn user_priority_does_not_walk_outside_priority_roots() {
    let temp = tempdir().unwrap();
    let inside_sessions = temp.path().join(".codex/sessions/2026");
    let outside_sessions = temp.path().join("Desktop/other/sessions/2026");
    fs::create_dir_all(&inside_sessions).unwrap();
    fs::create_dir_all(&outside_sessions).unwrap();
    fs::write(inside_sessions.join("rollout-inside.jsonl"), b"unopened").unwrap();
    fs::write(outside_sessions.join("rollout-outside.jsonl"), b"unopened").unwrap();

    let coordinator = RootDiscoveryCoordinator::default();
    discover_metadata_roots_in(
        &coordinator,
        LocalVolumeRoots {
            search_roots: vec![temp.path().join(".codex")],
            ..LocalVolumeRoots::default()
        },
    );
    let candidates = coordinator.candidates();
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].absolute_path.ends_with(".codex"));
    assert!(!candidates[0].absolute_path.contains("Desktop"));
}

/// Windows 与 macOS 分别维护自己的优先目录顺序，且均相对当前用户根。
#[test]
fn current_platform_priority_order_matches_product_list() {
    let roots = platform_priority_roots();
    #[cfg(target_os = "windows")]
    {
        let profile = current_user_home().expect("Windows user profile is available");
        assert!(
            roots.starts_with(
                &[
                    "AppData/Roaming/AIManagerData",
                    ".claude",
                    ".codex",
                    "AppData/Roaming",
                    "AppData/Local",
                ]
                .map(|relative| profile.join(relative))
            )
        );
    }
    #[cfg(target_os = "macos")]
    {
        let home = current_user_home().expect("macOS user home is available");
        assert!(
            roots.starts_with(
                &[
                    "Library/Application Support/AIManagerData",
                    ".claude",
                    ".codex",
                    "Library/Application Support",
                    "Library",
                ]
                .map(|relative| home.join(relative))
            )
        );
    }
}

/// 环境变量按精确根执行有界严格签名，不遍历其父目录或接受无签名目录。
#[test]
fn explicit_environment_roots_require_strict_client_signatures() {
    let temp = tempdir().unwrap();
    let codex = temp.path().join("custom-codex");
    let claude = temp.path().join("custom-claude");
    let codex_rollout = codex.join("sessions/2026/08/rollout-environment.jsonl");
    fs::create_dir_all(codex_rollout.parent().unwrap()).unwrap();
    fs::write(
        &codex_rollout,
        b"{\"timestamp\":\"2026-08-08T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic-environment\"}}\n",
    )
    .unwrap();
    let session_id = "00000000-0000-0000-0000-000000000001";
    let claude_transcript = claude.join(format!("projects/project-a/{session_id}.jsonl"));
    fs::create_dir_all(claude_transcript.parent().unwrap()).unwrap();
    fs::write(
        &claude_transcript,
        format!(
            "{{\"type\":\"user\",\"sessionId\":\"{session_id}\",\"timestamp\":\"2026-08-08T00:00:00Z\"}}\n{{\"type\":\"assistant\",\"sessionId\":\"{session_id}\",\"timestamp\":\"2026-08-08T00:00:01Z\",\"message\":{{\"id\":\"synthetic\",\"model\":\"claude-synthetic\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}}}\n"
        ),
    )
    .unwrap();

    let coordinator = RootDiscoveryCoordinator::default();
    let inspection = explicit_environment_candidates(
        &coordinator,
        Some(codex.clone()),
        Some(claude.clone()),
        None,
        None,
    );
    let candidates = inspection.candidates;
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].1, RootCandidateEvidence::CodexRollout);
    assert_eq!(candidates[1].1, RootCandidateEvidence::ClaudeTranscript);
    assert_eq!(candidates[0].0, fs::canonicalize(codex).unwrap());
    assert_eq!(candidates[1].0, fs::canonicalize(claude).unwrap());

    let subagent = temp.path().join("custom-claude-subagent");
    let subagent_transcript = subagent.join(format!(
        "projects/project-a/{session_id}/subagents/agent-synthetic.jsonl"
    ));
    fs::create_dir_all(subagent_transcript.parent().unwrap()).unwrap();
    fs::write(
        &subagent_transcript,
        format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"{session_id}\",\"timestamp\":\"2026-08-08T00:00:01Z\",\"message\":{{\"id\":\"synthetic-subagent\",\"model\":\"claude-synthetic\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}}}\n"
        ),
    )
    .unwrap();
    let subagent_inspection =
        explicit_environment_candidates(&coordinator, None, Some(subagent.clone()), None, None);
    let subagent_candidates = subagent_inspection.candidates;
    assert_eq!(subagent_candidates.len(), 1);
    assert_eq!(
        subagent_candidates[0].1,
        RootCandidateEvidence::ClaudeSubagent
    );
    assert_eq!(
        subagent_candidates[0].0,
        fs::canonicalize(subagent).unwrap()
    );

    let unsigned = temp.path().join("unsigned");
    fs::create_dir_all(&unsigned).unwrap();
    assert!(
        explicit_environment_candidates(
            &coordinator,
            Some(unsigned),
            Some(PathBuf::from("relative")),
            None,
            None,
        )
        .candidates
        .is_empty()
    );
}

/// 同一物理目录可同时承载两个客户端的独立严格根，候选 ID 必须按客户端隔离。
#[test]
fn same_path_keeps_independent_codex_and_claude_candidates() {
    let temp = tempdir().unwrap();
    let mixed = temp.path().join("mixed-agent-root");
    let codex_rollout = mixed.join("sessions/rollout-mixed.jsonl");
    fs::create_dir_all(codex_rollout.parent().unwrap()).unwrap();
    fs::write(
        codex_rollout,
        b"{\"timestamp\":\"2026-08-08T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic-mixed\"}}\n",
    )
    .unwrap();
    let session_id = "00000000-0000-0000-0000-000000000001";
    let claude_transcript = mixed.join(format!("projects/project-a/{session_id}.jsonl"));
    fs::create_dir_all(claude_transcript.parent().unwrap()).unwrap();
    fs::write(
        claude_transcript,
        format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"{session_id}\",\"timestamp\":\"2026-08-08T00:00:01Z\",\"message\":{{\"id\":\"synthetic\",\"model\":\"claude-synthetic\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}}}\n"
        ),
    )
    .unwrap();
    let cancellation = CancellationToken::new();
    let inspected = inspect_explicit_environment_candidates(
        Some(mixed.clone()),
        Some(mixed),
        None,
        None,
        &cancellation,
    );
    let coordinator = RootDiscoveryCoordinator::default();
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::UserPriority,
        1,
    ));
    for (path, evidence) in inspected.candidates {
        submit_candidate(
            &coordinator,
            path,
            evidence,
            RootDiscoveryStrategy::MetadataTraversal,
            &|_| {},
        );
    }

    let candidates = coordinator.candidates();
    assert_eq!(candidates.len(), 2);
    assert_ne!(candidates[0].id, candidates[1].id);
    assert_eq!(candidates[0].client, SourceClientKind::Codex);
    assert_eq!(candidates[1].client, SourceClientKind::ClaudeCode);
}

/// 已请求取消时，环境严格签名不会提交任何候选。
#[test]
fn explicit_environment_roots_observe_discovery_cancellation() {
    let temp = tempdir().unwrap();
    let codex = temp.path().join("custom-codex");
    let rollout = codex.join("sessions/rollout-environment.jsonl");
    fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    fs::write(
        rollout,
        b"{\"timestamp\":\"2026-08-08T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"synthetic-environment\"}}\n",
    )
    .unwrap();
    let coordinator = RootDiscoveryCoordinator::default();
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::UserPriority,
        1,
    ));
    assert!(coordinator.request_cancel());

    assert!(
        explicit_environment_candidates(&coordinator, Some(codex), None, None, None)
            .candidates
            .is_empty()
    );
}

/// 环境精确根被网络策略拒绝时，任务进度必须保留跳过计数。
#[test]
fn explicit_environment_gaps_are_visible_in_discovery_progress() {
    let coordinator = RootDiscoveryCoordinator::default();
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::UserPriority,
        0,
    ));
    #[cfg(target_os = "windows")]
    let network_root = PathBuf::from(r"\\synthetic-server\share\codex");
    #[cfg(not(target_os = "windows"))]
    let network_root = PathBuf::from("/Network/synthetic-codex");
    let inspection =
        explicit_environment_candidates(&coordinator, Some(network_root), None, None, None);
    assert!(inspection.candidates.is_empty());
    assert_eq!(inspection.skipped, 1);

    record_explicit_environment_gaps(&coordinator, &inspection);
    assert_eq!(coordinator.snapshot().progress.skipped, 1);
}

/// 零个可遍历根时，平台预检拒绝的范围也不能在提前结束时丢失。
#[test]
fn empty_scope_keeps_volume_preflight_gaps_visible() {
    let coordinator = RootDiscoveryCoordinator::default();
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::UserPriority,
        0,
    ));
    let volumes = LocalVolumeRoots {
        network_skipped_count: 2,
        other_skipped_count: 1,
        ..LocalVolumeRoots::default()
    };

    let progress = record_volume_scope_gaps(&coordinator, &volumes, 0);
    assert_eq!(progress.volumes_total, 0);
    assert_eq!(progress.skipped, 3);
    assert_eq!(coordinator.snapshot().progress.skipped, 3);
}

/// 系统索引的提前结果不能越过当前扫描范围。
#[test]
fn indexed_candidate_must_stay_inside_selected_scope() {
    let volumes = LocalVolumeRoots {
        search_roots: vec![PathBuf::from("C:/Users/allowed")],
        ..LocalVolumeRoots::default()
    };
    assert!(path_is_in_search_scope(
        Path::new("C:/Users/allowed/.codex/sessions/rollout-a.jsonl"),
        &volumes
    ));
    assert!(!path_is_in_search_scope(
        Path::new("D:/other/.codex/sessions/rollout-a.jsonl"),
        &volumes
    ));
}

/// 验证 C/D/E 等价多卷按轮次推进且均能产生候选。
#[test]
fn discovers_candidates_across_all_volume_queues() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let third = tempdir().unwrap();
    let session = third
        .path()
        .join("users/u/AppData/Roaming/AIManagerData/a/sessions/2026/08/06");
    fs::create_dir_all(&session).unwrap();
    fs::write(session.join("rollout-test.jsonl"), b"must never be opened").unwrap();
    let coordinator = RootDiscoveryCoordinator::default();
    discover_metadata_roots_in(
        &coordinator,
        LocalVolumeRoots {
            search_roots: vec![
                first.path().into(),
                second.path().into(),
                third.path().into(),
            ],
            ..LocalVolumeRoots::default()
        },
    );
    assert_eq!(coordinator.candidates().len(), 1);
    assert_eq!(coordinator.snapshot().progress.volumes_completed, 3);
}

/// 验证普通 JSONL 与 logs 根不会成为候选。
#[test]
fn rejects_unrelated_jsonl_and_auxiliary_roots() {
    let ordinary = Path::new("C:/x/projects/p/not-a-uuid.jsonl");
    let logs = Path::new("C:/logs/sessions/2026/rollout-a.jsonl");
    assert!(candidate_from_file_name(ordinary).is_none());
    assert!(candidate_from_file_name(logs).is_none());
}

/// 验证 Codex、Claude 主 transcript 与 subagent 只凭合法文件名层级命中。
#[test]
fn recognizes_only_approved_filename_hierarchies() {
    let uuid = "00000000-0000-0000-0000-000000000001";
    let codex = Path::new("C:/root/sessions/2026/rollout-a.jsonl");
    let claude = PathBuf::from(format!("C:/root/projects/project-a/{uuid}.jsonl"));
    let subagent = PathBuf::from(format!(
        "C:/root/projects/project-a/{uuid}/subagents/agent-a.jsonl"
    ));
    let forged = PathBuf::from(format!("C:/root/cache/project-a/{uuid}.jsonl"));
    assert_eq!(
        candidate_from_file_name(codex).unwrap().1,
        RootCandidateEvidence::CodexRollout
    );
    assert_eq!(
        candidate_from_file_name(&claude).unwrap().1,
        RootCandidateEvidence::ClaudeTranscript
    );
    assert_eq!(
        candidate_from_file_name(&subagent).unwrap().1,
        RootCandidateEvidence::ClaudeSubagent
    );
    assert!(candidate_from_file_name(&forged).is_none());
    let grok = Path::new(
        "C:/Users/me/.grok/sessions/%2Ftmp%2Fapp/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/updates.jsonl",
    );
    let grok_hit = candidate_from_file_name(grok).expect("Grok updates.jsonl is a candidate");
    assert_eq!(grok_hit.1, RootCandidateEvidence::GrokSessionUpdates);
    assert_eq!(grok_hit.0, PathBuf::from("C:/Users/me/.grok"));
}

/// Windows 独占锁定 JSONL 后仍可完成发现，证明发现阶段没有尝试打开正文。
#[cfg(windows)]
#[test]
fn discovers_an_exclusively_locked_jsonl_without_opening_it() {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;

    let temp = tempdir().unwrap();
    let session = temp.path().join("root/sessions/2026");
    fs::create_dir_all(&session).unwrap();
    let mut locked = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .share_mode(0)
        .open(session.join("rollout-locked.jsonl"))
        .unwrap();
    locked
        .write_all(b"content access must fail while locked")
        .unwrap();
    let coordinator = RootDiscoveryCoordinator::default();
    discover_metadata_roots_in(
        &coordinator,
        LocalVolumeRoots {
            search_roots: vec![temp.path().into()],
            ..LocalVolumeRoots::default()
        },
    );
    assert_eq!(coordinator.candidates().len(), 1);
}

/// 静态锁定发现适配器没有普通文件内容读取入口。
#[test]
fn discovery_adapter_has_no_content_open_primitive() {
    let source = include_str!("../metadata_discovery.rs");
    for parts in [
        ["File", "::open"],
        ["read_to", "_string"],
        ["read_to", "_end"],
        ["Buf", "Reader"],
    ] {
        let forbidden = parts.concat();
        assert!(!source.contains(&forbidden), "forbidden content primitive");
    }
}

/// 全盘遍历遇到排除名单或系统跳过目录名时不进入子树。
#[test]
fn full_discovery_skips_platform_impossible_directories() {
    let temp = tempdir().unwrap();
    let allowed = temp.path().join("Users/demo/.codex/sessions/2026");
    #[cfg(target_os = "windows")]
    let blocked = temp.path().join("Windows/fake/sessions/2026");
    #[cfg(target_os = "macos")]
    let blocked = temp.path().join("System/fake/sessions/2026");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let blocked = temp.path().join("proc/fake/sessions/2026");
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&blocked).unwrap();
    fs::write(allowed.join("rollout-ok.jsonl"), b"unopened").unwrap();
    fs::write(blocked.join("rollout-skip.jsonl"), b"unopened").unwrap();

    let root = temp.path().to_path_buf();
    let excluded =
        append_platform_discovery_excludes_for_volumes(Vec::new(), std::slice::from_ref(&root));
    let coordinator = RootDiscoveryCoordinator::default();
    discover_metadata_roots_in_with_callback(
        &coordinator,
        LocalVolumeRoots {
            search_roots: vec![root],
            excluded_roots: excluded,
            ..LocalVolumeRoots::default()
        },
        Vec::new(),
        &|_| {},
    );
    let candidates = coordinator.candidates();
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].absolute_path.contains(".codex"));
}

/// 在隔离主目录写入带签名的默认 `.grok`，不设 `GROK_HOME`。
fn write_signed_default_grok_home(home_dir: &Path) -> PathBuf {
    let grok_home = home_dir.join(".grok");
    let session = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fapp")
        .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    fs::create_dir_all(&session).expect("session directory");
    fs::write(
        session.join("updates.jsonl"),
        crate::backend::local_index::SYNTHETIC_GROK_UPDATES_JSONL,
    )
    .expect("fixture written");
    grok_home
}

/// UserPriority 显式核对在未设 `GROK_HOME` 时仍必须发现默认 `~/.grok`。
#[test]
fn user_priority_finds_default_grok_home_without_grok_home_env() {
    let isolated_home = tempdir().unwrap();
    let grok_home = write_signed_default_grok_home(isolated_home.path());
    let coordinator = RootDiscoveryCoordinator::default();
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::UserPriority,
        0,
    ));
    let inspection = explicit_environment_candidates(
        &coordinator,
        None,
        None,
        None,
        Some(isolated_home.path().to_path_buf()),
    );
    assert_eq!(inspection.candidates.len(), 1);
    assert_eq!(
        inspection.candidates[0].1,
        RootCandidateEvidence::GrokSessionUpdates
    );
    assert_ne!(
        inspection.candidates[0].1,
        RootCandidateEvidence::CodexRollout
    );
    assert_ne!(
        inspection.candidates[0].1,
        RootCandidateEvidence::ClaudeTranscript
    );
    assert_eq!(
        inspection.candidates[0].0,
        fs::canonicalize(&grok_home).unwrap()
    );
    submit_candidate(
        &coordinator,
        inspection.candidates[0].0.clone(),
        inspection.candidates[0].1,
        RootDiscoveryStrategy::MetadataTraversal,
        &|_| {},
    );
    let submitted = coordinator.candidates();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].client, SourceClientKind::GrokBuildCli);
    eprintln!(
        "gating default grok client={:?} evidence={:?}",
        submitted[0].client, inspection.candidates[0].1
    );
}

/// 默认 `.grok` 缺失、空 sessions、或只有未完成轮次时不得伪造 Grok 根。
#[test]
fn user_priority_does_not_invent_grok_root_without_signed_updates() {
    let missing = tempdir().unwrap();
    let unsigned = tempdir().unwrap();
    fs::create_dir_all(unsigned.path().join(".grok/sessions")).unwrap();
    let started_only = tempdir().unwrap();
    let started_session = started_only
        .path()
        .join(".grok/sessions/%2Ftmp%2Fapp/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    fs::create_dir_all(&started_session).unwrap();
    fs::write(
        started_session.join("updates.jsonl"),
        r#"{"timestamp":1786891791,"method":"session/update","params":{"sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","update":{"sessionUpdate":"turn_started"}}}"#,
    )
    .unwrap();
    let coordinator = RootDiscoveryCoordinator::default();
    for home in [missing.path(), unsigned.path(), started_only.path()] {
        let inspection = explicit_environment_candidates(
            &coordinator,
            None,
            None,
            None,
            Some(home.to_path_buf()),
        );
        assert!(
            inspection
                .candidates
                .iter()
                .all(|(_, evidence)| *evidence != RootCandidateEvidence::GrokSessionUpdates),
            "unsigned or missing .grok must not yield a Grok root"
        );
        eprintln!(
            "gating unsigned home candidates={} grok_hits=0",
            inspection.candidates.len()
        );
    }
}

/// 显式非空 `GROK_HOME` 指向另一合法夹具时仍发现该目录。
#[test]
fn user_priority_still_finds_explicit_grok_home() {
    let isolated_home = tempdir().unwrap();
    let grok_home = tempdir().unwrap();
    let session = grok_home
        .path()
        .join("sessions")
        .join("%2Ftmp%2Fapp")
        .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    fs::create_dir_all(&session).unwrap();
    fs::write(
        session.join("updates.jsonl"),
        crate::backend::local_index::SYNTHETIC_GROK_UPDATES_JSONL,
    )
    .unwrap();
    let coordinator = RootDiscoveryCoordinator::default();
    let inspection = explicit_environment_candidates(
        &coordinator,
        None,
        None,
        Some(grok_home.path().to_path_buf()),
        Some(isolated_home.path().to_path_buf()),
    );
    assert_eq!(inspection.candidates.len(), 1);
    assert_eq!(
        inspection.candidates[0].1,
        RootCandidateEvidence::GrokSessionUpdates
    );
    assert_eq!(
        inspection.candidates[0].0,
        fs::canonicalize(grok_home.path()).unwrap()
    );
    eprintln!(
        "gating explicit GROK_HOME evidence={:?} path_is_isolated_home={}",
        inspection.candidates[0].1,
        inspection.candidates[0].0.starts_with(isolated_home.path())
    );
}

/// 生产 `params.update` 信封在未设 `GROK_HOME` 时仍必须被默认 `.grok` 识别为 Grok。
#[test]
fn user_priority_finds_default_grok_home_from_production_envelope() {
    let isolated_home = tempdir().unwrap();
    let grok_home = isolated_home.path().join(".grok");
    let session = grok_home
        .join("sessions")
        .join("%2Ftmp%2Fapp")
        .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    fs::create_dir_all(&session).unwrap();
    fs::write(
        session.join("updates.jsonl"),
        crate::backend::local_index::PRODUCTION_GROK_SESSION_ENVELOPE_JSONL,
    )
    .unwrap();
    let coordinator = RootDiscoveryCoordinator::default();
    let inspection = explicit_environment_candidates(
        &coordinator,
        None,
        None,
        None,
        Some(isolated_home.path().to_path_buf()),
    );
    assert_eq!(inspection.candidates.len(), 1);
    assert_eq!(
        inspection.candidates[0].1,
        RootCandidateEvidence::GrokSessionUpdates
    );
    assert_eq!(
        inspection.candidates[0].0,
        fs::canonicalize(&grok_home).unwrap()
    );
    submit_candidate(
        &coordinator,
        inspection.candidates[0].0.clone(),
        inspection.candidates[0].1,
        RootDiscoveryStrategy::MetadataTraversal,
        &|_| {},
    );
    let submitted = coordinator.candidates();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].client, SourceClientKind::GrokBuildCli);
    eprintln!(
        "gating production envelope evidence={:?} client={:?}",
        inspection.candidates[0].1, submitted[0].client
    );
}
