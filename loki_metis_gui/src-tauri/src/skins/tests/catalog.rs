    #[test]
    /// 验证换皮迁移中的 `creates_valid_user_theme_with_safe_unique_id` 回归场景。
    fn creates_valid_user_theme_with_safe_unique_id() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("create-theme");
            let service = create_service(&root);
            assert!(service.list_skins().expect("初始目录应为空").is_empty());

            let first = service
                .create_user_theme("  My Cool Theme  ", "  测试作者  ")
                .await
                .expect("应创建标准纯主题");
            assert!(first.id.starts_with("theme_"));
            let first_uuid =
                uuid::Uuid::parse_str(&first.id["theme_".len()..]).expect("主题标识后缀应为 UUID");
            assert_eq!(first_uuid.get_version_num(), 7);
            assert_eq!(first.name, "My Cool Theme");
            assert_eq!(first.author, "测试作者");
            assert_eq!(first.package_type, SkinPackageType::Theme);
            let directory = root.join("user").join(&first.id);
            let files = std::fs::read_dir(&directory)
                .expect("应读取新主题目录")
                .map(|entry| {
                    entry
                        .expect("应读取新主题文件")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect::<HashSet<_>>();
            assert_eq!(
                files,
                HashSet::from([
                    "theme.json".to_owned(),
                    "theme.css".to_owned(),
                    "preview.png".to_owned(),
                    "background.png".to_owned(),
                ])
            );
            let manifest: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(directory.join("theme.json")).expect("应读取主题清单"),
            )
            .expect("主题清单应为 JSON");
            assert_eq!(manifest["id"], first.id);
            assert_eq!(manifest["name"], "My Cool Theme");
            assert_eq!(manifest["author"], "测试作者");
            assert_eq!(manifest["schemaVersion"], 3);
            assert!(manifest.get("colors").is_none());
            let theme_css =
                std::fs::read_to_string(directory.join("theme.css")).expect("应读取主题变量表");
            assert!(theme_css.contains("--skin-bg: #061E33"));
            assert!(theme_css.contains("--skin-accent: #5BD7FF"));
            assert_eq!(
                manifest["description"],
                "My Cool Theme 的纯主题，可在主题目录中继续编辑。"
            );
            for (file_name, width, height) in [
                ("preview.png", 1280_u32, 720_u32),
                ("background.png", 1920, 1080),
            ] {
                let bytes = std::fs::read(directory.join(file_name)).expect("应读取蓝图图片");
                assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
                assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), width);
                assert_eq!(
                    u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
                    height
                );
            }
            assert_eq!(service.list_skins().expect("创建后应刷新缓存").len(), 1);

            let duplicate = service
                .create_user_theme("My Cool Theme", "另一个作者")
                .await
                .expect("同名主题应使用新目录");
            assert!(duplicate.id.starts_with("theme_"));
            assert_ne!(duplicate.id, first.id);
            assert!(root
                .join("user")
                .join(&first.id)
                .join("theme.json")
                .is_file());
            assert!(root
                .join("user")
                .join(&duplicate.id)
                .join("theme.json")
                .is_file());
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `theme_creation_validates_unicode_input` 回归场景。
    fn theme_creation_validates_unicode_input() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("create-theme-input");
            let service = create_service(&root);
            assert_eq!(
                service
                    .create_user_theme(" ", "作者")
                    .await
                    .expect_err("空名称必须被拒绝")
                    .code,
                "skin.create_invalid"
            );
            let long_name = "主".repeat(81);
            assert_eq!(
                service
                    .create_user_theme(&long_name, "作者")
                    .await
                    .expect_err("超过 80 个 Unicode 字符必须被拒绝")
                    .code,
                "skin.create_invalid"
            );
            assert!(std::fs::read_dir(root.join("user"))
                .expect("应读取用户目录")
                .next()
                .is_none());
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }
    #[test]
    /// 验证换皮迁移中的 `exports_theme_and_legacy_skin_with_exact_runtime_files` 回归场景。
    fn exports_theme_and_legacy_skin_with_exact_runtime_files() {
        let root = temp_directory("export-skins");
        create_mode_theme_fixture(&root.join("user/theme-css-example"), "theme-css-example");
        create_fixture(&root.join("user/legacy-example"), "legacy-example");
        std::fs::write(root.join("user/legacy-example/notes.txt"), "不要导出")
            .expect("应写入额外文件");
        std::fs::create_dir_all(root.join("builtin")).expect("应创建内置目录");
        let service = SkinService::new(root.join("builtin"), root.join("user"));

        assert_eq!(
            archive_names(
                service
                    .build_export_archive(&SkinReference {
                        source: SkinSource::User,
                        id: "theme-css-example".into(),
                    })
                    .expect("CSS 变量主题应可导出"),
            ),
            [
                "background-dark.jpg",
                "background-light.png",
                "background.png",
                "preview.png",
                "theme.css",
                "theme.dark.css",
                "theme.json",
                "theme.light.css",
            ]
        );
        assert_eq!(
            archive_names(
                service
                    .build_export_archive(&SkinReference {
                        source: SkinSource::User,
                        id: "legacy-example".into(),
                    })
                    .expect("旧皮肤应可导出"),
            ),
            [
                "avatar.png",
                "dream-skin.css",
                "qq2007-sky.png",
                "qqshow.jpg",
                "renderer-inject.js",
                "theme.json",
            ]
        );
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `exported_theme_and_legacy_skin_can_be_imported_again` 回归场景。
    fn exported_theme_and_legacy_skin_can_be_imported_again() {
        let root = temp_directory("export-reimport");
        create_theme_css_fixture(
            &root.join("source/user/theme-css-example"),
            "theme-css-example",
        );
        create_fixture(&root.join("source/user/legacy-example"), "legacy-example");
        let source = create_service(&root.join("source"));
        let target = create_service(&root.join("target"));

        for (id, package_type) in [
            ("theme-css-example", SkinPackageType::Theme),
            ("legacy-example", SkinPackageType::LegacySkin),
        ] {
            let archive_path = root.join(format!("{id}.zip"));
            let archive = source
                .build_export_archive(&SkinReference {
                    source: SkinSource::User,
                    id: id.into(),
                })
                .expect("用户资源应可导出");
            std::fs::write(&archive_path, archive).expect("应保存测试导出包");
            let prepared = target
                .prepare_test_import_batch(&archive_path)
                .expect("导出包应可直接重新导入");
            assert_eq!(prepared.items[0].skin.id, id);
            assert_eq!(prepared.items[0].skin.package_type, package_type);
            target
                .cancel_import(&prepared.token)
                .expect("应取消测试导入");
        }

        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `export_rejects_builtin_and_invalid_edited_theme` 回归场景。
    fn export_rejects_builtin_and_invalid_edited_theme() {
        let root = temp_directory("export-invalid");
        create_theme_css_fixture(&root.join("builtin/theme-example"), "theme-example");
        create_theme_css_fixture(&root.join("user/theme-example"), "theme-example");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        assert_eq!(
            service
                .build_export_archive(&SkinReference {
                    source: SkinSource::Builtin,
                    id: "theme-example".into(),
                })
                .expect_err("内置主题不可导出")
                .code,
            "skin.export_builtin"
        );
        std::fs::write(root.join("user/theme-example/background.png"), "损坏")
            .expect("应破坏用户主题图片");
        assert_eq!(
            service
                .build_export_archive(&SkinReference {
                    source: SkinSource::User,
                    id: "theme-example".into(),
                })
                .expect_err("非法编辑后的主题不可导出")
                .code,
            "skin.assets_invalid"
        );
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `zip_path_traversal_is_rejected_without_leftovers` 回归场景。
    fn zip_path_traversal_is_rejected_without_leftovers() {
        let root = temp_directory("zip-traversal");
        let archive_path = root.join("bad.zip");
        let archive_file = std::fs::File::create(&archive_path).expect("应创建 ZIP");
        let mut writer = zip::ZipWriter::new(archive_file);
        writer
            .start_file("theme.json", SimpleFileOptions::default())
            .expect("应创建清单条目");
        writer.write_all(b"{}").expect("应写入清单");
        writer
            .start_file("../escape.txt", SimpleFileOptions::default())
            .expect("应创建越界条目");
        writer.write_all(b"escape").expect("应写入越界条目");
        writer.finish().expect("应完成 ZIP");

        let service = create_service(&root);
        let error = service
            .prepare_test_import_batch(&archive_path)
            .expect_err("路径穿越 ZIP 必须被拒绝");
        assert_eq!(error.code, "skin.import_invalid");
        assert!(!root.join("escape.txt").exists());
        assert!(service.list_skins().expect("资源库仍应可扫描").is_empty());
        assert!(std::fs::read_dir(root.join("user"))
            .expect("应读取用户目录")
            .next()
            .is_none());
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `skin_id_rejects_paths_and_blank_values` 回归场景。
    fn skin_id_rejects_paths_and_blank_values() {
        assert!(is_valid_skin_id("manonloki"));
        assert!(is_valid_skin_id("blue-dream_2"));
        assert!(!is_valid_skin_id(""));
        assert!(!is_valid_skin_id("../manonloki"));
        assert!(!is_valid_skin_id("Manonloki"));

        let root = temp_directory("missing");
        let error = match load_skin(
            &root,
            &root,
            &SkinReference {
                source: SkinSource::User,
                id: "missing".into(),
            },
            false,
        ) {
            Err(error) => error,
            Ok(_) => panic!("不存在的皮肤标识不应被加载"),
        };
        assert_eq!(error.code, "skin.not_found");
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `status_uses_frontend_contract` 回归场景。
    fn status_uses_frontend_contract() -> Result<(), serde_json::Error> {
        let skin = super::SkinDescriptor {
            id: "manonloki".into(),
            source: SkinSource::Builtin,
            package_type: SkinPackageType::LegacySkin,
            name: "Codex QQ 复古风".into(),
            author: "ManonLoki".into(),
            version: SKIN_VERSION.into(),
            preview_data_url: "data:image/png;base64,AA==".into(),
            supported_color_modes: vec![super::ColorMode::Light],
        };
        assert_eq!(
            serde_json::to_value(SkinStatus::running(
                &skin,
                2,
                Some(SkinCompatibilityStatus {
                    version: HOST_COMPATIBILITY_VERSION.into(),
                    mode: SkinCompatibilityMode::Adapted,
                    applied_rules: vec!["legacy-main-surface".into()],
                    skipped_rules: Vec::new(),
                }),
            ))?,
            json!({
                "installed": true,
                "skinId": "manonloki",
                "source": "builtin",
                "packageType": "legacySkin",
                "skinName": "Codex QQ 复古风",
                "version": "1.7.0",
                "affectedPages": 2,
                "compatibility": {
                    "version": "5",
                    "mode": "adapted",
                    "appliedRules": ["legacy-main-surface"],
                    "skippedRules": []
                }
            })
        );
        Ok(())
    }

    #[test]
    /// 验证换皮迁移中的 `compatibility_results_are_deduplicated_and_partial_takes_precedence` 回归场景。
    fn compatibility_results_are_deduplicated_and_partial_takes_precedence() {
        let mut report = InjectionReport::default();
        report.include_compatibility(CompatibilityPageReport {
            version: HOST_COMPATIBILITY_VERSION.into(),
            applied_rules: vec!["legacy-main-surface".into(), "legacy-main-surface".into()],
            skipped_rules: Vec::new(),
        });
        report.include_compatibility(CompatibilityPageReport {
            version: HOST_COMPATIBILITY_VERSION.into(),
            applied_rules: vec!["legacy-app-header-tint".into()],
            skipped_rules: vec!["legacy-composer-surface-chrome".into()],
        });

        assert_eq!(
            report.compatibility_status(),
            SkinCompatibilityStatus {
                version: HOST_COMPATIBILITY_VERSION.into(),
                mode: SkinCompatibilityMode::Partial,
                applied_rules: vec![
                    "legacy-app-header-tint".into(),
                    "legacy-main-surface".into(),
                ],
                skipped_rules: vec!["legacy-composer-surface-chrome".into()],
            }
        );
    }

    #[test]
    /// 验证换皮迁移中的 `loading_skin_keeps_css_file_bytes_unchanged` 回归场景。
    fn loading_skin_keeps_css_file_bytes_unchanged() {
        let root = temp_directory("css-byte-stability");
        let skin_directory = root.join("user/test-skin");
        create_fixture(&skin_directory, "test-skin");
        let before =
            std::fs::read(skin_directory.join("dream-skin.css")).expect("应读取加载前样式");

        load_skin(
            &root.join("builtin"),
            &root.join("user"),
            &SkinReference {
                source: SkinSource::User,
                id: "test-skin".into(),
            },
            true,
        )
        .expect("皮肤应成功加载");

        let after = std::fs::read(skin_directory.join("dream-skin.css")).expect("应读取加载后样式");
        assert_eq!(before, after);
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    #[test]
    /// 验证换皮迁移中的 `codex_runtime_status_uses_frontend_contract_and_cdp_precedence` 回归场景。
    fn codex_runtime_status_uses_frontend_contract_and_cdp_precedence(
    ) -> Result<(), serde_json::Error> {
        assert_eq!(
            classify_codex_runtime(false, false),
            CodexRuntimeState::Stopped
        );
        assert_eq!(
            classify_codex_runtime(true, false),
            CodexRuntimeState::Ready
        );
        assert_eq!(
            classify_codex_runtime(false, true),
            CodexRuntimeState::RunningWithoutCdp
        );
        assert_eq!(classify_codex_runtime(true, true), CodexRuntimeState::Ready);
        assert_eq!(
            serde_json::to_value(CodexRuntimeStatus::new(
                CodexRuntimeState::RunningWithoutCdp
            ))?,
            json!({ "state": "runningWithoutCdp" })
        );
        Ok(())
    }

    #[test]
    /// 验证换皮迁移中的 `force_close_timeout_uses_stable_error_contract` 回归场景。
    fn force_close_timeout_uses_stable_error_contract() {
        let error = force_close_timeout_error(SkinHostKind::Codex);
        assert_eq!(error.code, "skin.codex_force_close_timeout");
        assert!(error.message.contains("15 秒"));
    }

    #[test]
    /// 验证换皮迁移中的 `codex_wait_limits_and_process_exit_use_stable_contracts` 回归场景。
    fn codex_wait_limits_and_process_exit_use_stable_contracts() {
        assert_eq!(CODEX_LAUNCH_TIMEOUT, Duration::from_secs(15));
        assert_eq!(CODEX_PAGE_READY_TIMEOUT, Duration::from_secs(15));
        assert_eq!(EXISTING_CODEX_PAGE_READY_TIMEOUT, Duration::from_secs(15));
        assert!(ensure_codex_running(true).is_ok());
        assert_eq!(
            ensure_codex_running(false)
                .expect_err("进程退出必须结束等待")
                .code,
            "skin.codex_exited"
        );
    }

    #[test]
    /// 验证换皮迁移中的 `codex_operation_cancel_is_immediate_and_generation_safe` 回归场景。
    fn codex_operation_cancel_is_immediate_and_generation_safe() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("codex-operation-cancel");
            let service = create_service(&root);
            let (old_guard, _old_receiver) = service.begin_codex_operation().expect("应注册旧操作");
            let (new_guard, mut receiver) = service.begin_codex_operation().expect("应注册新操作");
            drop(old_guard);

            assert!(service.cancel_codex_operation());
            let error = cancellable_sleep(&mut receiver, Duration::from_secs(1))
                .await
                .expect_err("取消后不应继续等待");
            assert_eq!(error.code, "skin.operation_cancelled");

            drop(new_guard);
            assert!(!service.cancel_codex_operation());
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }
