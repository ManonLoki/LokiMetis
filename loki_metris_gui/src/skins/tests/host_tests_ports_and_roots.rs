// 本文件是 host.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：Codex 声明端口与候选不一致时不能借“唯一进程”回退冒认端点。；首次注入提交前 owner 根 PID 消失或换成其它进程时必须拒绝提交。；已验证 CDP 只在 WorkBuddy 恰有一个可信树根时复用，零根和多根都须恢复。

    #[test]
    /// Codex 声明端口与候选不一致时不能借“唯一进程”回退冒认端点。
    fn codex_declared_port_mismatch_rejects_root_fallback() {
        let process = |pid, command_line: &str| PlatformHostProcess {
            pid,
            executable: PathBuf::from(r"C:\Program Files\ChatGPT\ChatGPT.exe"),
            command_line: command_line.into(),
        };
        assert_eq!(
            trusted_codex_root_pid(
                &[process(41, "ChatGPT.exe --remote-debugging-port=9342")],
                CdpEndpoint::new(9341),
                None,
            ),
            None
        );
        assert_eq!(
            trusted_codex_root_pid(
                &[process(41, "ChatGPT.exe")],
                CdpEndpoint::new(9341),
                None,
            ),
            Some(41)
        );
    }

    #[test]
    /// 首次注入提交前 owner 根 PID 消失或换成其它进程时必须拒绝提交。
    fn initial_injection_rejects_endpoint_owner_change() {
        assert!(endpoint_owner_is_stable(41, Some(41)));
        assert!(!endpoint_owner_is_stable(41, Some(42)));
        assert!(!endpoint_owner_is_stable(41, None));
    }

    #[test]
    /// 已验证 CDP 只在 WorkBuddy 恰有一个可信树根时复用，零根和多根都须恢复。
    fn ready_workbuddy_runtime_is_reused_only_for_a_single_root() {
        assert!(ready_runtime_can_be_reused(SkinHostKind::Codex, 3));
        assert!(!ready_runtime_can_be_reused(SkinHostKind::WorkBuddy, 0));
        assert!(ready_runtime_can_be_reused(SkinHostKind::WorkBuddy, 1));
        assert!(!ready_runtime_can_be_reused(SkinHostKind::WorkBuddy, 2));
    }

    #[test]
    /// WorkBuddy 存活时的 CDP 页面故障应进入确认恢复，取消与非连接错误不得被改写。
    fn workbuddy_connection_failures_use_recovery_contract() {
        assert!(should_request_workbuddy_recovery(
            SkinHostKind::WorkBuddy,
            "skin.cdp_request_timeout",
            true,
        ));
        assert!(should_request_workbuddy_recovery(
            SkinHostKind::WorkBuddy,
            "skin.workbuddy_page_not_found",
            true,
        ));
        assert!(!should_request_workbuddy_recovery(
            SkinHostKind::WorkBuddy,
            "skin.cdp_request_timeout",
            false,
        ));
        assert!(!should_request_workbuddy_recovery(
            SkinHostKind::Codex,
            "skin.cdp_request_timeout",
            true,
        ));
        assert!(!should_request_workbuddy_recovery(
            SkinHostKind::WorkBuddy,
            "skin.workbuddy_process_inspection_failed",
            true,
        ));
        for code in [
            "skin.codex_cdp_owner_inspection_failed",
            "skin.codex_process_inspection_failed",
            "skin.codex_process_inspection_timeout",
            "skin.workbuddy_cdp_owner_inspection_failed",
            "skin.workbuddy_process_inspection_failed",
            "skin.workbuddy_process_inspection_timeout",
        ] {
            assert!(is_host_inspection_error(code));
            assert!(!is_workbuddy_connection_error(code));
        }
    }

    #[test]
    /// WorkBuddy 从根 A 交接到根 B 时，即使复用了同一端点，也不能把注入状态登记到旧 ID。
    fn workbuddy_binding_rejects_single_root_pid_handoff() {
        let process = |pid| PlatformHostProcess {
            pid,
            executable: PathBuf::from(
                r"C:\Users\test\AppData\Local\Programs\WorkBuddy\WorkBuddy.exe",
            ),
            command_line: "WorkBuddy.exe --remote-debugging-port=9442".into(),
        };
        let endpoint = CdpEndpoint::new(9442);
        let root_a = resolved_instance_for_host(SkinHostKind::WorkBuddy, process(70));
        let root_b = resolved_instance_for_host(SkinHostKind::WorkBuddy, process(71));

        let bound = unique_workbuddy_target_for_endpoint(std::slice::from_ref(&root_a), endpoint)
            .expect("唯一根应绑定到已验证端点");
        assert!(workbuddy_target_binding_matches(
            &root_a, &bound, endpoint
        ));
        assert!(!workbuddy_target_binding_matches(
            &root_a, &root_b, endpoint
        ));
        assert!(unique_workbuddy_target_for_endpoint(&[], endpoint).is_none());
        assert!(unique_workbuddy_target_for_endpoint(&[root_a, root_b], endpoint).is_none());
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
