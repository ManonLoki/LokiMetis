// 本文件是 imports.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证换皮迁移中的 `imports_zip_with_one_wrapper_directory` 回归场景。；验证换皮迁移中的 `imports_manifestless_legacy_zip_and_generates_stable_manifest` 回归场景。；验证换皮迁移中的 `manifestless_legacy_zip_reports_every_missing_runtime_file` 回归场景。

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
    crate::skins::test_block_on(async {
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
