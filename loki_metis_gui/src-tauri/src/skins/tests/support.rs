    use std::collections::HashSet;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use base64::Engine as _;
    use serde_json::json;
    use zip::write::SimpleFileOptions;

    use super::{
        available_import_identity, build_appearance_check, build_payload, cancellable_sleep,
        checked_batch_import_bytes, classify_codex_runtime, command_line_arguments,
        commit_injection_transaction_expression, converted_theme_id, converted_theme_name,
        current_skin_expression,
        debug_port_from_command_line, displayed_active_skin_name,
        endpoint_candidates_from_commands, endpoint_owner_is_stable, ensure_codex_running,
        finish_cleanup_report,
        force_close_timeout_error, host_endpoint_candidates_from_commands,
        is_allowed_websocket, is_host_inspection_error, is_primary_codex_command_line,
        is_valid_skin_id, is_workbuddy_connection_error,
        legacy_has_complete_dual_palettes,
        load_descriptor, load_skin, matching_process_command_lines, normalize_theme_color,
        ready_runtime_can_be_reused, resolved_instance_for_host, restarted_instance_for_endpoint,
        reusable_process_arguments, rollback_injection_transaction_expression,
        reconnect_validation_decision, scanned_codex_instance, should_reconnect_initial_session,
        should_request_workbuddy_recovery, unique_codex_pid_for_endpoint,
        trusted_codex_root_pid, trusted_workbuddy_root_pid, unique_workbuddy_target_for_endpoint,
        user_data_directory, user_data_profile,
        validate_bundled_skill_file,
        validated_account_avatar, workbuddy_target_binding_matches, AppError, AppearancePolicy,
        AppearanceProbe, CdpEndpoint, CodexInstance,
        CodexRuntimeState, CodexRuntimeStatus, ColorMode, CompatibilityPageReport,
        ConnectionSource, HandlerTaskGuard, InjectionReport, InjectionTransaction, LegacyManifest,
        PageProbe, PlatformCodexProcess, ReconnectValidationDecision, SkinCompatibilityMode,
        SkinCompatibilityStatus,
        SkinImportPreparationEvent,
        SkinHostKind, SkinPackageType, SkinReference, SkinService, SkinSource, SkinStatus,
        ACCOUNT_PROFILE_PROBE_SCRIPT, CODEX_LAUNCH_TIMEOUT, CODEX_PAGE_READY_TIMEOUT,
        EXISTING_CODEX_PAGE_READY_TIMEOUT, HOST_COMPATIBILITY_SCRIPT,
        HOST_COMPATIBILITY_VERSION, LEGACY_REQUIRED_FILES, LEGACY_RUNTIME_FILES,
        MAX_IMPORT_BATCH_BYTES, MAX_IMPORT_BATCH_FILES, PROBE_SCRIPT, SKIN_VERSION,
        THEME_RUNTIME_CSS, WORKBUDDY_HOST_COMPATIBILITY_SCRIPT,
        WORKBUDDY_PROBE_SCRIPT,
    };

    /// 验证换皮迁移中的 `temp_directory` 回归场景。
    fn temp_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |value| value.as_nanos());
        let directory = std::env::temp_dir().join(format!(
            "loki-metis-skin-test-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("应创建测试目录");
        directory
    }

    #[test]
    /// 验证换皮迁移中的 `instance_skin_name_is_exposed_for_live_recovered_state` 回归场景。
    fn instance_skin_name_is_exposed_for_live_recovered_state() {
        let root = temp_directory("instance-active-skin");
        let skin_directory = root.join("test-skin");
        create_fixture(&skin_directory, "test-skin");
        let skin = load_descriptor(&skin_directory, "test-skin", SkinSource::User)
            .expect("测试皮肤描述应有效");

        assert_eq!(
            displayed_active_skin_name(Some(&skin)),
            Some("测试皮肤".into())
        );
        assert_eq!(displayed_active_skin_name(None), None);

        std::fs::remove_dir_all(root).expect("应清理测试目录");
    }

    /// 验证换皮迁移中的 `create_fixture` 回归场景。
    fn create_fixture(directory: &Path, id: &str) {
        std::fs::create_dir_all(directory).expect("应创建皮肤夹具目录");
        let manifest = json!({
            "schemaVersion": 1,
            "id": id,
            "name": "测试皮肤",
            "author": "测试作者",
            "image": "qq2007-sky.png",
            "friendCards": {
                "profileImage": "avatar.png",
                "listImage": "qqshow.jpg"
            }
        });
        std::fs::write(directory.join("theme.json"), manifest.to_string()).expect("应写入清单");
        std::fs::write(directory.join("dream-skin.css"), ":root.codex-dream-skin{}")
            .expect("应写入样式");
        std::fs::write(
            directory.join("renderer-inject.js"),
            "__DREAM_SKIN_CSS_JSON__;__DREAM_SKIN_ART_JSON__;__DREAM_SKIN_AVATAR_JSON__;__DREAM_SKIN_FRIENDS_JSON__;__DREAM_SKIN_THEME_JSON__;__DREAM_SKIN_VERSION_JSON__;",
        )
        .expect("应写入注入脚本");
        for file in ["qq2007-sky.png", "avatar.png", "qqshow.jpg"] {
            std::fs::write(directory.join(file), [1_u8, 2, 3]).expect("应写入图片");
        }
    }

    /// 验证换皮迁移中的 `create_theme_fixture` 回归场景。
    fn create_theme_fixture(directory: &Path, id: &str) {
        std::fs::create_dir_all(directory).expect("应创建纯主题夹具目录");
        let manifest = json!({
            "schemaVersion": 2,
            "type": "theme",
            "id": id,
            "name": "测试纯主题",
            "description": "用于验证应用内统一主题运行时。",
            "author": "测试作者",
            "images": {
                "preview": "background.png",
                "background": "background.png",
                "profile": "profile.jpg"
            },
            "colors": {
                "dark": {
                    "background": "#07131F",
                    "panel": "#102738",
                    "panelAlt": "#18394A",
                    "accent": "#78F0C6",
                    "accentAlt": "#78B9FF",
                    "text": "#F4FAFF",
                    "muted": "#B6CBD8",
                    "line": "#416477"
                },
                "light": {
                    "background": "#E7F4F5",
                    "panel": "#F8FCFC",
                    "panelAlt": "#D9ECEE",
                    "accent": "#087B65",
                    "accentAlt": "#176DB0",
                    "text": "#102A34",
                    "muted": "#4C6871",
                    "line": "#91ADB4"
                }
            }
        });
        std::fs::write(directory.join("theme.json"), manifest.to_string())
            .expect("应写入纯主题清单");
        std::fs::write(
            directory.join("background.png"),
            b"\x89PNG\r\n\x1a\nfixture",
        )
        .expect("应写入 PNG 夹具");
        std::fs::write(directory.join("profile.jpg"), [0xff, 0xd8, 0xff, 0xd9])
            .expect("应写入 JPEG 夹具");
    }

    /// 验证换皮迁移中的 `create_theme_css_fixture` 回归场景。
    fn create_theme_css_fixture(directory: &Path, id: &str) {
        std::fs::create_dir_all(directory).expect("应创建 CSS 变量主题夹具目录");
        let manifest = json!({
            "schemaVersion": 3,
            "type": "theme",
            "id": id,
            "name": "测试变量主题",
            "description": "用于验证受限 theme.css 的实际级联覆盖。",
            "author": "测试作者"
        });
        std::fs::write(directory.join("theme.json"), manifest.to_string())
            .expect("应写入 CSS 变量主题清单");
        std::fs::write(
            directory.join("theme.css"),
            r#"html.codex-dream-skin {
  --skin-preview-image: url("preview.png");
  --skin-background-image: url("background.png");
  --skin-background-size: cover;
  --skin-background-position-x: 48%;
  --skin-background-position-y: 52%;
  --skin-radius: 21px;
  --skin-blur: 19px;
  --skin-sidebar-opacity: 91%;
  --skin-main-opacity: 79%;
  --skin-header-opacity: 87%;
  --skin-composer-opacity: 95%;
  --skin-card-opacity: 89%;
  --skin-border-width: 2px;
  --skin-shadow-opacity: 64%;
  --skin-texture-opacity: 12%;
}
html.codex-dream-skin[data-dream-shell="dark"] {
  --skin-bg: #07131F; --skin-panel: #102738; --skin-panel-alt: #18394A;
  --skin-accent: #78F0C6; --skin-accent-alt: #78B9FF; --skin-text: #F4FAFF;
  --skin-muted: #B6CBD8; --skin-line: #416477;
}
html.codex-dream-skin[data-dream-shell="light"] {
  --skin-bg: #E7F4F5; --skin-panel: #F8FCFC; --skin-panel-alt: #D9ECEE;
  --skin-accent: #087B65; --skin-accent-alt: #176DB0; --skin-text: #102A34;
  --skin-muted: #4C6871; --skin-line: #91ADB4;
}"#,
        )
        .expect("应写入受限主题变量表");
        for file in ["preview.png", "background.png"] {
            std::fs::write(directory.join(file), b"\x89PNG\r\n\x1a\nfixture")
                .expect("应写入 PNG 夹具");
        }
    }

    /// 验证换皮迁移中的 `create_mode_theme_fixture` 回归场景。
    fn create_mode_theme_fixture(directory: &Path, id: &str) {
        create_theme_css_fixture(directory, id);
        let manifest = json!({
            "schemaVersion": 3,
            "type": "theme",
            "id": id,
            "name": "测试双模式主题",
            "description": "用于验证基础 CSS 与模式增量覆盖。",
            "author": "测试作者",
            "appearance": {
                "supportedColorModes": ["light", "dark"],
                "requirements": {
                    "dark": { "codeThemeId": "codex", "accent": "#339CFF", "contrast": 60 }
                }
            }
        });
        std::fs::write(directory.join("theme.json"), manifest.to_string()).expect("应写入模式清单");
        std::fs::write(directory.join("theme.css"), r#"html.codex-dream-skin {
  --skin-preview-image: url("preview.png"); --skin-background-image: url("background.png");
  --skin-background-size: cover; --skin-background-position-x: 48%; --skin-background-position-y: 52%;
  --skin-radius: 21px; --skin-blur: 19px; --skin-sidebar-opacity: 91%; --skin-main-opacity: 79%;
  --skin-header-opacity: 87%; --skin-composer-opacity: 95%; --skin-card-opacity: 89%;
  --skin-border-width: 2px; --skin-shadow-opacity: 64%; --skin-texture-opacity: 12%;
  --skin-bg: #E7F4F5; --skin-panel: #F8FCFC; --skin-panel-alt: #D9ECEE;
  --skin-accent: #087B65; --skin-accent-alt: #176DB0; --skin-text: #102A34;
  --skin-muted: #4C6871; --skin-line: #91ADB4;
}"#).expect("应写入基础主题变量表");
        std::fs::write(
            directory.join("theme.light.css"),
            r#"html.codex-dream-skin[data-dream-shell="light"] {
  --skin-accent: #087B65;
  --skin-background-image: url("background-light.png");
}"#,
        )
        .expect("应写入浅色覆盖");
        std::fs::write(
            directory.join("theme.dark.css"),
            r#"html.codex-dream-skin[data-dream-shell="dark"] {
  --skin-bg: #07131F; --skin-accent: #78F0C6; --skin-text: #F4FAFF;
  --skin-background-image: url("background-dark.jpg");
}"#,
        )
        .expect("应写入深色覆盖");
        std::fs::write(
            directory.join("background-light.png"),
            b"\x89PNG\r\n\x1a\nlight-fixture",
        )
        .expect("应写入浅色 PNG 夹具");
        std::fs::write(
            directory.join("background-dark.jpg"),
            [0xff, 0xd8, 0xff, 0xdb, 0x00, 0x01, 0xff, 0xd9],
        )
        .expect("应写入深色 JPEG 夹具");
    }

    /// 验证换皮迁移中的 `create_zip` 回归场景。
    fn create_zip(archive_path: &Path, fixture: &Path, wrapper: Option<&str>) {
        create_manifestless_legacy_zip(archive_path, fixture, wrapper, &LEGACY_REQUIRED_FILES);
    }

    /// 验证换皮迁移中的 `create_manifestless_legacy_zip` 回归场景。
    fn create_manifestless_legacy_zip(
        archive_path: &Path,
        fixture: &Path,
        wrapper: Option<&str>,
        files: &[&str],
    ) {
        let archive_file = std::fs::File::create(archive_path).expect("应创建无清单旧皮肤 ZIP");
        let mut writer = zip::ZipWriter::new(archive_file);
        for file_name in files {
            let path = wrapper
                .map(|root| format!("{root}/{file_name}"))
                .unwrap_or_else(|| (*file_name).to_owned());
            writer
                .start_file(path, SimpleFileOptions::default())
                .expect("应创建旧皮肤 ZIP 条目");
            writer
                .write_all(&std::fs::read(fixture.join(file_name)).expect("应读取旧皮肤夹具"))
                .expect("应写入旧皮肤 ZIP 条目");
        }
        writer.finish().expect("应完成无清单旧皮肤 ZIP");
    }

    /// 验证换皮迁移中的 `create_service` 回归场景。
    fn create_service(root: &Path) -> SkinService {
        let builtin_root = root.join("builtin");
        let user_root = root.join("user");
        std::fs::create_dir_all(&builtin_root).expect("应创建内置目录");
        std::fs::create_dir_all(&user_root).expect("应创建用户目录");
        SkinService::new(builtin_root, user_root)
    }

    /// 验证换皮迁移中的 `archive_names` 回归场景。
    fn archive_names(bytes: Vec<u8>) -> Vec<String> {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("导出结果必须是有效 ZIP");
        let mut names = (0..archive.len())
            .map(|index| {
                archive
                    .by_index(index)
                    .expect("应读取导出 ZIP 条目")
                    .name()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }
