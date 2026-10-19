//! 启动来源属于宿主机制；登录启动不激活主窗，手动启动保留恢复入口。

/// 登录项传入稳定标记，旧 macOS LaunchAgent 只按自身精确服务身份兼容。
pub(crate) const AUTOSTART_ARGUMENT: &str = "--autostart";

/// 只识别完整参数或自身 launchd job，不把已开启自启、父进程或其它服务当成登录启动。
pub(crate) fn login_launch(arguments: impl IntoIterator<Item = std::ffi::OsString>, service: Option<&std::ffi::OsStr>) -> bool {
    arguments.into_iter().any(|argument| argument == AUTOSTART_ARGUMENT)
        || service.is_some_and(|service| service == "com.loki-metris.app")
}

/// 三平台只按本次启动标记隐藏；旧 launchd 身份兼容仅在 macOS 读取。
pub(crate) fn starts_hidden() -> bool {
    #[cfg(target_os = "macos")]
    let service = std::env::var_os("XPC_SERVICE_NAME");
    #[cfg(not(target_os = "macos"))]
    let service: Option<std::ffi::OsString> = None;
    login_launch(std::env::args_os().skip(1), service.as_deref())
}

/// 隐藏创建后最多等三秒确认托盘；超时显示，期间手动显示或服务退出会撤销旧意图。
pub(crate) fn complete(hidden: bool, cx: &mut gpui_kit::App, show: impl FnOnce(&mut gpui_kit::App) + 'static) {
    let can_wait = cx.try_global::<super::Services>().is_some_and(|service| {
        #[cfg(target_os = "linux")]
        if !service.main_is_x11 { return false; }
        tray_installed(service)
    });
    if !hidden || !can_wait { show(cx); return; }
    let generation = super::next_window_action(cx);
    let executor = cx.background_executor().clone();
    let deadline = executor.now() + std::time::Duration::from_secs(3);
    cx.spawn(async move |cx| {
        // 真实恢复入口来自已有后台探测，不在事件线程查询 D-Bus。
        loop {
            let decision = cx.update(|cx| {
                if !cx.has_global::<super::Services>() || !super::window_action_current(generation, cx) { return Some(false); }
                if super::tray_restore_available(cx) {
                    #[cfg(target_os = "linux")]
                    { cx.global_mut::<super::Services>().main_hidden = true; }
                    tracing::info!(event = "application_started_hidden");
                    return Some(false);
                }
                (executor.now() >= deadline || !tray_installed(cx.global::<super::Services>())).then_some(true)
            });
            if let Some(should_show) = decision {
                if should_show { cx.update(show); }
                return;
            }
            executor.timer(std::time::Duration::from_millis(40).min(deadline.saturating_duration_since(executor.now()))).await;
        }
    }).detach();
}

/// 测试仅替换托盘存在性及恢复事实，生产仍由实际托盘对象拥有生命周期。
fn tray_installed(service: &super::Services) -> bool {
    #[cfg(all(target_os = "linux", test))]
    if service.tray_restore_available_for_test.is_some() { return true; }
    service.tray.is_some()
}

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
}
