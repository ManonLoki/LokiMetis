//! 启动来源属于宿主机制；登录启动不激活主窗，手动启动保留恢复入口。

/// 登录项传入稳定标记，旧 macOS LaunchAgent 只按自身精确服务身份兼容。
pub(crate) const AUTOSTART_ARGUMENT: &str = "--autostart";

/// 只识别完整参数或自身 launchd job，不把已开启自启、父进程或其它服务当成登录启动。
pub(crate) fn login_launch(arguments: impl IntoIterator<Item = std::ffi::OsString>, service: Option<&std::ffi::OsStr>) -> bool {
    arguments.into_iter().any(|argument| argument == AUTOSTART_ARGUMENT)
        || service.is_some_and(|service| service == "com.loki-metris.app")
}

/// macOS 登录启动保持后台，其它平台继续使用既有前台启动合同。
pub(crate) fn starts_hidden() -> bool {
    #[cfg(target_os = "macos")]
    { login_launch(std::env::args_os().skip(1), std::env::var_os("XPC_SERVICE_NAME").as_deref()) }
    #[cfg(not(target_os = "macos"))]
    { false }
}

/// 恢复入口确实存在才允许后台启动，托盘失败时必须显示主窗。
pub(crate) fn show_main(hidden: bool, tray_available: bool) -> bool { !hidden || !tray_available }

#[cfg(test)]
mod tests {
    use super::*;
    /// 登录标记和旧自身服务身份均隐藏；手动参数、其它服务与近似值不误判。
    #[test]
    fn startup_distinguishes_manual_login_and_legacy_launch_agent() {
        let args = |values: &[&str]| values.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        assert!(!login_launch(args(&[]), None));
        assert!(login_launch(args(&[AUTOSTART_ARGUMENT]), None));
        assert!(login_launch(args(&[]), Some(std::ffi::OsStr::new("com.loki-metris.app"))));
        for value in ["--autostart=false", "--other", "com.loki-metris.app.extra"] {
            assert!(!login_launch(args(&[value]), Some(std::ffi::OsStr::new(value))));
        }
    }
    /// 后台启动没有可恢复托盘时回退前台，手动打开始终显示。
    #[test]
    fn startup_requires_a_real_restore_entry_before_hiding() {
        assert!(!show_main(true, true));
        assert!(show_main(true, false));
        assert!(show_main(false, true));
        assert!(show_main(false, false));
    }
}
