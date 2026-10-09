//! 原生应用菜单拥有进程内退出动作，不注册任何系统全局热键。

use gpui_kit::{App, KeyBinding, Menu, MenuItem, actions};

actions!(loki_metris, [Quit, ToggleNotification]);

/// 标准退出动作进入应用已有的有界保存与扫描收尾钩子。
pub(super) fn install(locale: &str, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &ToggleNotification,cx| {let state=super::notification_state(cx);if !state.busy {super::set_notification(!state.enabled,cx);}});
    cx.bind_keys([KeyBinding::new(
        if cfg!(target_os = "macos") {
            "cmd-q"
        } else {
            "ctrl-q"
        },
        Quit,
        None,
    )]);
    set_language(locale, cx);
}

/// 菜单和界面使用同一有效语言，退出行为与标题无关。
pub(super) fn set_language(locale: &str, cx: &mut App) {
    let chinese = locale == "zh-CN";
    let state=super::notification_state(cx);
    let mut menus = Vec::new();
    menus.push(
        Menu::new(if chinese {
            "诡秘神谕"
        } else {
            "LokiMetris"
        })
        .items([MenuItem::action(super::notifications::menu_label(locale,&state),ToggleNotification).checked(state.enabled).disabled(state.busy),MenuItem::Separator,MenuItem::action(
            if chinese {
                "退出诡秘神谕"
            } else {
                "Quit LokiMetris"
            },
            Quit,
        )]),
    );
    // GPUI以精确的Window名称注册AppKit系统窗口菜单，系统维护真实窗口列表。
    #[cfg(target_os = "macos")]
    menus.push(Menu::new("Window"));
    cx.set_menus(menus);
}

/// 状态变化只重建内存菜单，不读取系统权限或写偏好。
pub(super) fn refresh(cx:&mut App) {let locale=rust_i18n::locale();set_language(&locale,cx);}
