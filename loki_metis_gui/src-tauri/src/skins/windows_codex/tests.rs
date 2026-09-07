    use super::{
        decide_launch, filter_verified_command_lines, is_gui_executable, is_store_gui_path,
        is_verified_gui_process_path, path_executable_candidates, quote_windows_argument,
        select_target_window, traditional_candidates, GuiProcess, LaunchDecision, WindowCandidate,
        WmiProcess, STORE_PACKAGE_FAMILY,
    };
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use windows::Win32::Foundation::HWND;

    #[test]
    /// 验证换皮迁移中的 `foreground_window_selection_requires_exact_visible_unowned_pid` 回归场景。
    fn foreground_window_selection_requires_exact_visible_unowned_pid() {
        let mut handles = [0u8; 4];
        let first = HWND((&mut handles[0] as *mut u8).cast());
        let second = HWND((&mut handles[1] as *mut u8).cast());
        let third = HWND((&mut handles[2] as *mut u8).cast());
        let fourth = HWND((&mut handles[3] as *mut u8).cast());
        let candidates = [
            WindowCandidate {
                handle: first,
                pid: 41,
                visible: true,
                owned: false,
            },
            WindowCandidate {
                handle: second,
                pid: 42,
                visible: false,
                owned: false,
            },
            WindowCandidate {
                handle: third,
                pid: 42,
                visible: true,
                owned: true,
            },
            WindowCandidate {
                handle: fourth,
                pid: 42,
                visible: true,
                owned: false,
            },
        ];
        assert_eq!(select_target_window(&candidates, 42), Some(fourth));
        assert_eq!(select_target_window(&candidates, 99), None);
    }

    #[test]
    /// 验证换皮迁移中的 `accepts_store_and_traditional_gui_paths` 回归场景。
    fn accepts_store_and_traditional_gui_paths() {
        let store = Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
        );
        assert!(is_gui_executable(store));
        assert!(is_store_gui_path(store));
        assert!(is_gui_executable(Path::new(
            r"C:\Users\test\AppData\Local\Programs\Codex\Codex.exe"
        )));
        assert!(is_verified_gui_process_path(store, &[]));
        assert!(!is_verified_gui_process_path(
            Path::new(r"C:\Untrusted\ChatGPT.exe"),
            &[]
        ));
        assert!(is_verified_gui_process_path(
            Path::new(r"C:\Tools\ChatGPT.exe"),
            &[PathBuf::from(r"c:\tools\chatgpt.exe")]
        ));
    }

    #[test]
    /// 验证换皮迁移中的 `rejects_store_cli_and_execution_alias_codex_paths` 回归场景。
    fn rejects_store_cli_and_execution_alias_codex_paths() {
        assert!(!is_gui_executable(Path::new(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0\app\resources\codex.exe"
        )));
        assert!(!is_gui_executable(Path::new(
            r"C:\Users\test\AppData\Local\Microsoft\WindowsApps\Codex.exe"
        )));
        assert!(!is_gui_executable(Path::new(
            r"C:\Users\test\AppData\Local\OpenAI\Codex\bin\b99306303521e97e\codex.exe"
        )));
    }

    #[test]
    /// 验证换皮迁移中的 `gui_process_identity_keeps_pid_and_verified_path_together` 回归场景。
    fn gui_process_identity_keeps_pid_and_verified_path_together() {
        let process = GuiProcess {
            pid: 42,
            path: PathBuf::from(r"C:\Tools\ChatGPT.exe"),
        };
        assert_eq!(process.pid, 42);
        assert!(is_verified_gui_process_path(
            &process.path,
            &[PathBuf::from(r"c:\tools\chatgpt.exe")]
        ));
        assert!(!is_verified_gui_process_path(
            Path::new(r"C:\Tools\resources\codex.exe"),
            &[PathBuf::from(r"C:\Tools\ChatGPT.exe")]
        ));
    }

    #[test]
    /// 验证换皮迁移中的 `wmi_command_lines_require_a_matching_verified_pid` 回归场景。
    fn wmi_command_lines_require_a_matching_verified_pid() {
        let verified = vec![GuiProcess {
            pid: 42,
            path: PathBuf::from(r"C:\Tools\ChatGPT.exe"),
        }];
        let rows = vec![
            WmiProcess {
                process_id: 42,
                command_line: Some("ChatGPT.exe --remote-debugging-port=9222".into()),
            },
            WmiProcess {
                process_id: 99,
                command_line: Some("ChatGPT.exe --remote-debugging-port=9444".into()),
            },
            WmiProcess {
                process_id: 42,
                command_line: None,
            },
        ];
        assert_eq!(
            filter_verified_command_lines(&verified, rows),
            vec![(42, "ChatGPT.exe --remote-debugging-port=9222".to_owned())]
        );
    }

    #[test]
    /// 验证换皮迁移中的 `path_lookup_generates_gui_candidate_names_without_spawning_a_console` 回归场景。
    fn path_lookup_generates_gui_candidate_names_without_spawning_a_console() {
        let paths = path_executable_candidates(Some(OsStr::new(r"C:\Tools;D:\Desktop Apps")));
        assert_eq!(
            paths,
            vec![
                PathBuf::from(r"C:\Tools\ChatGPT.exe"),
                PathBuf::from(r"C:\Tools\Codex.exe"),
                PathBuf::from(r"D:\Desktop Apps\ChatGPT.exe"),
                PathBuf::from(r"D:\Desktop Apps\Codex.exe"),
            ]
        );
    }

    #[test]
    /// 验证换皮迁移中的 `store_activation_precedes_traditional_fallback` 回归场景。
    fn store_activation_precedes_traditional_fallback() {
        let fallback = PathBuf::from(r"C:\Tools\ChatGPT.exe");
        assert_eq!(
            decide_launch(true, Some(fallback.clone()), true),
            LaunchDecision::StartedStore
        );
        assert_eq!(
            decide_launch(false, Some(fallback.clone()), true),
            LaunchDecision::StartTraditional(fallback)
        );
        assert_eq!(
            decide_launch(false, None, true),
            LaunchDecision::StoreFailed
        );
        assert_eq!(decide_launch(false, None, false), LaunchDecision::NotFound);
    }

    #[test]
    /// 验证换皮迁移中的 `traditional_candidates_cover_user_and_system_locations` 回归场景。
    fn traditional_candidates_cover_user_and_system_locations() {
        let candidates = traditional_candidates(
            Some(OsStr::new(r"C:\Users\test\AppData\Local")),
            Some(OsStr::new(r"C:\Program Files")),
            Some(OsStr::new(r"C:\Program Files (x86)")),
        );
        assert_eq!(candidates.len(), 10);
        assert!(candidates
            .iter()
            .any(|path| path.ends_with("Programs/ChatGPT/ChatGPT.exe")));
        assert!(candidates
            .iter()
            .any(|path| path.ends_with("Codex/Codex.exe")));
    }

    #[test]
    /// 验证换皮迁移中的 `store_identity_matches_supported_package_family` 回归场景。
    fn store_identity_matches_supported_package_family() {
        assert_eq!(STORE_PACKAGE_FAMILY, "OpenAI.Codex_2p2nqsd0c76g0");
    }

    #[test]
    /// 验证换皮迁移中的 `store_restart_arguments_quote_spaces_without_command_injection` 回归场景。
    fn store_restart_arguments_quote_spaces_without_command_injection() {
        assert_eq!(quote_windows_argument("--flag=yes"), "--flag=yes");
        assert_eq!(
            quote_windows_argument(r"--user-data-dir=C:\Company Profile"),
            r#""--user-data-dir=C:\Company Profile""#
        );
        assert_eq!(
            quote_windows_argument("value\"quoted"),
            "\"value\\\"quoted\""
        );
    }
