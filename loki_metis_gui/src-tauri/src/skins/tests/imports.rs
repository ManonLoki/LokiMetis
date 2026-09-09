#[test]
/// 验证换皮迁移中的 `batch_prepare_rejects_more_than_one_hundred_files` 回归场景。
fn batch_prepare_rejects_more_than_one_hundred_files() {
    tauri::async_runtime::block_on(async {
        let root = temp_directory("batch-limit");
        let service = create_service(&root);
        let paths = (0..=MAX_IMPORT_BATCH_FILES)
            .map(|index| root.join(format!("{index}.zip")))
            .collect();
        assert_eq!(
            service
                .prepare_import_batch(paths)
                .await
                .expect_err("超过一百个文件必须被拒绝")
                .code,
            "skin.import_batch_limit"
        );
        assert_eq!(MAX_IMPORT_BATCH_BYTES, 6_710_886_400);
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    });
}

#[test]
/// 未确认第三方代码时不得把兼容皮肤移入用户资源库，批次仍可取消并完整清理。
fn legacy_import_without_consent_has_no_persistent_side_effect() {
    tauri::async_runtime::block_on(async {
        let root = temp_directory("legacy-import-consent");
        let fixture = root.join("fixture");
        create_fixture(&fixture, "legacy-script");
        let archive = root.join("legacy-script.zip");
        create_zip(&archive, &fixture, None);
        let service = create_service(&root);
        let prepared = service
            .prepare_test_import_batch(&archive)
            .expect("兼容皮肤应进入待审核暂存批次");
        let selected = vec![prepared.items[0].item_id.clone()];

        let error = service
            .commit_import_batch(&prepared.token, &selected, false)
            .await
            .expect_err("未确认信任时不得提交兼容皮肤");
        assert_eq!(error.code, "skin.third_party_code_consent_required");
        assert!(!root.join("user/legacy-script").exists());
        assert!(
            service
                .pending_import
                .lock()
                .expect("应读取待审核批次")
                .as_ref()
                .is_some_and(|pending| pending.token == prepared.token)
        );

        service
            .cancel_import(&prepared.token)
            .expect("取消应清理未受信任的暂存批次");
        assert!(
            std::fs::read_dir(root.join("user"))
                .expect("应读取用户资源目录")
                .next()
                .is_none()
        );
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    });
}

#[test]
/// 验证换皮迁移中的 `batch_total_size_accepts_the_boundary_and_rejects_one_more_byte` 回归场景。
fn batch_total_size_accepts_the_boundary_and_rejects_one_more_byte() {
    assert_eq!(
        checked_batch_import_bytes(MAX_IMPORT_BATCH_BYTES - 1, 1).expect("批次总量边界应允许"),
        MAX_IMPORT_BATCH_BYTES
    );
    assert_eq!(
        checked_batch_import_bytes(MAX_IMPORT_BATCH_BYTES, 1)
            .expect_err("超过批次总量一字节必须拒绝")
            .code,
        "skin.import_batch_too_large"
    );
}

#[test]
/// 验证换皮迁移中的 `batch_prepare_rejects_a_batch_when_every_file_is_invalid` 回归场景。
fn batch_prepare_rejects_a_batch_when_every_file_is_invalid() {
    tauri::async_runtime::block_on(async {
        let root = temp_directory("batch-all-invalid");
        let first = root.join("one.txt");
        let second = root.join("two.zip");
        std::fs::write(&first, "文本").expect("应创建非 ZIP 文件");
        std::fs::write(&second, "损坏 ZIP").expect("应创建损坏 ZIP");
        let service = create_service(&root);
        let error = service
            .prepare_import_batch(vec![first, second])
            .await
            .expect_err("全部无效时不得生成审核批次");
        assert_eq!(error.code, "skin.import_batch_empty");
        assert_eq!(error.details.len(), 2);
        assert!(
            std::fs::read_dir(root.join("user"))
                .expect("应读取用户目录")
                .next()
                .is_none()
        );
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    });
}

#[test]
/// 验证换皮迁移中的 `commit_rejects_destination_created_after_prepare` 回归场景。
fn commit_rejects_destination_created_after_prepare() {
    tauri::async_runtime::block_on(async {
        let root = temp_directory("import-race");
        let fixture = root.join("fixture");
        create_fixture(&fixture, "test-skin");
        let archive = root.join("skin.zip");
        create_zip(&archive, &fixture, None);
        let service = create_service(&root);
        let prepared = service
            .prepare_test_import_batch(&archive)
            .expect("ZIP 应通过预检");
        create_fixture(&root.join("user/test-skin"), "test-skin");
        let error = service
            .commit_test_import_batch(&prepared.token)
            .await
            .expect_err("提交不得覆盖刚出现的目标目录");
        assert_eq!(error.code, "skin.import_conflict");
        assert!(root.join("user/test-skin/theme.json").is_file());
        service
            .cancel_import(&prepared.token)
            .expect("应清理暂存导入");
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    });
}

#[test]
/// 验证换皮迁移中的 `numbered_identity_stays_within_manifest_limits` 回归场景。
fn numbered_identity_stays_within_manifest_limits() {
    let root = temp_directory("numbered-identity-limits");
    let user_root = root.join("user");
    std::fs::create_dir_all(&user_root).expect("应创建用户目录");
    let base_id = "a".repeat(64);
    let base_name = "名".repeat(80);
    std::fs::create_dir(user_root.join(&base_id)).expect("应占用原始 ID");

    let (id, name) = available_import_identity(&user_root, &base_id, &base_name)
        .expect("应分配带编号的合法身份");
    assert_eq!(id.len(), 64);
    assert!(id.ends_with("_1"));
    assert_eq!(name.chars().count(), 80);
    assert!(name.ends_with("(1)"));
    assert!(is_valid_skin_id(&id));

    std::fs::remove_dir_all(root).expect("应清理测试目录");
}
#[test]
/// 验证换皮迁移中的 `initialize_cleans_staging_and_recovers_backup` 回归场景。
fn initialize_cleans_staging_and_recovers_backup() {
    let root = temp_directory("startup-recovery");
    std::fs::create_dir_all(root.join("builtin")).expect("应创建内置目录");
    std::fs::create_dir_all(root.join("user/.import-stale")).expect("应创建遗留暂存目录");
    std::fs::create_dir_all(root.join("user/.create-stale")).expect("应创建遗留创建目录");
    create_fixture(&root.join("user/.backup-stale"), "recovered-skin");
    let service = SkinService::new(root.join("builtin"), root.join("user"));

    service.initialize().expect("初始化应清理并恢复事务目录");
    assert!(!root.join("user/.import-stale").exists());
    assert!(!root.join("user/.create-stale").exists());
    assert!(root.join("user/recovered-skin/theme.json").is_file());
    assert_eq!(service.list_skins().expect("应读取恢复皮肤").len(), 1);
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}

#[test]
/// 验证换皮迁移中的 `builtin_skin_cannot_be_opened_or_deleted` 回归场景。
fn builtin_skin_cannot_be_opened_or_deleted() {
    tauri::async_runtime::block_on(async {
        let root = temp_directory("builtin-read-only");
        create_fixture(&root.join("builtin/test-skin"), "test-skin");
        std::fs::create_dir_all(root.join("user")).expect("应创建用户目录");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let skin = SkinReference {
            source: SkinSource::Builtin,
            id: "test-skin".into(),
        };
        assert_eq!(
            service
                .open_directory(&skin)
                .await
                .expect_err("内置目录不可打开")
                .code,
            "skin.builtin_read_only"
        );
        assert_eq!(
            service
                .delete(&skin)
                .await
                .expect_err("内置皮肤不可删除")
                .code,
            "skin.builtin_read_only"
        );
        assert!(root.join("builtin/test-skin").exists());
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    });
}

#[test]
/// 验证换皮迁移中的 `batch_delete_validates_selection_and_preserves_successes` 回归场景。
fn batch_delete_validates_selection_and_preserves_successes() {
    tauri::async_runtime::block_on(async {
        let root = temp_directory("batch-delete");
        std::fs::create_dir_all(root.join("builtin")).expect("应创建内置目录");
        create_fixture(&root.join("user/first"), "first");
        create_fixture(&root.join("user/second"), "second");
        let service = SkinService::new(root.join("builtin"), root.join("user"));
        let first = SkinReference {
            source: SkinSource::User,
            id: "first".into(),
        };
        let missing = SkinReference {
            source: SkinSource::User,
            id: "missing".into(),
        };

        let result = service
            .delete_many(&[first.clone(), missing.clone()])
            .await
            .expect("有效批次应尽量删除");
        assert_eq!(result.deleted, vec![first.clone()]);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].skin, missing);
        assert_eq!(result.failed[0].code, "skin.not_found");
        assert!(!root.join("user/first").exists());
        assert!(root.join("user/second").exists());

        let second = SkinReference {
            source: SkinSource::User,
            id: "second".into(),
        };
        let descriptor = load_descriptor(&root.join("user/second"), "second", SkinSource::User)
            .expect("活动皮肤描述应有效");
        let (cancel, _) = tokio::sync::watch::channel(false);
        let join = tokio::spawn(async {
            std::future::pending::<()>().await;
            Ok::<usize, super::AppError>(0)
        });
        let handler_abort = join.abort_handle();
        {
            let mut runtime = service.runtime.lock().await;
            runtime.instances.insert(
                "test-instance".into(),
                super::InstanceRuntime {
                    active: Some(descriptor),
                    compatibility: None,
                    task: Some(super::WatchTask::new_test(
                        super::SkinHostKind::Codex,
                        cancel,
                        join,
                        handler_abort,
                        super::CdpEndpoint::default(),
                    )),
                },
            );
            runtime
                .last_targets
                .insert(super::SkinHostKind::Codex, "test-instance".into());
        }
        let active_result = service
            .delete_many(std::slice::from_ref(&second))
            .await
            .expect("活动项目应作为单项失败返回");
        assert!(active_result.deleted.is_empty());
        assert_eq!(active_result.failed[0].code, "skin.in_use");
        assert!(root.join("user/second").exists());
        if let Some(task) = service
            .runtime
            .lock()
            .await
            .instances
            .remove("test-instance")
            .and_then(|instance| instance.task)
        {
            task.abort();
        }

        assert_eq!(
            service
                .delete_many(&[])
                .await
                .expect_err("空批次必须拒绝")
                .code,
            "skin.delete_selection_invalid"
        );
        assert_eq!(
            service
                .delete_many(&[first.clone(), first])
                .await
                .expect_err("重复选择必须拒绝")
                .code,
            "skin.delete_selection_duplicate"
        );
        assert_eq!(
            service
                .delete_many(&[SkinReference {
                    source: SkinSource::Builtin,
                    id: "builtin".into(),
                }])
                .await
                .expect_err("内置皮肤必须拒绝")
                .code,
            "skin.builtin_read_only"
        );
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    });
}

#[test]
/// 验证换皮迁移中的 `catalog_cache_refreshes_when_user_files_change` 回归场景。
fn catalog_cache_refreshes_when_user_files_change() {
    let root = temp_directory("catalog-auto-refresh");
    std::fs::create_dir_all(root.join("builtin")).expect("应创建内置目录");
    std::fs::create_dir_all(root.join("user/test-skin")).expect("应创建生成中的主题目录");
    std::fs::write(root.join("user/test-skin/theme.json"), "{}").expect("应写入未完成清单");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    assert!(
        service
            .list_skins()
            .expect("未完成目录应被安全忽略")
            .is_empty()
    );
    assert!(!service.catalog_changed().expect("缓存建立后目录应未变化"));

    create_fixture(&root.join("user/test-skin"), "test-skin");
    assert!(service.catalog_changed().expect("目录完成后应报告变化"));
    assert_eq!(
        service
            .list_skins()
            .expect("目录完成后应自动重新扫描")
            .len(),
        1
    );
    assert!(!service.catalog_changed().expect("刷新后目录应恢复稳定"));

    std::fs::write(root.join("user/test-skin/theme.json"), "{}").expect("应破坏测试清单");
    assert!(service.catalog_changed().expect("文件变化后应报告变化"));
    assert!(
        service
            .list_skins()
            .expect("文件变化后应重新校验")
            .is_empty()
    );
    assert!(!service.catalog_changed().expect("重新扫描后目录应恢复稳定"));
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}

#[test]
/// 验证换皮迁移中的 `catalog_orders_newest_user_skins_before_builtins` 回归场景。
fn catalog_orders_newest_user_skins_before_builtins() {
    let root = temp_directory("catalog-latest-first");
    create_fixture(&root.join("builtin/builtin-skin"), "builtin-skin");
    create_fixture(&root.join("user/older-skin"), "older-skin");
    std::thread::sleep(Duration::from_millis(25));
    create_fixture(&root.join("user/newer-skin"), "newer-skin");
    let service = SkinService::new(root.join("builtin"), root.join("user"));

    let skins = service.list_skins().expect("应扫描并排序双来源资源");
    assert_eq!(
        skins
            .iter()
            .map(|skin| (skin.source, skin.id.as_str()))
            .collect::<Vec<_>>(),
        [
            (SkinSource::User, "newer-skin"),
            (SkinSource::User, "older-skin"),
            (SkinSource::Builtin, "builtin-skin"),
        ]
    );
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}

#[test]
/// 验证换皮迁移中的 `repository_builtin_skin_folders_all_pass_native_validation` 回归场景。
fn repository_builtin_skin_folders_all_pass_native_validation() {
    let builtin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources/builtin-skins");
    let mut ids = std::fs::read_dir(builtin_root)
        .expect("应读取仓库内置皮肤目录")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .map(|entry| {
            let id = entry
                .file_name()
                .to_str()
                .map(str::to_owned)
                .expect("内置皮肤目录名应为 UTF-8");
            let descriptor = load_descriptor(&entry.path(), &id, SkinSource::Builtin)
                .expect("内置皮肤应通过原生资源校验");
            assert_eq!(
                descriptor.author, "ManonLoki",
                "内置皮肤 {id} 应统一作者署名"
            );
            assert_eq!(
                descriptor.supported_color_modes,
                vec![ColorMode::Light, ColorMode::Dark],
                "内置皮肤 {id} 应完整支持浅色与深色"
            );
            id
        })
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(
        ids,
        [
            "minecraft",
            "misty-meadow-dawn",
            "pastoral-landscape",
            "pikachu",
            "silver-core-voyage",
            "woodland-dawn",
        ]
    );
}

#[test]
/// 验证换皮迁移中的 `imports_zip_with_one_wrapper_directory` 回归场景。
fn imports_zip_with_one_wrapper_directory() {
    let root = temp_directory("zip");
    let fixture = root.join("fixture");
    create_fixture(&fixture, "zip-skin");
    let archive_path = root.join("skin.zip");
    create_zip(&archive_path, &fixture, Some("wrapper"));
    let service = create_service(&root);
    let prepared = service
        .prepare_test_import_batch(&archive_path)
        .expect("有效 ZIP 应通过预检");
    assert_eq!(prepared.items[0].skin.id, "zip-skin");
    assert_eq!(prepared.items[0].skin.source, SkinSource::User);
    service
        .cancel_import(&prepared.token)
        .expect("应取消暂存导入");
    assert!(!root.join("user/zip-skin").exists());
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}

#[test]
/// 验证换皮迁移中的 `imports_manifestless_legacy_zip_and_generates_stable_manifest` 回归场景。
fn imports_manifestless_legacy_zip_and_generates_stable_manifest() {
    tauri::async_runtime::block_on(async {
        let root = temp_directory("manifestless-legacy");
        let fixture = root.join("fixture");
        create_fixture(&fixture, "unused-id");
        let archive_path = root.join("凡人修仙二.zip");
        create_manifestless_legacy_zip(
            &archive_path,
            &fixture,
            Some("旧皮肤"),
            &LEGACY_RUNTIME_FILES,
        );
        let service = create_service(&root);

        let prepared = service
            .prepare_test_import_batch(&archive_path)
            .expect("无清单旧皮肤应自动生成兼容清单");
        assert_eq!(prepared.items[0].skin.name, "凡人修仙二");
        assert_eq!(prepared.items[0].skin.author, "未知作者");
        assert_eq!(
            prepared.items[0].skin.package_type,
            SkinPackageType::LegacySkin
        );
        assert!(prepared.items[0].skin.id.starts_with("legacy-"));
        let generated_id = prepared.items[0].skin.id.clone();
        service
            .commit_test_import_batch(&prepared.token)
            .await
            .expect("无清单旧皮肤应完成安装");

        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("user").join(&generated_id).join("theme.json"))
                .expect("安装目录应包含推导清单"),
        )
        .expect("推导清单应为有效 JSON");
        assert_eq!(manifest["schemaVersion"], 1);
        assert_eq!(manifest["id"], generated_id);
        assert_eq!(manifest["image"], "qq2007-sky.png");
        assert_eq!(manifest["friendCards"]["profileImage"], "avatar.png");
        assert_eq!(manifest["friendCards"]["listImage"], "qqshow.jpg");
        std::fs::remove_dir_all(root).expect("应清理测试目录");
    });
}

#[test]
/// 验证换皮迁移中的 `manifestless_legacy_zip_reports_every_missing_runtime_file` 回归场景。
fn manifestless_legacy_zip_reports_every_missing_runtime_file() {
    let root = temp_directory("manifestless-legacy-missing");
    let fixture = root.join("fixture");
    create_fixture(&fixture, "unused-id");
    let archive_path = root.join("不完整旧皮肤.zip");
    create_manifestless_legacy_zip(
        &archive_path,
        &fixture,
        None,
        &["dream-skin.css", "qq2007-sky.png"],
    );
    let service = create_service(&root);

    let error = service
        .prepare_test_import_batch(&archive_path)
        .expect_err("缺少运行文件的旧皮肤必须被拒绝");
    assert_eq!(error.code, "skin.assets_invalid");
    assert_eq!(error.details.len(), 3);
    assert!(
        error
            .details
            .iter()
            .any(|detail| detail.contains("renderer-inject.js"))
    );
    assert!(
        error
            .details
            .iter()
            .any(|detail| detail.contains("avatar.png"))
    );
    assert!(
        error
            .details
            .iter()
            .any(|detail| detail.contains("qqshow.jpg"))
    );
    assert!(
        std::fs::read_dir(root.join("user"))
            .expect("应读取用户目录")
            .next()
            .is_none()
    );
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}

#[test]
/// 验证换皮迁移中的 `new_theme_without_manifest_reports_theme_json_instead_of_legacy_files` 回归场景。
fn new_theme_without_manifest_reports_theme_json_instead_of_legacy_files() {
    let root = temp_directory("theme-missing-manifest");
    let fixture = root.join("fixture");
    create_theme_css_fixture(&fixture, "unused-theme");
    let archive_path = root.join("新版主题.zip");
    create_manifestless_legacy_zip(
        &archive_path,
        &fixture,
        None,
        &["theme.css", "preview.png", "background.png"],
    );
    let service = create_service(&root);

    let error = service
        .prepare_test_import_batch(&archive_path)
        .expect_err("新版主题缺少清单时必须明确提示 theme.json");
    assert_eq!(error.code, "skin.assets_invalid");
    assert_eq!(error.details.len(), 1);
    assert!(error.details[0].contains("theme.json"));
    assert!(!error.details[0].contains("renderer-inject.js"));
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}
