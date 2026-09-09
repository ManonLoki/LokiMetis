    #[test]
    /// 验证换皮迁移中的 `builtin_and_user_skin_with_same_id_coexist` 回归场景。
    fn builtin_and_user_skin_with_same_id_coexist() {
        let root = temp_directory("same-id");
        create_fixture(&root.join("builtin/shared"), "shared");
        create_fixture(&root.join("user/shared"), "shared");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let skins = service.list_skins().expect("应扫描双来源");
        assert_eq!(skins.len(), 2);
        assert_eq!(skins[0].source, SkinSource::User);
        assert_eq!(skins[1].source, SkinSource::Builtin);
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `schema_v2_theme_is_rejected_with_conversion_guidance` 回归场景。
    fn schema_v2_theme_is_rejected_with_conversion_guidance() {
        let root = temp_directory("deprecated-theme");
        let directory = root.join("theme-example");
        create_theme_fixture(&directory, "theme-example");
        let error = load_descriptor(&directory, "theme-example", SkinSource::User)
            .expect_err("schemaVersion 2 不再属于受支持格式");
        assert_eq!(error.code, "skin.import_unsupported");
        assert!(error.details[0].contains("schemaVersion 3"));
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }
    #[test]
    /// 验证换皮迁移中的 `css_variable_theme_is_appended_after_runtime_css` 回归场景。
    fn css_variable_theme_is_appended_after_runtime_css() {
        let root = temp_directory("theme-css-cascade");
        let directory = root.join("theme-css-example");
        create_theme_css_fixture(&directory, "theme-css-example");
        let descriptor = load_descriptor(&directory, "theme-css-example", SkinSource::User)
            .expect("CSS 变量主题应通过严格校验");
        assert_eq!(descriptor.package_type, SkinPackageType::Theme);
        let payload =
            build_payload(&directory, false).expect("CSS 变量主题应构建统一载荷");
        let runtime_index = payload
            .find("--skin-radius: 12px")
            .expect("应包含运行时默认值");
        let theme_index = payload
            .find("--skin-radius: 21px")
            .expect("应包含主题覆盖值");
        assert!(
            theme_index > runtime_index,
            "主题 CSS 必须位于统一 CSS 之后"
        );
        assert!(payload.contains("--skin-background-image"));
        assert!(THEME_RUNTIME_CSS.contains("#root :where(div):has(> [role=\"menubar\"])"));
        assert!(THEME_RUNTIME_CSS.contains("[data-app-shell-focus-area=\"main\"] > div > div"));
        assert!(THEME_RUNTIME_CSS.contains("section div.border-default:has([role=\"switch\"])"));
        assert!(THEME_RUNTIME_CSS.contains("[role=\"switch\"] > span:first-child > span"));
        assert!(!payload.contains("__DREAM_SKIN_"));
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 兼容皮肤载荷在读取并拼接自由脚本前必须取得本次操作的显式信任。
    fn legacy_payload_requires_explicit_third_party_code_consent() {
        let root = temp_directory("legacy-payload-consent");
        let directory = root.join("legacy-script");
        create_fixture(&directory, "legacy-script");

        let error = build_payload(&directory, false)
            .expect_err("未确认信任时不得构建兼容皮肤脚本载荷");
        assert_eq!(error.code, "skin.third_party_code_consent_required");
        assert!(error.message.contains("renderer-inject.js"));
        assert!(error.message.contains("不能证明脚本安全"));

        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `color_modes_default_to_light_and_explicit_overlays_join_the_payload` 回归场景。
    fn color_modes_default_to_light_and_explicit_overlays_join_the_payload() {
        let root = temp_directory("theme-color-modes");
        let legacy_directory = root.join("legacy-contract");
        create_theme_css_fixture(&legacy_directory, "legacy-contract");
        let legacy = load_descriptor(&legacy_directory, "legacy-contract", SkinSource::User)
            .expect("未声明 appearance 的主题应继续有效");
        assert_eq!(legacy.supported_color_modes, vec![ColorMode::Light]);

        let mode_directory = root.join("mode-contract");
        create_mode_theme_fixture(&mode_directory, "mode-contract");
        let descriptor = load_descriptor(&mode_directory, "mode-contract", SkinSource::User)
            .expect("显式双模式主题应通过校验");
        assert_eq!(
            descriptor.supported_color_modes,
            vec![ColorMode::Light, ColorMode::Dark]
        );
        let payload =
            build_payload(&mode_directory, false).expect("模式覆盖应加入运行时载荷");
        assert!(payload.contains("--skin-accent: #78F0C6"));
        assert!(payload.contains("data:image/png;base64,"));
        assert!(payload.contains("data:image/jpeg;base64,"));
        assert!(payload.contains("backgrounds"));
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `legacy_color_mode_support_requires_two_complete_palettes` 回归场景。
    fn legacy_color_mode_support_requires_two_complete_palettes() {
        let complete: LegacyManifest = serde_json::from_value(json!({
            "schemaVersion": 1,
            "id": "legacy-dual",
            "name": "双模式旧皮肤",
            "author": "测试作者",
            "image": "qq2007-sky.png",
            "friendCards": {
                "profileImage": "avatar.png",
                "listImage": "qqshow.jpg"
            },
            "colors": {
                "light": {
                    "background": "#FFFFFF", "panel": "#F5F5F5",
                    "panelAlt": "#EEEEEE", "accent": "#006644",
                    "accentAlt": "#0055AA", "text": "#111111",
                    "muted": "#555555", "line": "#AAAAAA"
                },
                "dark": {
                    "background": "#111111", "panel": "#222222",
                    "panelAlt": "#333333", "accent": "#88E0C0",
                    "accentAlt": "#88BBFF", "text": "#FFFFFF",
                    "muted": "#BBBBBB", "line": "#666666"
                }
            }
        }))
        .expect("双模式旧皮肤清单应可解析");
        assert!(legacy_has_complete_dual_palettes(&complete));

        let mut incomplete = complete;
        incomplete
            .colors
            .as_mut()
            .and_then(|colors| colors.get_mut("dark"))
            .and_then(serde_json::Value::as_object_mut)
            .expect("应取得深色调色板")
            .remove("line");
        assert!(!legacy_has_complete_dual_palettes(&incomplete));
    }

    #[test]
    /// 验证换皮迁移中的 `explicit_color_modes_require_matching_safe_css_files` 回归场景。
    fn explicit_color_modes_require_matching_safe_css_files() {
        let root = temp_directory("theme-mode-files");
        let directory = root.join("mode-contract");
        create_mode_theme_fixture(&directory, "mode-contract");
        std::fs::remove_file(directory.join("theme.dark.css")).expect("应删除深色覆盖");
        let missing = load_descriptor(&directory, "mode-contract", SkinSource::User)
            .expect_err("声明深色但缺少文件必须拒绝");
        assert!(missing
            .details
            .iter()
            .any(|detail| detail.contains("theme.dark.css")));

        create_mode_theme_fixture(&directory, "mode-contract");
        std::fs::write(
            directory.join("theme.dark.css"),
            r#"html.codex-dream-skin[data-dream-shell="dark"] { --skin-background-image: url("../other.png"); }"#,
        )
        .expect("应写入非法图片覆盖");
        let image = load_descriptor(&directory, "mode-contract", SkinSource::User)
            .expect_err("模式 CSS 非安全图片路径必须拒绝");
        assert!(image.message.contains("theme.dark.css"));
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `appearance_check_distinguishes_mismatch_and_unreadable_fields` 回归场景。
    fn appearance_check_distinguishes_mismatch_and_unreadable_fields() {
        let policy = AppearancePolicy {
            supported_color_modes: vec![ColorMode::Light, ColorMode::Dark],
            requirements: Some(super::AppearanceRequirements {
                dark: Some(super::AppearanceRequirement {
                    code_theme_id: Some("codex".into()),
                    accent: Some("#339CFF".into()),
                    opaque_windows: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        };
        let probe = AppearanceProbe {
            effective_mode: ColorMode::Dark,
            effective: json!({ "accent": "#339cff" }),
            appearance_readable: false,
            appearance: serde_json::Value::Null,
        };
        let check = build_appearance_check(&policy, &probe);
        assert!(!check
            .differences
            .iter()
            .any(|item| item.field == "colorMode"));
        assert!(!check.differences.iter().any(|item| item.field == "accent"));
        assert!(check
            .unreadable
            .iter()
            .any(|item| item.field == "codeThemeId"));
        assert!(check
            .unreadable
            .iter()
            .any(|item| item.field == "opaqueWindows"));
        let mode_check = build_appearance_check(
            &AppearancePolicy {
                supported_color_modes: vec![ColorMode::Light],
                requirements: None,
            },
            &probe,
        );
        assert!(mode_check
            .differences
            .iter()
            .any(|item| item.field == "colorMode"));
    }

    #[test]
    /// 验证换皮迁移中的 `css_variable_theme_rejects_selectors_properties_and_remote_urls` 回归场景。
    fn css_variable_theme_rejects_selectors_properties_and_remote_urls() {
        let root = temp_directory("theme-css-invalid");
        let directory = root.join("theme-css-example");
        create_theme_css_fixture(&directory, "theme-css-example");
        let path = directory.join("theme.css");
        let original = std::fs::read_to_string(&path).expect("应读取变量表");
        std::fs::write(&path, format!("{original}\nbody {{ --skin-bg: #000000; }}"))
            .expect("应写入非法选择器");
        assert_eq!(
            load_descriptor(&directory, "theme-css-example", SkinSource::User)
                .expect_err("宿主选择器必须被拒绝")
                .code,
            "skin.assets_invalid"
        );
        create_theme_css_fixture(&directory, "theme-css-example");
        let css = std::fs::read_to_string(&path)
            .expect("应读取变量表")
            .replace("--skin-radius: 21px;", "border-radius: 21px;");
        std::fs::write(&path, css).expect("应写入普通属性");
        assert_eq!(
            load_descriptor(&directory, "theme-css-example", SkinSource::User)
                .expect_err("普通 CSS 属性必须被拒绝")
                .code,
            "skin.assets_invalid"
        );
        create_theme_css_fixture(&directory, "theme-css-example");
        let css = std::fs::read_to_string(&path)
            .expect("应读取变量表")
            .replace(
                "url(\"background.png\")",
                "url(\"https://example.com/a.png\")",
            );
        std::fs::write(&path, css).expect("应写入网络图片");
        assert_eq!(
            load_descriptor(&directory, "theme-css-example", SkinSource::User)
                .expect_err("网络图片必须被拒绝")
                .code,
            "skin.assets_invalid"
        );
        create_theme_css_fixture(&directory, "theme-css-example");
        let css = std::fs::read_to_string(&path)
            .expect("应读取变量表")
            .replace(
                "--skin-radius: 21px;",
                "--skin-radius: 21px; --skin-radius: 8px;",
            );
        std::fs::write(&path, css).expect("应写入重复变量");
        assert_eq!(
            load_descriptor(&directory, "theme-css-example", SkinSource::User)
                .expect_err("重复变量必须被拒绝")
                .code,
            "skin.assets_invalid"
        );
        create_theme_css_fixture(&directory, "theme-css-example");
        std::fs::write(directory.join("renderer-inject.js"), "alert(1)")
            .expect("应写入混合格式脚本");
        assert_eq!(
            load_descriptor(&directory, "theme-css-example", SkinSource::User)
                .expect_err("CSS 变量主题不得混入脚本")
                .code,
            "skin.assets_invalid"
        );
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `repeated_zip_installs_use_numbered_ids_and_names` 回归场景。
    fn repeated_zip_installs_use_numbered_ids_and_names() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("catalog-invalidation");
            let fixture = root.join("fixture");
            create_fixture(&fixture, "test-skin");
            let archive = root.join("skin.zip");
            create_zip(&archive, &fixture, Some("wrapper"));
            let service = create_service(&root);

            assert!(service.list_skins().expect("初始资源库应为空").is_empty());
            let prepared = service
                .prepare_test_import_batch(&archive)
                .expect("有效 ZIP 应通过预检");
            assert_eq!(prepared.items[0].skin.id, "test-skin");
            assert_eq!(prepared.items[0].skin.name, "测试皮肤");
            service
                .commit_test_import_batch(&prepared.token)
                .await
                .expect("首次安装应成功");
            assert_eq!(service.list_skins().expect("应读取新导入皮肤").len(), 1);

            let second = service
                .prepare_test_import_batch(&archive)
                .expect("重复 ZIP 应自动编号");
            assert_eq!(second.items[0].skin.id, "test-skin_1");
            assert_eq!(second.items[0].skin.name, "测试皮肤(1)");
            service
                .commit_test_import_batch(&second.token)
                .await
                .expect("第二份应独立安装");

            let third = service
                .prepare_test_import_batch(&archive)
                .expect("再次重复 ZIP 应继续编号");
            assert_eq!(third.items[0].skin.id, "test-skin_2");
            assert_eq!(third.items[0].skin.name, "测试皮肤(2)");
            service
                .commit_test_import_batch(&third.token)
                .await
                .expect("第三份应独立安装");
            let installed = service.list_skins().expect("应读取三份用户皮肤");
            assert_eq!(installed.len(), 3);
            assert!(installed.iter().any(|skin| skin.id == "test-skin"));
            assert!(installed.iter().any(|skin| skin.id == "test-skin_1"));
            assert!(installed.iter().any(|skin| skin.id == "test-skin_2"));

            for id in ["test-skin", "test-skin_1", "test-skin_2"] {
                service
                    .delete(&SkinReference {
                        source: SkinSource::User,
                        id: id.into(),
                    })
                    .await
                    .expect("每份用户皮肤都应可独立删除");
            }
            assert!(service
                .list_skins()
                .expect("删除后应重新扫描资源库")
                .is_empty());

            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }
    #[test]
    /// 验证换皮迁移中的 `batch_prepare_numbers_duplicates_and_summarizes_invalid_files` 回归场景。
    fn batch_prepare_numbers_duplicates_and_summarizes_invalid_files() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("batch-prepare");
            let fixture = root.join("fixture");
            create_fixture(&fixture, "test-skin");
            let first = root.join("first.zip");
            let second = root.join("second.zip");
            create_zip(&first, &fixture, None);
            create_zip(&second, &fixture, None);
            let invalid = root.join("note.txt");
            std::fs::write(&invalid, "不是 ZIP").expect("应创建无效文件");
            let service = create_service(&root);

            let prepared = service
                .prepare_import_batch(vec![first, invalid, second])
                .await
                .expect("混合批次应保留有效项");
            assert_eq!(prepared.total_files, 3);
            assert_eq!(prepared.items.len(), 2);
            assert_eq!(prepared.items[0].skin.id, "test-skin");
            assert_eq!(prepared.items[1].skin.id, "test-skin_1");
            assert_eq!(prepared.items[1].skin.name, "测试皮肤(1)");
            assert_eq!(prepared.skipped.len(), 1);
            assert_eq!(prepared.skipped[0].archive_name, "note.txt");
            service
                .cancel_import(&prepared.token)
                .expect("取消应清理整个批次");
            assert!(std::fs::read_dir(root.join("user"))
                .expect("应读取用户目录")
                .next()
                .is_none());
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }
    #[test]
    /// 验证换皮迁移中的 `batch_prepare_streams_started_ready_and_skipped_events_in_order` 回归场景。
    fn batch_prepare_streams_started_ready_and_skipped_events_in_order() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("batch-progress");
            let fixture = root.join("fixture");
            create_fixture(&fixture, "stream-skin");
            let archive = root.join("stream.zip");
            create_zip(&archive, &fixture, None);
            let invalid = root.join("invalid.txt");
            std::fs::write(&invalid, "不是 ZIP").expect("应创建无效文件");
            let service = create_service(&root);
            let events = Arc::new(StdMutex::new(Vec::new()));
            let captured = Arc::clone(&events);

            let prepared = service
                .prepare_import_batch_with_progress(vec![archive, invalid], move |event| {
                    captured
                        .lock()
                        .expect("应记录进度事件")
                        .push(serde_json::to_value(event).expect("进度事件应可序列化"));
                })
                .await
                .expect("混合批次应完成预检");
            let events = events.lock().expect("应读取进度事件");
            assert_eq!(events.len(), 3);
            assert_eq!(events[0]["type"], "started");
            assert_eq!(events[0]["totalFiles"], 2);
            assert_eq!(events[1]["type"], "itemReady");
            assert_eq!(events[1]["item"]["archiveName"], "stream.zip");
            assert_eq!(events[2]["type"], "itemSkipped");
            assert_eq!(events[2]["item"]["archiveName"], "invalid.txt");
            drop(events);
            service
                .cancel_import(&prepared.token)
                .expect("应清理测试批次");
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `batch_prepare_can_be_cancelled_from_the_started_event` 回归场景。
    fn batch_prepare_can_be_cancelled_from_the_started_event() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("batch-progress-cancel");
            let fixture = root.join("fixture");
            create_fixture(&fixture, "cancel-skin");
            let archive = root.join("cancel.zip");
            create_zip(&archive, &fixture, None);
            let service = Arc::new(create_service(&root));
            let cancel_service = Arc::clone(&service);

            let error = service
                .prepare_import_batch_with_progress(vec![archive.clone()], move |event| {
                    if let SkinImportPreparationEvent::Started { token, .. } = event {
                        cancel_service
                            .cancel_import(&token)
                            .expect("加载期间取消不应等待操作锁");
                    }
                })
                .await
                .expect_err("预检应响应取消");
            assert_eq!(error.code, "skin.import_cancelled");
            assert!(std::fs::read_dir(root.join("user"))
                .expect("应读取用户目录")
                .next()
                .is_none());

            let prepared = service
                .prepare_import_batch(vec![archive])
                .await
                .expect("取消后下一批应立即可用");
            service
                .cancel_import(&prepared.token)
                .expect("应清理后续批次");
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `batch_commit_keeps_selected_items_and_expires_the_batch` 回归场景。
    fn batch_commit_keeps_selected_items_and_expires_the_batch() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("batch-selection");
            let first_fixture = root.join("first-fixture");
            let second_fixture = root.join("second-fixture");
            create_fixture(&first_fixture, "first-skin");
            create_fixture(&second_fixture, "second-skin");
            let first = root.join("first.zip");
            let second = root.join("second.zip");
            create_zip(&first, &first_fixture, None);
            create_zip(&second, &second_fixture, None);
            let service = create_service(&root);
            let prepared = service
                .prepare_import_batch(vec![first, second])
                .await
                .expect("批次应通过预检");

            let result = service
                .commit_import_batch(
                    &prepared.token,
                    &[prepared.items[1].item_id.clone()],
                    true,
                )
                .await
                .expect("选中项应成功提交");
            assert_eq!(result.installed.len(), 1);
            assert_eq!(result.installed[0].id, "second-skin");
            assert!(!root.join("user/first-skin").exists());
            assert!(root.join("user/second-skin/theme.json").is_file());
            assert_eq!(
                service
                    .commit_import_batch(
                        &prepared.token,
                        &[prepared.items[0].item_id.clone()],
                        true,
                    )
                    .await
                    .expect_err("完成后的批次令牌必须失效")
                    .code,
                "skin.import_expired"
            );
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `batch_commit_preserves_success_when_another_destination_conflicts` 回归场景。
    fn batch_commit_preserves_success_when_another_destination_conflicts() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("batch-partial");
            let first_fixture = root.join("first-fixture");
            let second_fixture = root.join("second-fixture");
            create_fixture(&first_fixture, "first-skin");
            create_fixture(&second_fixture, "second-skin");
            let first = root.join("first.zip");
            let second = root.join("second.zip");
            create_zip(&first, &first_fixture, None);
            create_zip(&second, &second_fixture, None);
            let service = create_service(&root);
            let prepared = service
                .prepare_import_batch(vec![first, second])
                .await
                .expect("批次应通过预检");
            create_fixture(&root.join("user/second-skin"), "second-skin");
            let selected = prepared
                .items
                .iter()
                .map(|item| item.item_id.clone())
                .collect::<Vec<_>>();

            let result = service
                .commit_import_batch(&prepared.token, &selected, true)
                .await
                .expect("单项冲突不应回滚整个批次");
            assert_eq!(result.installed.len(), 1);
            assert_eq!(result.installed[0].id, "first-skin");
            assert_eq!(result.failed.len(), 1);
            assert_eq!(result.failed[0].code, "skin.import_conflict");
            assert!(root.join("user/first-skin/theme.json").is_file());
            assert!(root.join("user/second-skin/theme.json").is_file());
            assert!(!std::fs::read_dir(root.join("user"))
                .expect("应读取用户目录")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(".import-")));
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }
