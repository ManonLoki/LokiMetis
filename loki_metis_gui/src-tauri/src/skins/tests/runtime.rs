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
        let loaded = load_skin(&root.join("builtin"), &root.join("user"), &reference, true)
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
    /// 未确认第三方代码时应用流程必须在宿主探测、注入和运行态持久化之前失败。
    fn legacy_install_without_consent_stops_before_host_side_effects() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("legacy-install-consent");
            let directory = root.join("user/legacy-script");
            create_fixture(&directory, "legacy-script");
            let service = create_service(&root);
            let reference = SkinReference {
                source: SkinSource::User,
                id: "legacy-script".into(),
            };
            let script_before = std::fs::read(directory.join("renderer-inject.js"))
                .expect("应读取兼容皮肤脚本");

            let error = service
                .install(
                    SkinHostKind::Codex,
                    &reference,
                    false,
                    None,
                    false,
                    false,
                )
                .await
                .expect_err("未确认信任时不得接触宿主");
            assert_eq!(error.code, "skin.third_party_code_consent_required");
            assert!(service.runtime.lock().await.instances.is_empty());
            assert_eq!(
                std::fs::read(directory.join("renderer-inject.js"))
                    .expect("拒绝后原始脚本应保持不变"),
                script_before
            );

            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
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

    #[cfg(target_os = "windows")]
    #[tokio::test]
    /// Windows WorkBuddy 恢复后，新 PID 安装必须回收该宿主全部旧监视任务并保留 Codex 状态。
    async fn windows_workbuddy_install_replaces_old_pid_watchers() {
        let watch_task = |host| {
            let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
            let join = tokio::spawn(async move {
                let _ = cancelled.changed().await;
                Ok::<usize, super::AppError>(0)
            });
            let handler_abort = join.abort_handle();
            super::WatchTask {
                host,
                cancel,
                join,
                handler_abort,
                endpoint: super::CdpEndpoint::default(),
            }
        };
        let mut runtime = super::RuntimeState::default();
        for key in ["workBuddy:old-pid", "workBuddy:new-pid"] {
            runtime.instances.insert(
                key.into(),
                super::InstanceRuntime {
                    active: None,
                    compatibility: None,
                    task: Some(watch_task(super::SkinHostKind::WorkBuddy)),
                },
            );
        }
        runtime.instances.insert(
            "codex:preserved".into(),
            super::InstanceRuntime {
                active: None,
                compatibility: None,
                task: None,
            },
        );

        let tasks = super::take_replaced_watch_tasks(
            &mut runtime,
            super::SkinHostKind::WorkBuddy,
            "workBuddy:new-pid",
        );

        assert_eq!(tasks.len(), 2);
        assert!(runtime
            .instances
            .keys()
            .all(|key| !super::runtime_instance_belongs_to_host(
                super::SkinHostKind::WorkBuddy,
                key,
            )));
        assert!(runtime.instances.contains_key("codex:preserved"));
        for task in tasks {
            task.handler_abort.abort();
            task.join.abort();
        }

        runtime.instances.insert(
            "workBuddy:stale-last-target".into(),
            super::InstanceRuntime {
                active: None,
                compatibility: None,
                task: Some(watch_task(super::SkinHostKind::WorkBuddy)),
            },
        );
        runtime.last_targets.insert(
            super::SkinHostKind::WorkBuddy,
            "workBuddy:stale-last-target".into(),
        );
        let uninstall_tasks = super::take_uninstall_watch_tasks(
            &mut runtime,
            super::SkinHostKind::WorkBuddy,
            Some("workBuddy:current-pid"),
        );
        assert_eq!(uninstall_tasks.len(), 1);
        assert!(!runtime
            .last_targets
            .contains_key(&super::SkinHostKind::WorkBuddy));
        assert!(runtime
            .instances
            .keys()
            .all(|key| !super::runtime_instance_belongs_to_host(
                super::SkinHostKind::WorkBuddy,
                key,
            )));
        for task in uninstall_tasks {
            task.handler_abort.abort();
            task.join.abort();
        }
    }

    #[tokio::test]
    /// 枚举结果不再包含旧 PID 时，应删除已结束任务，但仍保留尚在清理中的任务。
    async fn recovered_runtime_prunes_finished_old_pid_watcher() {
        let root = temp_directory("prune-finished-workbuddy-watcher");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let (finished_cancel, _) = tokio::sync::watch::channel(false);
        let finished_join = tokio::spawn(async { Ok::<usize, super::AppError>(0) });
        while !finished_join.is_finished() {
            tokio::task::yield_now().await;
        }
        let finished_abort = finished_join.abort_handle();
        let (running_cancel, mut running_cancelled) = tokio::sync::watch::channel(false);
        let running_join = tokio::spawn(async move {
            let _ = running_cancelled.changed().await;
            Ok::<usize, super::AppError>(0)
        });
        let running_abort = running_join.abort_handle();
        {
            let mut runtime = service.runtime.lock().await;
            runtime.instances.insert(
                "workBuddy:finished-old-pid".into(),
                super::InstanceRuntime {
                    active: None,
                    compatibility: None,
                    task: Some(super::WatchTask {
                        host: super::SkinHostKind::WorkBuddy,
                        cancel: finished_cancel,
                        join: finished_join,
                        handler_abort: finished_abort,
                        endpoint: super::CdpEndpoint::default(),
                    }),
                },
            );
            runtime.instances.insert(
                "workBuddy:running-old-pid".into(),
                super::InstanceRuntime {
                    active: None,
                    compatibility: None,
                    task: Some(super::WatchTask {
                        host: super::SkinHostKind::WorkBuddy,
                        cancel: running_cancel,
                        join: running_join,
                        handler_abort: running_abort,
                        endpoint: super::CdpEndpoint::default(),
                    }),
                },
            );
        }

        service
            .retain_recovered_instance_runtimes(
                super::SkinHostKind::WorkBuddy,
                std::iter::empty::<&str>(),
            )
            .await;

        let running_task = {
            let mut runtime = service.runtime.lock().await;
            assert!(!runtime.instances.contains_key("workBuddy:finished-old-pid"));
            runtime
                .instances
                .remove("workBuddy:running-old-pid")
                .and_then(|instance| instance.task)
                .expect("尚未结束的监视任务必须保留")
        };
        running_task.handler_abort.abort();
        running_task.join.abort();
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[tokio::test]
    /// 跨越一次显式宿主变更才返回的旧页面探针，不得重新写回已经失效的活动皮肤。
    async fn recovered_runtime_rejects_probe_from_previous_generation() {
        let root = temp_directory("reject-stale-runtime-probe");
        create_fixture(&root.join("user/old-skin"), "old-skin");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let descriptor = load_descriptor(
            &root.join("user/old-skin"),
            "old-skin",
            SkinSource::User,
        )
        .expect("应读取旧皮肤测试描述");
        let observed_generation = service.host_runtime_generation(SkinHostKind::WorkBuddy);
        let mutation = service.begin_host_runtime_mutation(SkinHostKind::WorkBuddy);
        drop(mutation);

        service
            .reconcile_recovered_instance_runtime(
                SkinHostKind::WorkBuddy,
                "workbuddy-current",
                Some(descriptor),
                observed_generation,
            )
            .await;

        assert!(service.runtime.lock().await.instances.is_empty());
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[tokio::test]
    /// 当前 PID 的监视任务已结束且页面已无皮肤时，页面探针必须清掉旧活动状态。
    async fn recovered_runtime_none_clears_finished_current_pid_watcher() {
        let root = temp_directory("clear-finished-current-runtime");
        create_fixture(&root.join("user/old-skin"), "old-skin");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let descriptor = load_descriptor(
            &root.join("user/old-skin"),
            "old-skin",
            SkinSource::User,
        )
        .expect("应读取旧皮肤测试描述");
        let (cancel, _) = tokio::sync::watch::channel(false);
        let join = tokio::spawn(async { Ok::<usize, super::AppError>(0) });
        while !join.is_finished() {
            tokio::task::yield_now().await;
        }
        let handler_abort = join.abort_handle();
        let key = super::runtime_instance_key(SkinHostKind::WorkBuddy, "workbuddy-current");
        {
            let mut runtime = service.runtime.lock().await;
            runtime.instances.insert(
                key.clone(),
                super::InstanceRuntime {
                    active: Some(descriptor),
                    compatibility: None,
                    task: Some(super::WatchTask {
                        host: SkinHostKind::WorkBuddy,
                        cancel,
                        join,
                        handler_abort,
                        endpoint: super::CdpEndpoint::default(),
                    }),
                },
            );
        }
        let observed_generation = service.host_runtime_generation(SkinHostKind::WorkBuddy);

        service
            .reconcile_recovered_instance_runtime(
                SkinHostKind::WorkBuddy,
                "workbuddy-current",
                None,
                observed_generation,
            )
            .await;

        assert!(!service.runtime.lock().await.instances.contains_key(&key));
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }
