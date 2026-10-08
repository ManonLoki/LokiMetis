//! 已选通知开关通过真实渲染树点击验证，测试平台不发送系统通知。

use super::tests::open_shell;
use gpui_kit::{AppContext, TestAppContext, test::TestWindowExt};

/// 点击开关只改应用偏好，不从周围卡片代理动作，也不自动投递通知。
#[gpui_kit::test]
fn notification_switch_owns_click_without_automatic_submission(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!shell.read(cx).preferences.system_notification);
        window.click("system-notification-switch", cx);
        assert!(shell.read(cx).preferences.system_notification);
        window.render_frame(cx);
        window.click("main-content", cx);
        assert!(shell.read(cx).preferences.system_notification);
        window.click("system-notification-switch", cx);
        assert!(!shell.read(cx).preferences.system_notification);
    }).expect("更新原生设置测试窗口");
    assert!(cx.shown_system_notifications().is_empty());
}
