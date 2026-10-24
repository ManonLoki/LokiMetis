// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证 C/D/E 等价多卷按轮次推进且均能产生候选。；验证普通 JSONL 与 logs 根不会成为候选。；验证 Codex、Claude 主 transcript 与 subagent 只凭合法文件名层级命中。

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
    // 源码已拆成同目录分片，契约检查要看到完整文本（含兜底遍历实现）。
    let source = [include_str!("mod.rs"), include_str!("metadata_discovery_fallback_walk.rs")].join("\n");
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
        crate::local_index::SYNTHETIC_GROK_UPDATES_JSONL,
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
    assert_eq!(submitted[0].client, SourceClientKind::Grok);
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
        crate::local_index::SYNTHETIC_GROK_UPDATES_JSONL,
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
        crate::local_index::PRODUCTION_GROK_SESSION_ENVELOPE_JSONL,
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
    assert_eq!(submitted[0].client, SourceClientKind::Grok);
    eprintln!(
        "gating production envelope evidence={:?} client={:?}",
        inspection.candidates[0].1, submitted[0].client
    );
}
