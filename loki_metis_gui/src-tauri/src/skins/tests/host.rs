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
    /// 重连候选只有通过宿主页与进程归属复核后才能替换当前会话。
    fn reconnected_session_is_adopted_only_after_full_host_validation() {
        assert!(matches!(
            reconnect_validation_decision(SkinHostKind::WorkBuddy, Ok(true)),
            ReconnectValidationDecision::Accept
        ));

        let ReconnectValidationDecision::Retry(error) =
            reconnect_validation_decision(SkinHostKind::WorkBuddy, Ok(false))
        else {
            panic!("未通过宿主校验的候选必须关闭并重试");
        };
        assert_eq!(error.code, "skin.cdp_rejected");

        let ReconnectValidationDecision::Retry(error) = reconnect_validation_decision(
            SkinHostKind::WorkBuddy,
            Err(AppError::new("skin.cdp_failed", "测试错误")),
        ) else {
            panic!("普通连接故障应关闭候选并允许有界重试");
        };
        assert_eq!(error.code, "skin.cdp_failed");
    }

    #[test]
    /// WorkBuddy 归属检查不可判定时必须失败关闭，不能继续采用或降级重试。
    fn reconnected_workbuddy_session_fails_closed_on_ownership_inspection_error() {
        for code in [
            "skin.workbuddy_cdp_owner_inspection_failed",
            "skin.workbuddy_process_inspection_failed",
        ] {
            let ReconnectValidationDecision::Reject(error) = reconnect_validation_decision(
                SkinHostKind::WorkBuddy,
                Err(AppError::new(code, "测试错误")),
            ) else {
                panic!("归属检查故障必须立即拒绝重连候选");
            };
            assert_eq!(error.code, code);
        }
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
    /// WorkBuddy 页面探针只接受本机官方页面标记，并拒绝 Codex 或远端页面。
    fn workbuddy_page_probe_rejects_codex_and_remote_pages() {
        assert!(WORKBUDDY_PROBE_SCRIPT.contains("document.title === 'WorkBuddy'"));
        assert!(WORKBUDDY_PROBE_SCRIPT.contains("body?.dataset.applicationName === 'workbuddy'"));
        assert!(WORKBUDDY_PROBE_SCRIPT.contains("body?.dataset.electronDesktop === 'true'"));
        assert!(WORKBUDDY_PROBE_SCRIPT.contains("body?.dataset.productName === 'WorkBuddy'"));
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
    /// 注入事务必须精确匹配 nonce，并只在清理脚本成功后放弃回滚所有权。
    fn injection_transaction_scripts_preserve_exact_rollback_ownership() {
        let rollback = rollback_injection_transaction_expression("tx-\"quoted\"")
            .expect("事务表达式应可编码")
            .to_string();
        assert!(rollback.contains(
            "window.__LOKI_METIS_SKIN_TRANSACTION__ !== \"tx-\\\"quoted\\\"\""
        ));
        assert!(rollback.contains("return null"));
        assert!(rollback.contains("const removed = (() =>"));
        assert!(rollback.contains(
            "if (removed === true) delete window.__LOKI_METIS_SKIN_TRANSACTION__"
        ));
        assert!(rollback.contains("return removed === true"));

        let commit = commit_injection_transaction_expression("tx-1")
            .expect("提交表达式应可编码")
            .to_string();
        assert!(commit.contains(
            "window.__LOKI_METIS_SKIN_TRANSACTION__ !== \"tx-1\""
        ));
        assert!(commit.contains("delete window.__LOKI_METIS_SKIN_TRANSACTION__"));
    }

    #[test]
    /// 页面任务被取消并丢弃后，外层事务仍必须持有副作用发生前登记的 target。
    fn injection_transaction_tracker_survives_page_future_drop() {
        let transaction = InjectionTransaction::new("tx-1");
        let page_future_owner = transaction.clone();
        page_future_owner.track("target-a".into());
        drop(page_future_owner);
        assert_eq!(
            transaction.tracked_targets(),
            std::collections::BTreeSet::from(["target-a".to_owned()])
        );
    }

    #[test]
    /// 已结束的 CDP handler 不得移交给 watcher 并把安装误报为运行中。
    fn handler_task_guard_rejects_finished_task() {
        tauri::async_runtime::block_on(async {
            let task = tokio::spawn(async {});
            while !task.is_finished() {
                tokio::task::yield_now().await;
            }
            let mut guard = HandlerTaskGuard::new(task);
            assert!(guard.take().is_none());
        });
    }

    #[test]
    /// 任一宿主页清理失败都必须触发端点兜底，不能被其它页面的成功掩盖。
    fn partial_page_cleanup_is_an_error() {
        assert_eq!(finish_cleanup_report(2, 0).expect("全量成功"), 2);
        let error = finish_cleanup_report(1, 1).expect_err("部分失败必须上抛");
        assert_eq!(error.code, "skin.cdp_cleanup_failed");
        assert_eq!(error.details, ["removed_pages=1", "failed_pages=1"]);
    }

    #[test]
    /// 验证 WorkBuddy 专属适配器受宿主标记约束，并具备独立样式与对称清理接口。
    fn workbuddy_compatibility_adapter_has_bounded_lifecycle() {
        assert!(HOST_COMPATIBILITY_SCRIPT.contains("const VERSION = \"5\""));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("const VERSION = \"5\""));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("data-application-name"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("data-electron-desktop"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("data-product-name"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains(".teams-container"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains(".conversation-list"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains(".main-content"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains(".wb-cb-chat"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("data-cb-chat-input-toolbar-selector"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("loki-metis-workbuddy-skin-compat-style"));
        assert!(WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("cleanup"));
        assert!(!WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("fetch("));
        assert!(!WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("XMLHttpRequest"));
        assert!(!WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("_editable_"));
        assert!(!WORKBUDDY_HOST_COMPATIBILITY_SCRIPT.contains("class*="));
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
    /// 验证 WorkBuddy renderer 的自定义端口优先于 9441 宿主默认回退。
    fn workbuddy_endpoint_candidates_keep_renderer_port_and_host_default() {
        assert_eq!(
            host_endpoint_candidates_from_commands(
                SkinHostKind::WorkBuddy,
                vec![(
                    51,
                    "WorkBuddy.exe --type=renderer --remote-debugging-port=9442".into(),
                )],
                None,
            ),
            vec![CdpEndpoint::new(9442), CdpEndpoint::new(9441)]
        );
    }

    #[test]
    /// WorkBuddy 显式声明 9341 时必须保留它，不能误当成 Codex 自动回退删除。
    fn workbuddy_endpoint_candidates_preserve_explicit_codex_default_port() {
        assert_eq!(
            host_endpoint_candidates_from_commands(
                SkinHostKind::WorkBuddy,
                vec![(
                    52,
                    "WorkBuddy.exe --type=renderer --remote-debugging-port=9341".into(),
                )],
                None,
            ),
            vec![CdpEndpoint::new(9341), CdpEndpoint::new(9441)]
        );
    }

    #[test]
    /// 验证 WMI 命令行不可用时，刚验证过的动态端点仍能跨恢复后的首次重扫保留。
    fn workbuddy_endpoint_candidates_prefer_verified_runtime_hint_without_wmi() {
        assert_eq!(
            host_endpoint_candidates_from_commands(
                SkinHostKind::WorkBuddy,
                Vec::new(),
                Some(CdpEndpoint::new(9442)),
            ),
            vec![CdpEndpoint::new(9442), CdpEndpoint::new(9441)]
        );
    }

    #[test]
    /// WMI 命令行从可用降级为空时，同一 WorkBuddy 根的实例 ID 必须保持不变。
    fn workbuddy_instance_id_does_not_depend_on_command_line() {
        let process = |command_line: &str| PlatformCodexProcess {
            pid: 77,
            executable: PathBuf::from(
                r"C:\Users\test\AppData\Local\Programs\WorkBuddy\WorkBuddy.exe",
            ),
            command_line: command_line.into(),
        };
        let with_wmi = resolved_instance_for_host(
            SkinHostKind::WorkBuddy,
            process("WorkBuddy.exe --remote-debugging-port=9442"),
        );
        let without_wmi =
            resolved_instance_for_host(SkinHostKind::WorkBuddy, process(""));
        assert_eq!(with_wmi.id, without_wmi.id);
        assert_eq!(with_wmi.debug_port, Some(9442));
        assert_eq!(without_wmi.debug_port, None);
    }

    #[test]
    /// 多实例时只选取用户指定的 WorkBuddy 根 PID，不得改用当前其它实例。
    fn selected_workbuddy_root_binding_rejects_replacement_instance() {
        let process = |pid| PlatformCodexProcess {
            pid,
            executable: PathBuf::from("/Applications/WorkBuddy.app/Contents/MacOS/WorkBuddy"),
            command_line: format!("WorkBuddy --remote-debugging-port={}", 9400 + pid),
        };
        let processes = vec![process(41), process(42)];

        assert_eq!(trusted_workbuddy_root_pid(&processes, Some(42)), Some(42));
        assert_eq!(trusted_workbuddy_root_pid(&processes, Some(43)), None);
        assert_eq!(trusted_workbuddy_root_pid(&processes, None), None);
        assert_eq!(trusted_workbuddy_root_pid(&[process(41)], None), Some(41));
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
    }

    #[test]
    /// WorkBuddy 从根 A 交接到根 B 时，即使复用了同一端点，也不能把注入状态登记到旧 ID。
    fn workbuddy_binding_rejects_single_root_pid_handoff() {
        let process = |pid| PlatformCodexProcess {
            pid,
            executable: PathBuf::from(
                r"C:\Users\test\AppData\Local\Programs\WorkBuddy\WorkBuddy.exe",
            ),
            command_line: "WorkBuddy.exe --remote-debugging-port=9442".into(),
        };
        let endpoint = CdpEndpoint::new(9442);
        let root_a = resolved_instance_for_host(SkinHostKind::WorkBuddy, process(70));
        let root_b = resolved_instance_for_host(SkinHostKind::WorkBuddy, process(71));

        let bound = unique_workbuddy_target_for_endpoint(&[root_a.clone()], endpoint)
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
