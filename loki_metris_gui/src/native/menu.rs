//! 原生应用菜单拥有进程内退出动作，不注册任何系统全局热键。

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;

use gpui_kit::{App, Global, KeyBinding, Menu, MenuItem, actions};

actions!(loki_metris, [Quit, ToggleNotification, ToggleAutostart]);

/// 菜单记住本应用显式选择的语言，异步状态刷新不依赖翻译库的进程默认值。
struct MenuLocale(&'static str);
impl Global for MenuLocale {}

/// 标准退出动作进入应用已有的有界保存与扫描收尾钩子。
pub(super) fn install(locale: &str, cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &ToggleNotification, cx| {
        let state = super::notification_state(cx);
        if !state.busy {
            super::set_notification(!state.enabled, cx);
        }
    });
    cx.on_action(|_: &ToggleAutostart, cx| {
        let state = super::autostart_state(cx);
        if !state.busy {
            if let Some(enabled) = state.enabled {
                super::set_autostart(!enabled, cx);
            }
        }
    });
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
    cx.set_global(MenuLocale(if locale == "zh-CN" {
        "zh-CN"
    } else {
        "en-US"
    }));
    refresh(cx);
}

/// 菜单投影只消费已读回的状态；等待或未知时禁用自启，不把待写意图当作系统事实。
fn apply_menus(
    autostart: &super::autostart::State,
    notification: &super::notifications::State,
    cx: &mut App,
) {
    let locale = cx
        .try_global::<MenuLocale>()
        .map(|value| value.0)
        .unwrap_or("en-US");
    let chinese = locale == "zh-CN";
    let mut menus = Vec::new();
    menus.push(
        Menu::new(if chinese {
            "诡秘神谕"
        } else {
            "LokiMetris"
        })
        .items([
            MenuItem::action(
                rust_i18n::t!("settings.autostart", locale = locale).to_string(),
                ToggleAutostart,
            )
            .checked(autostart.enabled == Some(true))
            .disabled(autostart.busy || autostart.enabled.is_none()),
            MenuItem::action(
                super::notifications::menu_label(locale, notification),
                ToggleNotification,
            )
            .checked(notification.enabled)
            .disabled(notification.busy),
            MenuItem::Separator,
            MenuItem::action(
                if chinese {
                    "退出诡秘神谕"
                } else {
                    "Quit LokiMetris"
                },
                Quit,
            ),
        ]),
    );
    // GPUI以精确的Window名称注册AppKit系统窗口菜单，系统维护真实窗口列表。
    #[cfg(target_os = "macos")]
    menus.push(Menu::new("Window"));
    cx.set_menus(menus);
}

/// 状态变化只重建内存菜单，不读取系统权限或写偏好。
pub(super) fn refresh(cx: &mut App) {
    let autostart = super::autostart_state(cx);
    let notification = super::notification_state(cx);
    apply_menus(&autostart, &notification, cx);
}
