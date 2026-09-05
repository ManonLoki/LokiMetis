use super::*;

/// 验证初始化批次只接受完成/部分完成，显式发现取消后仍可统一索引。
#[test]
fn refresh_trigger_maps_only_approved_discovery_states() {
    for lifecycle in [
        RootDiscoveryLifecycle::Idle,
        RootDiscoveryLifecycle::Running,
        RootDiscoveryLifecycle::Cancelled,
        RootDiscoveryLifecycle::Failed,
    ] {
        assert!(
            refresh_origin_for_context(
                LocalIndexRefreshTriggerDto::Initialization,
                false,
                lifecycle
            )
            .is_err()
        );
    }
    for lifecycle in [
        RootDiscoveryLifecycle::Complete,
        RootDiscoveryLifecycle::Partial,
    ] {
        let origin = refresh_origin_for_context(
            LocalIndexRefreshTriggerDto::Initialization,
            false,
            lifecycle,
        )
        .expect("settled initialization discovery may index");
        assert_eq!(origin, ScanStartOrigin::ExplicitUser);
        assert_eq!(
            loki_metis_core::ensure_scan_start_allowed(
                origin,
                false,
                loki_metis_core::ScanKind::Quick,
            ),
            Ok(())
        );
    }
    assert_eq!(
        refresh_origin_for_context(
            LocalIndexRefreshTriggerDto::DiscoveryBatch,
            true,
            RootDiscoveryLifecycle::Cancelled
        ),
        Ok(ScanStartOrigin::ExplicitUser)
    );
    assert!(
        refresh_origin_for_context(
            LocalIndexRefreshTriggerDto::DiscoveryBatch,
            true,
            RootDiscoveryLifecycle::Failed
        )
        .is_err()
    );
}

/// 验证批量输入固定按 Codex、Claude、Grok 排序，并拒绝空集与重复客户端。
#[test]
fn refresh_clients_are_unique_and_use_fixed_order() {
    assert_eq!(
        ordered_refresh_clients(&[
            AgentClientKindDto::GrokBuildCli,
            AgentClientKindDto::Codex,
            AgentClientKindDto::ClaudeCode,
        ]),
        Ok(vec![
            AgentClientKindDto::Codex,
            AgentClientKindDto::ClaudeCode,
            AgentClientKindDto::GrokBuildCli,
        ])
    );
    assert!(ordered_refresh_clients(&[]).is_err());
    assert!(
        ordered_refresh_clients(&[AgentClientKindDto::Codex, AgentClientKindDto::Codex]).is_err()
    );
}

/// 只有已开放且明确 NeedsRescan 的客户端进入启动后立即重建队列。
#[test]
fn upgrade_reindex_selection_is_scoped_and_ordered() {
    use loki_metis_core::{EnabledAgents, LocalIndexState, SourceClientKind};

    let enabled = EnabledAgents::empty()
        .with(SourceClientKind::Codex, true)
        .with(SourceClientKind::GrokBuildCli, true);
    let states = [
        (
            AgentClientKindDto::GrokBuildCli,
            LocalIndexState::NeedsRescan,
        ),
        (AgentClientKindDto::ClaudeCode, LocalIndexState::NeedsRescan),
        (AgentClientKindDto::Codex, LocalIndexState::Ready),
    ];
    assert_eq!(
        upgrade_reindex_clients(enabled, &states),
        vec![AgentClientKindDto::GrokBuildCli]
    );
}
