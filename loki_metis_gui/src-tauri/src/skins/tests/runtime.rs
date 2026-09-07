    #[test]
    /// 验证换皮迁移中的 `empty_dual_source_library_is_valid` 回归场景。
    fn empty_dual_source_library_is_valid() {
        let root = temp_directory("empty");
        let service = create_service(&root);
        assert!(service.list_skins().expect("空资源库应可扫描").is_empty());
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `creation_prompt_uses_exact_user_and_bundled_skill_paths` 回归场景。
    fn creation_prompt_uses_exact_user_and_bundled_skill_paths() {
        let root = temp_directory("creation-prompt");
        let skill_root = root.join("resources/codex-skin-generator");
        std::fs::create_dir_all(skill_root.join("scripts")).expect("应创建 Skill 脚本目录");
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\nname: codex-skin-generator\n---",
        )
        .expect("应创建 Skill 入口");
        std::fs::write(skill_root.join("scripts/validate_skin.mjs"), "").expect("应创建正式校验器");
        let service = create_service(&root);

        let prompt = service
            .skin_creation_prompt(&skill_root)
            .expect("完整的安装包 Skill 应生成提示词")
            .prompt;

        assert!(prompt.contains(
            &serde_json::to_string(&root.join("user").to_string_lossy()).expect("路径应编码")
        ));
        assert!(prompt.contains(
            &serde_json::to_string(&skill_root.join("SKILL.md").to_string_lossy())
                .expect("路径应编码")
        ));
        assert!(prompt.contains("不要询问输出目录，不要制作 ZIP"));
        assert!(prompt.contains("1. 主题名称："));
        assert!(prompt.contains("4. 期望风格："));
        assert!(!prompt.contains("5. 主色："));
        assert!(prompt.contains("使用现有图片背景还是 AI 生成背景？"));
        assert!(prompt.contains("默认创建 schemaVersion 3 浅色与深色双模式纯主题"));
        assert!(prompt.contains("默认共用一张且不要主动询问"));
        assert!(prompt.contains("点号开头的隐藏草稿目录"));
        assert!(prompt.contains("在 ID 后追加 _1、_2"));
        assert!(prompt.contains("立即把它重命名回点号开头的隐藏草稿目录"));
        assert!(prompt.contains("回到 LokiMetis 的“换皮”页面"));
        assert!(!prompt.contains("--skin-id"));
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `creation_prompt_rejects_incomplete_or_out_of_scope_skill_files` 回归场景。
    fn creation_prompt_rejects_incomplete_or_out_of_scope_skill_files() {
        let root = temp_directory("creation-prompt-invalid");
        let skill_root = root.join("resources/codex-skin-generator");
        std::fs::create_dir_all(&skill_root).expect("应创建 Skill 目录");
        std::fs::write(skill_root.join("SKILL.md"), "skill").expect("应创建 Skill 入口");
        let service = create_service(&root);

        assert_eq!(
            service
                .skin_creation_prompt(&skill_root)
                .expect_err("缺少正式校验器必须拒绝")
                .code,
            "skin.prompt_unavailable"
        );
        assert_eq!(
            validate_bundled_skill_file(&skill_root, Path::new("../SKILL.md"))
                .expect_err("越界相对路径必须拒绝")
                .code,
            "skin.prompt_unavailable"
        );
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `account_profile_probe_is_read_only_and_bounded` 回归场景。
    fn account_profile_probe_is_read_only_and_bounded() {
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("button[aria-haspopup"));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("aria-haspopup=\"menu\""));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("Open profile menu"));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("image?.currentSrc"));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("data:image"));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("avatarDataUrl"));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("__CODEX_DREAM_SKIN_STATE__"));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("activeSkin"));
        assert!(ACCOUNT_PROFILE_PROBE_SCRIPT.contains("label.length <= 80"));
        for forbidden in [
            "fetch(",
            "new Image",
            "captureScreenshot",
            "getBoundingClientRect",
            "canvas",
            ".src =",
            ".click(",
            ".focus(",
            "scrollIntoView",
            "localStorage",
            "document.cookie",
            "email",
            "token",
        ] {
            assert!(!ACCOUNT_PROFILE_PROBE_SCRIPT.contains(forbidden));
        }
    }

    #[test]
    /// 验证换皮迁移中的 `skin_runtime_marker_and_current_check_use_exact_identity` 回归场景。
    fn skin_runtime_marker_and_current_check_use_exact_identity() {
        let root = temp_directory("skin-runtime-marker");
        create_fixture(&root.join("user/first-skin"), "first-skin");
        let reference = SkinReference {
            source: SkinSource::User,
            id: "first-skin".into(),
        };
        let loaded = load_skin(&root.join("builtin"), &root.join("user"), &reference)
            .expect("应构建带运行标记的皮肤载荷");
        assert!(loaded.payload.contains("window.__CODEX_DREAM_SKIN_STATE__"));
        assert!(loaded.payload.contains("state.skin"));
        assert!(loaded.payload.contains("\"source\":\"user\""));
        assert!(loaded.payload.contains("\"id\":\"first-skin\""));
        assert!(loaded.payload.contains("\"name\":\"测试皮肤\""));

        let expression = current_skin_expression(&reference);
        assert!(expression.contains("state?.version === \"1.7.0\""));
        assert!(expression.contains("skin?.source === \"user\""));
        assert!(expression.contains("skin?.id === \"first-skin\""));
        assert!(!expression.contains("themeId"));
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `recovered_legacy_skin_requires_unique_catalog_identity` 回归场景。
    fn recovered_legacy_skin_requires_unique_catalog_identity() {
        let root = temp_directory("recovered-legacy-skin");
        create_fixture(&root.join("user/shared-skin"), "shared-skin");
        create_fixture(&root.join("builtin/shared-skin"), "shared-skin");
        create_fixture(&root.join("user/unique-skin"), "unique-skin");
        create_theme_css_fixture(&root.join("user/legacy-theme"), "legacy-theme");
        let service = SkinService::new(root.join("builtin"), root.join("user"));

        assert!(service
            .resolve_recovered_skin(&super::RecoveredSkinIdentity::LegacyId(
                "shared-skin".into()
            ))
            .is_none());
        let unique = service
            .resolve_recovered_skin(&super::RecoveredSkinIdentity::LegacyId(
                "unique-skin".into(),
            ))
            .expect("历史 themeId 唯一映射时应恢复皮肤");
        assert_eq!(unique.source, SkinSource::User);
        let exact = service
            .resolve_recovered_skin(&super::RecoveredSkinIdentity::Exact(SkinReference {
                source: SkinSource::User,
                id: "shared-skin".into(),
            }))
            .expect("精确来源与 ID 应恢复用户皮肤");
        assert_eq!(exact.source, SkinSource::User);
        assert_eq!(exact.id, "shared-skin");
        let manifest =
            super::read_manifest(&root.join("user/legacy-theme")).expect("应读取旧载荷主题清单");
        let super::SkinManifest::ThemeCss(theme) = manifest else {
            panic!("测试夹具应为 v3 主题");
        };
        let config = super::read_theme_css_config(
            &root.join("user/legacy-theme"),
            theme.appearance.as_ref(),
        )
        .expect("应构建主题运行 CSS");
        let recovered_theme = service
            .resolve_recovered_skin(&super::RecoveredSkinIdentity::LegacyThemeCss(format!(
                "{}\n{}",
                super::THEME_RUNTIME_CSS,
                config.css_text,
            )))
            .expect("旧 v3 样式唯一匹配时应恢复主题");
        assert_eq!(recovered_theme.id, "legacy-theme");
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `active_skin_probe_prefers_exact_marker_and_validates_legacy_id` 回归场景。
    fn active_skin_probe_prefers_exact_marker_and_validates_legacy_id() {
        let exact = super::recovered_skin_identity(Some(super::ActiveSkinProbe {
            version: Some("1.7.0".into()),
            source: Some(SkinSource::Builtin),
            id: Some("minecraft".into()),
            legacy_theme_id: None,
            legacy_style_text: None,
        }));
        assert_eq!(
            exact,
            Some(super::RecoveredSkinIdentity::Exact(SkinReference {
                source: SkinSource::Builtin,
                id: "minecraft".into(),
            }))
        );
        assert_eq!(
            super::recovered_skin_identity(Some(super::ActiveSkinProbe {
                version: Some("1.7.0".into()),
                source: None,
                id: None,
                legacy_theme_id: Some("xiamizi".into()),
                legacy_style_text: None,
            })),
            Some(super::RecoveredSkinIdentity::LegacyId("xiamizi".into()))
        );
        assert!(super::recovered_skin_identity(Some(super::ActiveSkinProbe {
            version: Some("1.7.0".into()),
            source: None,
            id: None,
            legacy_theme_id: Some("../escape".into()),
            legacy_style_text: None,
        }))
        .is_none());
    }

    #[test]
    /// 验证换皮迁移中的 `window_activation_target_requires_one_exact_debug_endpoint` 回归场景。
    fn window_activation_target_requires_one_exact_debug_endpoint() {
        let instance = |pid, port| {
            super::resolved_instance(PlatformCodexProcess {
                pid,
                executable: PathBuf::from("Codex.exe"),
                command_line: format!("Codex.exe --remote-debugging-port={port}"),
            })
        };
        let unique = vec![instance(41, 9341), instance(42, 9342)];
        assert_eq!(
            unique_codex_pid_for_endpoint(&unique, CdpEndpoint::new(9342)),
            Some(42)
        );
        assert_eq!(
            unique_codex_pid_for_endpoint(&unique, CdpEndpoint::new(9999)),
            None
        );

        let ambiguous = vec![instance(42, 9342), instance(43, 9342)];
        assert_eq!(
            unique_codex_pid_for_endpoint(&ambiguous, CdpEndpoint::new(9342)),
            None
        );
    }

    #[tokio::test]
    /// 验证换皮迁移中的 `account_profile_probe_is_initialized_once_per_instance` 回归场景。
    async fn account_profile_probe_is_initialized_once_per_instance() {
        let root = temp_directory("account-profile-probe-cache");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let first = service
            .account_profile_probe_cell("same-instance")
            .expect("应创建账户资料探测槽");
        let second = service
            .account_profile_probe_cell("same-instance")
            .expect("应复用账户资料探测槽");
        let other = service
            .account_profile_probe_cell("other-instance")
            .expect("应为另一实例创建独立探测槽");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &other));

        let initializations = Arc::new(AtomicUsize::new(0));
        let first_count = initializations.clone();
        let second_count = initializations.clone();
        let (first_result, second_result) = tokio::join!(
            first.get_or_try_init(|| async move {
                first_count.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok::<Option<super::AccountProfile>, &'static str>(None)
            }),
            second.get_or_try_init(|| async move {
                second_count.fetch_add(1, Ordering::SeqCst);
                Ok::<Option<super::AccountProfile>, &'static str>(None)
            }),
        );
        assert_eq!(initializations.load(Ordering::SeqCst), 1);
        assert!(matches!(first_result, Ok(None)));
        assert!(matches!(second_result, Ok(None)));

        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[tokio::test]
    /// 验证换皮迁移中的 `account_profile_probe_does_not_cache_transient_failures` 回归场景。
    async fn account_profile_probe_does_not_cache_transient_failures() {
        let root = temp_directory("account-profile-probe-retry");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let probe = service
            .account_profile_probe_cell("retry-instance")
            .expect("应创建账户资料探测槽");
        let attempts = AtomicUsize::new(0);

        let failed = probe
            .get_or_try_init(|| async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err::<Option<super::AccountProfile>, _>("transient")
            })
            .await;
        assert!(matches!(failed, Err("transient")));
        assert!(probe.get().is_none());

        let recovered = probe
            .get_or_try_init(|| async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Ok::<Option<super::AccountProfile>, &'static str>(None)
            })
            .await;
        assert!(matches!(recovered, Ok(None)));
        let cached = probe
            .get_or_try_init(|| async {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err::<Option<super::AccountProfile>, _>("should-not-run")
            })
            .await;
        assert!(matches!(cached, Ok(None)));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);

        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `scanned_instance_exposes_process_before_account_profile_probe` 回归场景。
    fn scanned_instance_exposes_process_before_account_profile_probe() {
        let ready = scanned_codex_instance(super::resolved_instance(PlatformCodexProcess {
            pid: 42,
            executable: PathBuf::from("/Applications/ChatGPT"),
            command_line: "ChatGPT --user-data-dir=/profiles/personal --remote-debugging-port=9341"
                .into(),
        }));
        assert_eq!(ready.state, CodexRuntimeState::Ready);
        assert_eq!(ready.debug_port, Some(9341));
        assert_eq!(ready.account_label, None);
        assert_eq!(ready.avatar_data_url, None);

        let unavailable = scanned_codex_instance(super::resolved_instance(PlatformCodexProcess {
            pid: 43,
            executable: PathBuf::from("/Applications/ChatGPT"),
            command_line: "ChatGPT --user-data-dir=/profiles/company".into(),
        }));
        assert_eq!(unavailable.state, CodexRuntimeState::RunningWithoutCdp);
        assert_eq!(unavailable.debug_port, None);
        assert_eq!(unavailable.account_label, None);
        assert_eq!(unavailable.avatar_data_url, None);
    }

    #[test]
    /// 验证换皮迁移中的 `restarted_endpoint_uses_proven_ready_scan_without_profile_probe_state` 回归场景。
    fn restarted_endpoint_uses_proven_ready_scan_without_profile_probe_state() {
        let restarted = scanned_codex_instance(super::resolved_instance(PlatformCodexProcess {
            pid: 52,
            executable: PathBuf::from("/Applications/ChatGPT"),
            command_line: "ChatGPT --remote-debugging-port=9342".into(),
        }));
        let other = scanned_codex_instance(super::resolved_instance(PlatformCodexProcess {
            pid: 51,
            executable: PathBuf::from("/Applications/ChatGPT"),
            command_line: "ChatGPT --remote-debugging-port=9341".into(),
        }));
        let selected =
            restarted_instance_for_endpoint(vec![other, restarted], CdpEndpoint::new(9342))
                .expect("已成功连接的重启端口应返回对应实例");
        assert_eq!(selected.pid, 52);
        assert_eq!(selected.state, CodexRuntimeState::Ready);
        assert_eq!(selected.account_label, None);
        assert_eq!(selected.avatar_data_url, None);
    }

    #[test]
    /// 验证换皮迁移中的 `account_avatar_accepts_only_small_raster_data_urls` 回归场景。
    fn account_avatar_accepts_only_small_raster_data_urls() {
        let valid = "data:image/png;base64,AQID";
        assert_eq!(
            validated_account_avatar(Some(valid)).as_deref(),
            Some(valid)
        );
        assert!(validated_account_avatar(Some("https://example.com/avatar.png")).is_none());
        assert!(validated_account_avatar(Some("data:image/svg+xml;base64,PHN2Zz4=")).is_none());
        assert!(validated_account_avatar(Some("data:image/png;base64,@@@")).is_none());
        let oversized = format!(
            "data:image/webp;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(vec![0_u8; 256 * 1024 + 1])
        );
        assert!(validated_account_avatar(Some(&oversized)).is_none());
    }

    #[test]
    /// 验证换皮迁移中的 `codex_instance_serializes_avatar_and_exact_active_skin` 回归场景。
    fn codex_instance_serializes_avatar_and_exact_active_skin() -> Result<(), serde_json::Error> {
        let value = serde_json::to_value(CodexInstance {
            id: "instance-1".into(),
            pid: 42,
            label: "Codex · company".into(),
            profile: Some("company".into()),
            state: CodexRuntimeState::Ready,
            debug_port: Some(9341),
            active_skin_name: Some("共享名称".into()),
            active_skin: Some(SkinReference {
                source: SkinSource::User,
                id: "exact-theme".into(),
            }),
            account_label: Some("公司账号".into()),
            avatar_data_url: Some("data:image/png;base64,AQID".into()),
        })?;
        assert_eq!(
            value["activeSkin"],
            json!({ "source": "user", "id": "exact-theme" })
        );
        assert_eq!(value["avatarDataUrl"], "data:image/png;base64,AQID");
        Ok(())
    }
