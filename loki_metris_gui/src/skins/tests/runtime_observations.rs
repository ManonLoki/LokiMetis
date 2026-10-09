mod runtime_observations {
    use super::*;

    /// 构造无需访问真实宿主的已解析实例，用页面探针结果覆盖运行态回归。
    fn resolved_fixture(host: SkinHostKind) -> super::super::ResolvedSkinHostInstance {
        resolved_instance_for_host(
            host,
            PlatformHostProcess {
                pid: 42,
                executable: PathBuf::from("verified-host"),
                command_line: "verified-host --remote-debugging-port=9341".into(),
            },
        )
    }

    /// Codex 账户缓存已经初始化时，当前空探针必须清理恢复态并保留账户展示。
    #[tokio::test]
    async fn codex_cached_account_skin_cannot_restore_uninstalled_skin() {
        let root = temp_directory("codex-cached-skin-observation");
        create_fixture(&root.join("user/old-skin"), "old-skin");
        let service = create_service(&root);
        let resolved = resolved_fixture(SkinHostKind::Codex);
        let key = super::super::runtime_instance_key(SkinHostKind::Codex, &resolved.id);
        let cached = service.account_profile_probe_cell(&resolved.id).unwrap();
        cached.set(Some(super::super::AccountProfile {
            label: Some("Account".into()),
            avatar_data_url: Some("data:image/png;base64,AQID".into()),
        })).unwrap();
        let descriptor = load_descriptor(&root.join("user/old-skin"), "old-skin", SkinSource::User)
            .unwrap();
        service.runtime.lock().await.instances.insert(key.clone(), super::super::InstanceRuntime {
            active: Some(descriptor), compatibility: None, task: None,
        });
        let instance = service.host_instance_from_observation(
            SkinHostKind::Codex, resolved, cached.get().unwrap().clone(), Ok(None),
            service.host_runtime_generation(SkinHostKind::Codex),
        ).await;

        assert_eq!(instance.state, CodexRuntimeState::Ready);
        assert_eq!(instance.account_label.as_deref(), Some("Account"));
        assert_eq!(instance.avatar_data_url.as_deref(), Some("data:image/png;base64,AQID"));
        assert!(instance.active_skin.is_none());
        assert!(!service.runtime.lock().await.instances.contains_key(&key));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 皮肤状态与首次账户缓存分开，两个宿主都使用本次探针中的精确皮肤。
    #[tokio::test]
    async fn current_skin_observation_replaces_recovered_skin_for_both_hosts() {
        let root = temp_directory("current-skin-observation");
        create_fixture(&root.join("user/old-skin"), "old-skin");
        create_fixture(&root.join("user/current-skin"), "current-skin");
        let service = create_service(&root);
        for host in [SkinHostKind::Codex, SkinHostKind::WorkBuddy] {
            let resolved = resolved_fixture(host);
            let key = super::super::runtime_instance_key(host, &resolved.id);
            let old = load_descriptor(&root.join("user/old-skin"), "old-skin", SkinSource::User)
                .unwrap();
            service.runtime.lock().await.instances.insert(key, super::super::InstanceRuntime {
                active: Some(old), compatibility: None, task: None,
            });
            let reference = SkinReference { source: SkinSource::User, id: "current-skin".into() };
            let instance = service.host_instance_from_observation(
                host, resolved, None,
                Ok(Some(super::super::RecoveredSkinIdentity::Exact(reference.clone()))),
                service.host_runtime_generation(host),
            ).await;
            assert_eq!(instance.active_skin, Some(reference));
            assert_eq!(instance.state, CodexRuntimeState::Ready);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 迟到探针跨过应用或停用代次后不能恢复活动皮肤，两个宿主都遵守同一限制。
    #[tokio::test]
    async fn stale_probe_cannot_recreate_active_skin_for_both_hosts() {
        let root = temp_directory("stale-skin-observation");
        create_fixture(&root.join("user/old-skin"), "old-skin");
        let service = create_service(&root);
        for host in [SkinHostKind::Codex, SkinHostKind::WorkBuddy] {
            let generation = service.host_runtime_generation(host);
            drop(service.begin_host_runtime_mutation(host));
            let instance = service.host_instance_from_observation(
                host, resolved_fixture(host), None,
                Ok(Some(super::super::RecoveredSkinIdentity::Exact(SkinReference {
                    source: SkinSource::User, id: "old-skin".into(),
                }))), generation,
            ).await;
            assert!(instance.active_skin.is_none());
        }
        assert!(service.runtime.lock().await.instances.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 瞬时 CDP 故障保持不确定状态，不能把失败探针解释成空皮肤并抹掉恢复态。
    #[tokio::test]
    async fn failed_probe_preserves_recovered_runtime() {
        let root = temp_directory("failed-skin-observation");
        create_fixture(&root.join("user/old-skin"), "old-skin");
        let service = create_service(&root);
        for host in [SkinHostKind::Codex, SkinHostKind::WorkBuddy] {
            let resolved = resolved_fixture(host);
            let key = super::super::runtime_instance_key(host, &resolved.id);
            let descriptor = load_descriptor(&root.join("user/old-skin"), "old-skin", SkinSource::User)
                .unwrap();
            service.runtime.lock().await.instances.insert(key.clone(), super::super::InstanceRuntime {
                active: Some(descriptor), compatibility: None, task: None,
            });
            let instance = service.host_instance_from_observation(
                host, resolved, None, Err(AppError::new("skin.cdp_failed", "test")),
                service.host_runtime_generation(host),
            ).await;
            assert_eq!(instance.state, CodexRuntimeState::RunningWithoutCdp);
            assert!(instance.active_skin.is_some());
            assert!(service.runtime.lock().await.instances.contains_key(&key));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 一个成功页面不能掩盖另一个已验证页面的注入失败，零注入但已存在皮肤仍可提交。
    #[test]
    fn initial_injection_rejects_partial_or_entire_verified_page_failure() {
        for injected_pages in [0, 1] {
            let mut report = InjectionReport {
                verified_pages: 2, injected_pages, failed_pages: 1,
                ..InjectionReport::default()
            };
            assert_eq!(super::super::validate_initial_injection_report(&mut report).unwrap_err().code,
                "skin.injection_failed");
        }
        for injected_pages in [0, 2] {
            assert!(super::super::validate_initial_injection_report(&mut InjectionReport {
                verified_pages: 2, injected_pages, ..InjectionReport::default()
            }).is_ok());
        }
    }

    /// 页面已验证后真正的 CDP 故障继续使用原稳定码，保留 WorkBuddy 的恢复提示语义。
    #[test]
    fn initial_injection_preserves_connection_failure_code() {
        let mut report = InjectionReport {
            verified_pages: 1,
            failed_pages: 1,
            first_failure: Some(AppError::new("skin.cdp_request_timeout", "timeout")),
            ..InjectionReport::default()
        };
        assert_eq!(super::super::validate_initial_injection_report(&mut report).unwrap_err().code,
            "skin.cdp_request_timeout");
    }
}
