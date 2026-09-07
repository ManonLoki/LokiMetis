    #[test]
    /// 验证换皮迁移中的 `page_probe_requires_codex_markers_and_app_protocol` 回归场景。
    fn page_probe_requires_codex_markers_and_app_protocol() {
        assert!(PROBE_SCRIPT.contains("main[data-app-shell-main-surface]"));
        assert!(PROBE_SCRIPT.contains("['Codex', 'ChatGPT'].includes(document.title)"));
        assert!(PageProbe {
            codex: true,
            work_buddy: false,
            url: "app://-/index.html".into(),
        }
        .is_verified_codex());
        assert!(!PageProbe {
            codex: true,
            work_buddy: false,
            url: "https://example.com".into(),
        }
        .is_verified_codex());
        assert!(!PageProbe {
            codex: true,
            work_buddy: false,
            url: "app://-/index.html?initialRoute=%2Favatar-overlay".into(),
        }
        .is_verified_codex());
        assert!(!PageProbe {
            codex: true,
            work_buddy: false,
            url: "app://-/avatar-overlay-composition-surface.html".into(),
        }
        .is_verified_codex());
        assert!(!PageProbe {
            codex: false,
            work_buddy: false,
            url: "app://-/index.html".into(),
        }
        .is_verified_codex());
    }

    #[test]
    /// 验证换皮迁移中的 `existing_and_cold_codex_use_the_aggressive_page_ready_limit` 回归场景。
    fn existing_and_cold_codex_use_the_aggressive_page_ready_limit() {
        assert_eq!(
            ConnectionSource::Existing.page_ready_timeout(),
            EXISTING_CODEX_PAGE_READY_TIMEOUT
        );
        assert_eq!(
            ConnectionSource::Launched.page_ready_timeout(),
            CODEX_PAGE_READY_TIMEOUT
        );
        assert_eq!(EXISTING_CODEX_PAGE_READY_TIMEOUT, CODEX_PAGE_READY_TIMEOUT);
    }

    #[test]
    /// 验证换皮迁移中的 `initial_injection_reconnects_only_before_a_main_page_is_verified` 回归场景。
    fn initial_injection_reconnects_only_before_a_main_page_is_verified() {
        assert!(should_reconnect_initial_session(&InjectionReport {
            verified_pages: 0,
            injected_pages: 0,
            failed_pages: 4,
            ..InjectionReport::default()
        }));
        assert!(!should_reconnect_initial_session(&InjectionReport {
            verified_pages: 1,
            injected_pages: 1,
            failed_pages: 2,
            ..InjectionReport::default()
        }));
        assert!(!should_reconnect_initial_session(
            &InjectionReport::default()
        ));
    }

    #[test]
    /// 验证换皮迁移中的 `status_does_not_wait_for_mutating_operation_lock` 回归场景。
    fn status_does_not_wait_for_mutating_operation_lock() {
        tauri::async_runtime::block_on(async {
            let root = temp_directory("status-operation-lock");
            let service = create_service(&root);
            let _operation = service.operation.lock().await;
            let status = tokio::time::timeout(
                Duration::from_millis(50),
                service.status(SkinHostKind::Codex),
            )
                .await
                .expect("状态查询不应等待安装或卸载操作锁");
            assert!(!status.installed);
            std::fs::remove_dir_all(root).expect("应清理测试目录");
        });
    }

    #[test]
    fn workbuddy_page_probe_rejects_codex_and_remote_pages() {
        assert!(WORKBUDDY_PROBE_SCRIPT.contains("document.title === 'WorkBuddy'"));
        assert!(PageProbe {
            codex: false,
            work_buddy: true,
            url: "file:///Applications/WorkBuddy.app/renderer/index.html".into(),
        }
        .is_verified_workbuddy());
        assert!(!PageProbe {
            codex: true,
            work_buddy: false,
            url: "app://-/index.html".into(),
        }
        .is_verified_workbuddy());
        assert!(!PageProbe {
            codex: false,
            work_buddy: true,
            url: "https://example.com".into(),
        }
        .is_verified_workbuddy());
    }

    #[test]
    /// 验证换皮迁移中的 `debug_port_parser_accepts_supported_forms_and_rejects_invalid_values` 回归场景。
    fn debug_port_parser_accepts_supported_forms_and_rejects_invalid_values() {
        assert_eq!(
            debug_port_from_command_line("ChatGPT.exe --remote-debugging-port=9222"),
            Some(9222)
        );
        assert_eq!(
            debug_port_from_command_line("ChatGPT.exe --remote-debugging-port 9333"),
            Some(9333)
        );
        assert_eq!(
            debug_port_from_command_line("ChatGPT.exe --remote-debugging-port=\"9444\""),
            Some(9444)
        );
        assert_eq!(
            debug_port_from_command_line("ChatGPT.exe --remote-debugging-port=0"),
            None
        );
        assert_eq!(
            debug_port_from_command_line("ChatGPT.exe --remote-debugging-port=65536"),
            None
        );
        assert_eq!(debug_port_from_command_line("ChatGPT.exe --flag"), None);
    }

    #[test]
    /// 验证换皮迁移中的 `restart_arguments_preserve_profile_and_replace_debug_flags` 回归场景。
    fn restart_arguments_preserve_profile_and_replace_debug_flags() {
        let command = r#""C:\Program Files\ChatGPT\ChatGPT.exe" --user-data-dir="D:\Profiles\company" --remote-debugging-address 0.0.0.0 --remote-debugging-port=9222 --feature=yes"#;
        assert_eq!(
            command_line_arguments(command).first().map(String::as_str),
            Some(r"C:\Program Files\ChatGPT\ChatGPT.exe")
        );
        let arguments = reusable_process_arguments(command);
        assert_eq!(
            arguments,
            vec![r"--user-data-dir=D:\Profiles\company", "--feature=yes"]
        );
        assert_eq!(user_data_profile(&arguments).as_deref(), Some("company"));
        assert_eq!(
            user_data_directory(&arguments),
            Some(PathBuf::from(r"D:\Profiles\company"))
        );
        assert_eq!(
            user_data_profile(&[
                "--user-data-dir".into(),
                "/Users/example/Library/Application Support/Codex/personal/".into(),
            ])
            .as_deref(),
            Some("personal")
        );
    }

    #[test]
    /// 验证换皮迁移中的 `instance_discovery_excludes_electron_helper_processes` 回归场景。
    fn instance_discovery_excludes_electron_helper_processes() {
        assert!(is_primary_codex_command_line(
            r#""C:\Program Files\ChatGPT\ChatGPT.exe" --user-data-dir=D:\personal"#
        ));
        assert!(!is_primary_codex_command_line(
            r#""C:\Program Files\ChatGPT\ChatGPT.exe" --type=renderer --user-data-dir=D:\personal"#
        ));
        assert!(!is_primary_codex_command_line(
            r#""C:\Program Files\ChatGPT\ChatGPT.exe" --utility-sub-type=network.mojom.NetworkService"#
        ));
    }

    #[test]
    /// 验证换皮迁移中的 `dynamic_endpoint_candidates_prefer_newer_processes_and_keep_fixed_fallback` 回归场景。
    fn dynamic_endpoint_candidates_prefer_newer_processes_and_keep_fixed_fallback() {
        let endpoints = endpoint_candidates_from_commands(vec![
            (10, "ChatGPT.exe --remote-debugging-port=9222".into()),
            (30, "ChatGPT.exe --remote-debugging-port 9555".into()),
            (20, "ChatGPT.exe --remote-debugging-port=9222".into()),
            (40, "ChatGPT.exe --remote-debugging-port=0".into()),
        ]);
        assert_eq!(
            endpoints,
            vec![
                CdpEndpoint::new(9555),
                CdpEndpoint::new(9222),
                CdpEndpoint::default(),
            ]
        );
        assert_eq!(
            endpoint_candidates_from_commands(vec![(
                50,
                "ChatGPT.exe --remote-debugging-port=9341".into(),
            )]),
            vec![CdpEndpoint::default()]
        );
    }

    #[test]
    /// 验证换皮迁移中的 `macos_command_line_filter_requires_the_verified_executable` 回归场景。
    fn macos_command_line_filter_requires_the_verified_executable() {
        let output = "  50 /Applications/ChatGPT.app/Contents/MacOS/ChatGPT --remote-debugging-port=9222\n  60 /tmp/ChatGPT --remote-debugging-port=9444\n  70 /Applications/ChatGPT.app/Contents/MacOS/ChatGPT Helper --type=gpu";
        let matches = matching_process_command_lines(
            output,
            Path::new("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"),
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, 50);
        assert_eq!(debug_port_from_command_line(&matches[0].1), Some(9222));
    }

    #[test]
    /// 验证换皮迁移中的 `websocket_allowlist_requires_the_selected_loopback_port` 回归场景。
    fn websocket_allowlist_requires_the_selected_loopback_port() {
        let endpoint = CdpEndpoint::new(9222);
        assert!(is_allowed_websocket(
            "ws://127.0.0.1:9222/devtools/browser/id",
            endpoint,
        ));
        assert!(is_allowed_websocket(
            "ws://[::1]:9222/devtools/browser/id",
            endpoint,
        ));
        assert!(!is_allowed_websocket(
            "ws://127.0.0.1:9341/devtools/browser/id",
            endpoint,
        ));
        assert!(!is_allowed_websocket(
            "wss://example.com:9222/devtools/browser/id",
            endpoint,
        ));
    }

    /// 验证换皮迁移中的 `create_legacy_fixture_with_colors` 回归场景。
    fn create_legacy_fixture_with_colors(
        directory: &Path,
        id: &str,
        colors: Option<serde_json::Value>,
    ) {
        create_fixture(directory, id);
        if let Some(colors) = colors {
            let path = directory.join("theme.json");
            let mut manifest: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("应读取旧皮肤清单"))
                    .expect("旧皮肤清单应为 JSON");
            manifest["colors"] = colors;
            std::fs::write(&path, manifest.to_string()).expect("应写入旧皮肤配色");
        }
        // 转换目标是 v3 主题，会校验图片签名，因此夹具需要真实 PNG 头。
        std::fs::write(
            directory.join("qq2007-sky.png"),
            b"\x89PNG\r\n\x1a\nlegacy-art",
        )
        .expect("应写入 PNG 夹具");
    }

    /// 验证换皮迁移中的 `dual_palette_fixture_colors` 回归场景。
    fn dual_palette_fixture_colors() -> serde_json::Value {
        json!({
            "light": {
                "background": "#E7F4F5",
                "panel": "#F8FCFC",
                "panelAlt": "#D9ECEE",
                "accent": "#087B65",
                "accentAlt": "#176DB0",
                "text": "#102A34",
                "muted": "#4C6871",
                "line": "#91ADB4"
            },
            "dark": {
                "background": "#07131F",
                "panel": "#102738",
                "panelAlt": "#18394A",
                "accent": "#78F0C6",
                "accentAlt": "#78B9FF",
                "text": "#F4FAFF",
                "muted": "#B6CBD8",
                "line": "#416477"
            }
        })
    }

    /// 验证换皮迁移中的 `theme_css_block` 回归场景。
    fn theme_css_block(css: &str, selector: &str) -> String {
        let start = css.find(selector).expect("变量表应包含目标选择器");
        let open = css[start..].find('{').expect("选择器后应有变量块") + start;
        let close = css[open..].find('}').expect("变量块应闭合") + open;
        css[open + 1..close].to_owned()
    }

    /// 验证换皮迁移中的 `theme_directory_files` 回归场景。
    fn theme_directory_files(directory: &Path) -> HashSet<String> {
        std::fs::read_dir(directory)
            .expect("应读取主题目录")
            .map(|entry| {
                entry
                    .expect("应读取主题文件")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }
