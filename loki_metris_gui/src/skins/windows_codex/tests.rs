use super::install_identity::{
    STORE_PACKAGE_FAMILY, TrustedPublisher, package_identity_matches_store_path,
    publisher_subject_matches, store_path_matches_root,
};
use super::{
    GuiProcess, LaunchDecision, ProcessWaitState, WindowCandidate, classify_process_wait,
    decide_launch, process_belongs_to_verified_root, quote_windows_argument, select_target_window,
};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HWND, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};

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
/// Store 宿主必须位于 WindowsApps 精确包目录与固定 app 可执行位置。
fn accepts_only_exact_store_package_identity_under_program_files() {
    let root = Path::new(r"C:\Program Files");
    let store = Path::new(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
    );
    assert!(store_path_matches_root(store, root));
    assert!(!store_path_matches_root(
        Path::new(
            r"C:\Temp\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe"
        ),
        root,
    ));
}

#[test]
/// 模糊包前缀、错误 publisher、非法版本与包内 CLI 都不能冒充 Store GUI。
fn rejects_lookalike_store_package_and_nested_cli_paths() {
    let root = Path::new(r"C:\Program Files");
    for path in [
        r"C:\Program Files\WindowsApps\OpenAI.Codex_Evil_26.715.4045.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__attacker\app\ChatGPT.exe",
        r"C:\Program Files\WindowsApps\OpenAI.Codex_latest_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0\app\resources\Codex.exe",
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0\ChatGPT.exe",
    ] {
        assert!(!store_path_matches_root(Path::new(path), root), "{path}");
    }
}

#[test]
/// Store 进程必须由 Package API 返回的 family/full name 与路径目录三方一致。
fn store_package_identity_binds_family_publisher_and_path() {
    let root = Path::new(r"C:\Program Files");
    let full_name = "OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0";
    let path = Path::new(
        r"C:\Program Files\WindowsApps\OpenAI.Codex_26.715.4045.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
    );
    assert!(package_identity_matches_store_path(
        path,
        root,
        STORE_PACKAGE_FAMILY,
        full_name,
    ));
    assert!(!package_identity_matches_store_path(
        path,
        root,
        "OpenAI.Codex_attacker",
        full_name,
    ));
    assert!(!package_identity_matches_store_path(
        path,
        root,
        STORE_PACKAGE_FAMILY,
        "OpenAI.Codex_99.0.0.0_x64__2p2nqsd0c76g0",
    ));
}

#[test]
/// 传统宿主发行方只接受精确主体名，合法签名但相似组织名仍拒绝。
fn traditional_publishers_use_exact_product_allowlists() {
    assert!(publisher_subject_matches(
        TrustedPublisher::OpenAi,
        "OpenAI OpCo, LLC"
    ));
    assert!(publisher_subject_matches(
        TrustedPublisher::Tencent,
        "Tencent Technology (Shenzhen) Company Limited"
    ));
    assert!(!publisher_subject_matches(
        TrustedPublisher::OpenAi,
        "OpenAI OpCo, LLC Malware"
    ));
    assert!(!publisher_subject_matches(
        TrustedPublisher::Tencent,
        "Example Software LLC"
    ));
}

#[test]
/// 验证换皮迁移中的 `gui_process_identity_keeps_pid_and_verified_path_together` 回归场景。
fn gui_process_identity_keeps_pid_and_verified_path_together() {
    let process = GuiProcess {
        pid: 42,
        path: PathBuf::from(r"C:\Program Files\ChatGPT\ChatGPT.exe"),
    };
    assert_eq!(process.pid, 42);
    assert_eq!(
        process.path,
        PathBuf::from(r"C:\Program Files\ChatGPT\ChatGPT.exe")
    );
}

#[test]
/// 验证 Windows 候选路径混用分隔符时仍能识别同一个官方可执行文件。
fn mixed_windows_separators_identify_same_verified_executable() {
    assert!(super::paths_equal_ignore_ascii_case(
        Path::new(r"C:\Users\test\AppData\Local\Programs\WorkBuddy\WorkBuddy.exe"),
        Path::new(r"c:\users\test\appdata\local\Programs/WorkBuddy/WorkBuddy.exe"),
    ));
    assert!(!super::paths_equal_ignore_ascii_case(
        Path::new(r"C:\Users\test\AppData\Local\Programs\WorkBuddy\WorkBuddy.exe"),
        Path::new(r"C:\Temp\WorkBuddy.exe"),
    ));
}

#[test]
/// Windows 安装发现与进程命令行读取不得回退到 PATH、环境根或 WMI。
fn host_discovery_and_command_line_query_have_no_environment_or_wmi_fallback() {
    let identity_source = include_str!("install_identity.rs");
    let query_source = include_str!("process_query.rs");
    assert!(!identity_source.contains("std::env"));
    assert!(!identity_source.contains("var_os"));
    assert!(!identity_source.contains("split_paths"));
    assert!(!query_source.contains("WMIConnection"));
    assert!(!query_source.contains("spawn_blocking"));
    assert!(identity_source.contains("WinVerifyTrust"));
    assert!(identity_source.contains("GetPackageFamilyName"));
    assert!(identity_source.contains("GetPackageFullName"));
    assert!(identity_source.contains("szOID_ORGANIZATION_NAME"));
    assert!(include_str!("workbuddy.rs").contains("TrustedPublisher::Tencent"));
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
/// 端口 owner 只有沿同一可信快照父链归属预期根时才可接受。
fn codex_listener_owner_must_belong_to_expected_verified_root() {
    let verified = std::collections::HashSet::from([100, 101, 200]);
    let parents = std::collections::HashMap::from([(100, 10), (101, 100), (200, 20)]);
    assert!(process_belongs_to_verified_root(
        101, 100, &verified, &parents
    ));
    assert!(!process_belongs_to_verified_root(
        200, 100, &verified, &parents
    ));
    assert!(!process_belongs_to_verified_root(
        999, 100, &verified, &parents
    ));
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

#[test]
/// 只有进程句柄进入 signaled 状态才允许继续启动替代实例。
fn process_wait_requires_observed_signaled_terminal_state() {
    assert_eq!(
        classify_process_wait(WAIT_OBJECT_0),
        ProcessWaitState::Exited
    );
    assert_eq!(
        classify_process_wait(WAIT_TIMEOUT),
        ProcessWaitState::Running
    );
    assert_eq!(classify_process_wait(WAIT_FAILED), ProcessWaitState::Failed);
}

#[test]
/// Store 激活不得回退到 runtime 无法拥有的 Tokio blocking pool。
fn store_activation_has_explicit_native_thread_owner() {
    let source = include_str!("store_activation.rs");
    assert!(!source.contains("spawn_blocking"));
    assert!(source.contains("StoreActivationOwner"));
    assert!(source.contains("CoCancelCall"));
}
