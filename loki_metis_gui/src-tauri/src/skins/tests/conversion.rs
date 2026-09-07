    #[test]
    /// 验证换皮迁移中的 `normalizes_only_hex_theme_colors` 回归场景。
    fn normalizes_only_hex_theme_colors() {
        assert_eq!(normalize_theme_color("#abc"), Some("#AABBCC".into()));
        assert_eq!(normalize_theme_color("#abcd"), Some("#AABBCCDD".into()));
        assert_eq!(normalize_theme_color("  #0a1B2c  "), Some("#0A1B2C".into()));
        assert_eq!(normalize_theme_color("#0a1B2c80"), Some("#0A1B2C80".into()));
        for value in ["rgb(1,2,3)", "red", "#ab", "#abcde", "#gggggg", "", "#"] {
            assert_eq!(normalize_theme_color(value), None, "不应接受 {value}");
        }
    }

    #[test]
    /// 验证换皮迁移中的 `converted_identity_stays_within_length_limits` 回归场景。
    fn converted_identity_stays_within_length_limits() {
        assert_eq!(converted_theme_id("minecraft"), "minecraft-theme");
        let long_id = "a".repeat(64);
        let converted = converted_theme_id(&long_id);
        assert_eq!(converted.len(), 64);
        assert!(converted.ends_with("-theme"));
        assert!(is_valid_skin_id(&converted));

        assert_eq!(converted_theme_name("  测试皮肤  "), "测试皮肤（主题版）");
        let long_name = "长".repeat(120);
        assert_eq!(converted_theme_name(&long_name).chars().count(), 80);
    }

    #[test]
    /// 验证换皮迁移中的 `converts_legacy_skin_into_user_theme_without_touching_source` 回归场景。
    fn converts_legacy_skin_into_user_theme_without_touching_source() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("convert-legacy");
            let service = create_service(&root);
            let source = root.join("builtin").join("legacy-one");
            create_legacy_fixture_with_colors(
                &source,
                "legacy-one",
                Some(dual_palette_fixture_colors()),
            );
            let source_files = theme_directory_files(&source);

            let result = service
                .convert_to_theme(&SkinReference {
                    source: SkinSource::Builtin,
                    id: "legacy-one".into(),
                })
                .await
                .expect("旧皮肤应能转换为纯主题");

            assert!(result.fallback_roles.is_empty());
            assert_eq!(result.theme.id, "legacy-one-theme");
            assert_eq!(result.theme.name, "测试皮肤（主题版）");
            assert_eq!(result.theme.author, "测试作者");
            assert_eq!(result.theme.source, SkinSource::User);
            assert_eq!(result.theme.package_type, SkinPackageType::Theme);

            let directory = root.join("user").join(&result.theme.id);
            assert_eq!(
                theme_directory_files(&directory),
                HashSet::from([
                    "theme.json".to_owned(),
                    "theme.css".to_owned(),
                    "background.png".to_owned(),
                ])
            );
            let manifest: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(directory.join("theme.json")).expect("应读取主题清单"),
            )
            .expect("主题清单应为 JSON");
            assert_eq!(manifest["schemaVersion"], 3);
            assert_eq!(manifest["type"], "theme");
            assert_eq!(manifest["id"], "legacy-one-theme");
            assert!(manifest["$comment"]
                .as_str()
                .expect("应写入转换说明")
                .contains("测试皮肤"));

            let css = std::fs::read_to_string(directory.join("theme.css")).expect("应读取变量表");
            assert!(css.contains("--skin-preview-image: url(\"background.png\")"));
            assert!(css.contains("--skin-background-image: url(\"background.png\")"));
            let dark = theme_css_block(&css, "[data-dream-shell=\"dark\"]");
            assert!(dark.contains("--skin-bg: #07131F;"));
            assert!(dark.contains("--skin-accent-alt: #78B9FF;"));
            let light = theme_css_block(&css, "[data-dream-shell=\"light\"]");
            assert!(light.contains("--skin-bg: #E7F4F5;"));
            assert!(light.contains("--skin-line: #91ADB4;"));
            // 非颜色参数继承应用内模板默认值。
            assert!(css.contains("--skin-radius: 12px;"));

            assert_eq!(theme_directory_files(&source), source_files);
            let names = archive_names(
                service
                    .build_export_archive(&SkinReference {
                        source: SkinSource::User,
                        id: result.theme.id.clone(),
                    })
                    .expect("转换后的主题应可导出"),
            );
            assert_eq!(
                names,
                vec![
                    "background.png".to_owned(),
                    "theme.css".to_owned(),
                    "theme.json".to_owned()
                ]
            );
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `converts_single_palette_legacy_skin_for_both_modes` 回归场景。
    fn converts_single_palette_legacy_skin_for_both_modes() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("convert-single-palette");
            let service = create_service(&root);
            create_legacy_fixture_with_colors(
                &root.join("user").join("legacy-flat"),
                "legacy-flat",
                Some(json!({
                    "background": "#111111",
                    "panel": "#222222",
                    "panelAlt": "#333333",
                    "accent": "#abc",
                    "accentAlt": "#555555",
                    "text": "#666666",
                    "muted": "#777777",
                    "line": "#888888"
                })),
            );

            let result = service
                .convert_to_theme(&SkinReference {
                    source: SkinSource::User,
                    id: "legacy-flat".into(),
                })
                .await
                .expect("单套调色板也应能转换");

            assert!(result.fallback_roles.is_empty());
            let css =
                std::fs::read_to_string(root.join("user").join(&result.theme.id).join("theme.css"))
                    .expect("应读取变量表");
            let dark = theme_css_block(&css, "[data-dream-shell=\"dark\"]");
            let light = theme_css_block(&css, "[data-dream-shell=\"light\"]");
            assert!(dark.contains("--skin-bg: #111111;"));
            assert!(light.contains("--skin-bg: #111111;"));
            assert!(dark.contains("--skin-accent: #AABBCC;"));
            assert!(light.contains("--skin-accent: #AABBCC;"));
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `reports_fallback_roles_and_keeps_template_defaults` 回归场景。
    fn reports_fallback_roles_and_keeps_template_defaults() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("convert-fallback");
            let service = create_service(&root);
            create_legacy_fixture_with_colors(
                &root.join("user").join("legacy-partial"),
                "legacy-partial",
                Some(json!({
                    "light": {
                        "background": "#E7F4F5",
                        "panel": "rgb(1, 2, 3)",
                        "panelAlt": "#D9ECEE",
                        "accent": "#087B65",
                        "accentAlt": "#176DB0",
                        "text": "#102A34",
                        "muted": "#4C6871",
                        "line": "#91ADB4"
                    }
                })),
            );

            let result = service
                .convert_to_theme(&SkinReference {
                    source: SkinSource::User,
                    id: "legacy-partial".into(),
                })
                .await
                .expect("部分配色缺失时仍应产出可用主题");

            assert!(result.fallback_roles.contains(&"浅色模式的面板".to_owned()));
            assert!(result.fallback_roles.contains(&"深色模式的背景".to_owned()));
            assert_eq!(result.fallback_roles.len(), 9);

            let css =
                std::fs::read_to_string(root.join("user").join(&result.theme.id).join("theme.css"))
                    .expect("应读取变量表");
            let light = theme_css_block(&css, "[data-dream-shell=\"light\"]");
            assert!(light.contains("--skin-bg: #E7F4F5;"));
            // 无法迁移的角色保留模板默认值。
            assert!(light.contains("--skin-panel: #F5FBFE;"));
            let dark = theme_css_block(&css, "[data-dream-shell=\"dark\"]");
            assert!(dark.contains("--skin-bg: #061E33;"));
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `repeated_conversion_saves_numbered_copy` 回归场景。
    fn repeated_conversion_saves_numbered_copy() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("convert-repeat");
            let service = create_service(&root);
            create_legacy_fixture_with_colors(
                &root.join("builtin").join("legacy-two"),
                "legacy-two",
                Some(dual_palette_fixture_colors()),
            );
            let reference = SkinReference {
                source: SkinSource::Builtin,
                id: "legacy-two".into(),
            };

            let first = service
                .convert_to_theme(&reference)
                .await
                .expect("首次转换应成功");
            let second = service
                .convert_to_theme(&reference)
                .await
                .expect("重复转换应另存");

            assert_eq!(first.theme.id, "legacy-two-theme");
            assert_eq!(second.theme.id, "legacy-two-theme_1");
            assert_eq!(second.theme.name, "测试皮肤（主题版）(1)");
            assert!(root.join("user").join(&first.theme.id).is_dir());
            assert!(root.join("user").join(&second.theme.id).is_dir());
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `rejects_converting_pure_theme_and_missing_skin` 回归场景。
    fn rejects_converting_pure_theme_and_missing_skin() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("convert-rejects");
            let service = create_service(&root);
            create_theme_css_fixture(&root.join("user").join("already-theme"), "already-theme");

            let error = service
                .convert_to_theme(&SkinReference {
                    source: SkinSource::User,
                    id: "already-theme".into(),
                })
                .await
                .expect_err("纯主题不应可转换");
            assert_eq!(error.code, "skin.convert_not_legacy");

            let missing = service
                .convert_to_theme(&SkinReference {
                    source: SkinSource::User,
                    id: "not-installed".into(),
                })
                .await
                .expect_err("不存在的皮肤不应可转换");
            assert_eq!(missing.code, "skin.not_found");

            let invalid = service
                .convert_to_theme(&SkinReference {
                    source: SkinSource::User,
                    id: "../escape".into(),
                })
                .await
                .expect_err("非法标识不应可转换");
            assert_eq!(invalid.code, "skin.id_invalid");

            assert_eq!(
                theme_directory_files(&root.join("user")),
                HashSet::from(["already-theme".to_owned()])
            );
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    /// 验证换皮迁移中的 `conversion_rejects_legacy_art_that_is_not_png` 回归场景。
    fn conversion_rejects_legacy_art_that_is_not_png() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("convert-bad-art");
            let service = create_service(&root);
            // create_fixture 写入的是占位字节，不是合法 PNG。
            create_fixture(&root.join("user").join("legacy-bad"), "legacy-bad");

            let error = service
                .convert_to_theme(&SkinReference {
                    source: SkinSource::User,
                    id: "legacy-bad".into(),
                })
                .await
                .expect_err("背景图片不是 PNG 时不应产出主题");
            assert_eq!(error.code, "skin.assets_invalid");
            assert_eq!(
                theme_directory_files(&root.join("user")),
                HashSet::from(["legacy-bad".to_owned()])
            );
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }
