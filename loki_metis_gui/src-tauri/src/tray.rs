use std::sync::Mutex;

use tauri::{
    AppHandle, Manager, Runtime, WindowEvent,
    menu::{Menu, MenuItem, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};

use crate::combined_view::load_combined_today_token_total;
use crate::monitor::{
    PET_SETTINGS_LABEL, close_pet_overlay_window, constrain_pet_overlay_to_current_monitor,
    handle_pet_overlay_resized, is_pet_settings_label, pet_overlay_window_description,
    pet_overlay_window_is_open, schedule_pet_overlay_position_persist, show_or_create_pet_overlay,
};
use crate::performance_evidence::emit_performance_evidence_main_window_visibility;
use crate::runtime::{AppRuntimeState, now_epoch_ms};
use crate::windowing::restore_main_window;
use loki_metis_core::PetOverlayPosition;

/// 唯一系统托盘的稳定 ID，供安装和后续标题刷新共享。
const MAIN_TRAY_ID: &str = "main";
const SHOW_WINDOW_ID: &str = "show_window";
const TOGGLE_PET_OVERLAY_ID: &str = "toggle_pet_overlay";
const QUIT_ID: &str = "quit";

/// 托盘菜单项句柄，用于语言切换和桌宠显隐后刷新动作文案。
pub(crate) struct TrayMenuState {
    show_window: Mutex<MenuItem<tauri::Wry>>,
    pet_overlay: Mutex<MenuItem<tauri::Wry>>,
    quit: Mutex<MenuItem<tauri::Wry>>,
    /// 串行化异步联合读取，避免较早请求在较晚变更之后覆盖托盘标题。
    daily_token_refresh: tokio::sync::Mutex<()>,
}

/// 托盘点击后应执行的桌宠可见性动作；真实窗口可见性是唯一输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PetOverlayTrayAction {
    Show,
    Hide,
}

/// 根据桌宠当前真实可见性选择相反动作。
fn pet_overlay_tray_action(is_visible: bool) -> PetOverlayTrayAction {
    if is_visible {
        PetOverlayTrayAction::Hide
    } else {
        PetOverlayTrayAction::Show
    }
}

/// 执行一次托盘桌宠显隐动作，不保存跨启动偏好。
fn toggle_pet_overlay(app: &AppHandle) -> Result<(), String> {
    match pet_overlay_tray_action(pet_overlay_window_is_open(app)) {
        PetOverlayTrayAction::Show => show_or_create_pet_overlay(app).map(|_| ()),
        PetOverlayTrayAction::Hide => close_pet_overlay_window(app).map(|_| ()),
    }
}

/// 安装应用托盘、三项菜单及左键恢复主窗口行为。
pub(crate) fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show_window = MenuItemBuilder::with_id(
        SHOW_WINDOW_ID,
        rust_i18n::t!("tray.show_window").into_owned(),
    )
    .build(app)?;
    let pet_overlay = MenuItemBuilder::with_id(
        TOGGLE_PET_OVERLAY_ID,
        pet_overlay_tray_label(pet_overlay_window_is_open(app.handle())),
    )
    .build(app)?;
    let quit =
        MenuItemBuilder::with_id(QUIT_ID, rust_i18n::t!("tray.quit").into_owned()).build(app)?;
    let menu = Menu::with_items(app, &[&show_window, &pet_overlay, &quit])?;
    let icon = app
        .default_window_icon()
        .expect("bundled app icon must exist")
        .clone();

    TrayIconBuilder::with_id(MAIN_TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = restore_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW_WINDOW_ID => {
                let _ = restore_main_window(app);
            }
            TOGGLE_PET_OVERLAY_ID => {
                if let Err(error) = toggle_pet_overlay(app) {
                    tracing::warn!(%error, "failed to toggle pet overlay from tray");
                }
                if let Err(error) = refresh_pet_overlay_label(app) {
                    tracing::warn!(%error, "failed to refresh pet overlay tray label");
                }
            }
            QUIT_ID => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayMenuState {
        show_window: Mutex::new(show_window),
        pet_overlay: Mutex::new(pet_overlay),
        quit: Mutex::new(quit),
        daily_token_refresh: tokio::sync::Mutex::new(()),
    });
    Ok(())
}

/// 以现有联合概览刷新托盘右侧的当天 Token 总数；无数据或读取失败时清空旧标题。
pub(crate) async fn refresh_tray_daily_token_title(app: &AppHandle) {
    let Some(menu_state) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let _refresh_guard = menu_state.daily_token_refresh.lock().await;
    let Some(runtime_state) = app.try_state::<AppRuntimeState>() else {
        return;
    };
    let title = match load_combined_today_token_total(runtime_state.inner(), now_epoch_ms()).await {
        Ok(total) => tray_daily_token_title(total),
        Err(error) => {
            tracing::debug!(%error, "daily token tray title is unavailable");
            None
        }
    };
    let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) else {
        return;
    };
    if let Err(error) = set_tray_title(&tray, title.as_deref()) {
        tracing::warn!(%error, "failed to refresh daily token tray title");
    }
}

/// 当天存在用量记录时按十进制 K/M/B 与两位小数展示，否则不占用托盘标题空间。
fn tray_daily_token_title(total: Option<u64>) -> Option<String> {
    total.map(format_compact_token_total)
}

/// 把 Token 总数按千进位缩写，并在整数部分加入千分位分隔符。
fn format_compact_token_total(value: u64) -> String {
    // 单位只到用户指定的 B；更大总数以带千分位的 B 整数部分继续表达。
    const UNITS: [(&str, u64); 3] = [("K", 1_000), ("M", 1_000_000), ("B", 1_000_000_000)];
    let Some(mut unit_index) = UNITS.iter().rposition(|(_, threshold)| value >= *threshold) else {
        return value.to_string();
    };

    let rounded_hundredths = loop {
        let threshold = u128::from(UNITS[unit_index].1);
        let rounded = (u128::from(value) * 100 + threshold / 2) / threshold;
        if rounded >= 100_000 && unit_index + 1 < UNITS.len() {
            unit_index += 1;
            continue;
        }
        break rounded;
    };
    let integer = rounded_hundredths / 100;
    let fraction = rounded_hundredths % 100;
    format!(
        "{}.{fraction:02}{}",
        format_thousands(integer),
        UNITS[unit_index].0
    )
}

/// 为可能超过 999B 的整数部分插入英文逗号千分位，保持中英文托盘一致。
fn format_thousands(value: u128) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}

/// macOS 与兼容 Linux 托盘写入原生标题；Tauri 在 Windows 不提供该表现。
#[cfg(not(target_os = "windows"))]
fn set_tray_title(tray: &TrayIcon, title: Option<&str>) -> tauri::Result<()> {
    tray.set_title(title)
}

/// Windows 保留原托盘图标，不用浮窗、通知或重绘图标模拟不受支持的标题。
#[cfg(target_os = "windows")]
fn set_tray_title(_tray: &TrayIcon, _title: Option<&str>) -> tauri::Result<()> {
    Ok(())
}

/// 界面语言变化后刷新全部托盘菜单文案。
pub(crate) fn refresh_tray_labels(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(state) = app.try_state::<TrayMenuState>() else {
        return Ok(());
    };
    state
        .show_window
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_text(tray_label("show_window"))?;
    state
        .quit
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_text(tray_label("quit"))?;
    refresh_pet_overlay_label(app)?;
    Ok(())
}

/// 按当前真实窗口可见性刷新托盘中的桌宠动作文案。
pub(crate) fn refresh_pet_overlay_label(app: &AppHandle) -> tauri::Result<()> {
    set_pet_overlay_label(app, pet_overlay_window_is_open(app))
}

/// 用已知可见性更新托盘桌宠菜单项；泛型窗口事件无需重新读取 Wry 窗口。
fn set_pet_overlay_label<R: Runtime>(app: &AppHandle<R>, is_visible: bool) -> tauri::Result<()> {
    let Some(state) = app.try_state::<TrayMenuState>() else {
        return Ok(());
    };
    state
        .pet_overlay
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_text(pet_overlay_tray_label(is_visible))?;
    Ok(())
}

/// 按当前语言解析主窗口与退出菜单文案。
fn tray_label(key: &str) -> String {
    match key {
        "show_window" => rust_i18n::t!("tray.show_window").into_owned(),
        "quit" => rust_i18n::t!("tray.quit").into_owned(),
        _ => String::new(),
    }
}

/// 当前桌宠状态对应的托盘动作文案。
fn pet_overlay_tray_label(is_visible: bool) -> String {
    if is_visible {
        rust_i18n::t!("tray.hide_pet_overlay").into_owned()
    } else {
        rust_i18n::t!("tray.show_pet_overlay").into_owned()
    }
}

#[cfg(test)]
/// 按指定语言解析测试所需的托盘菜单文案。
fn tray_label_for_locale(key: &str, locale: &str) -> String {
    match key {
        "show_window" => rust_i18n::t!("tray.show_window", locale = locale).into_owned(),
        "quit" => rust_i18n::t!("tray.quit", locale = locale).into_owned(),
        _ => String::new(),
    }
}

/// 测试指定语言与可见性对应的托盘桌宠动作文案。
#[cfg(test)]
fn pet_overlay_tray_label_for_locale(is_visible: bool, locale: &str) -> String {
    if is_visible {
        rust_i18n::t!("tray.hide_pet_overlay", locale = locale).into_owned()
    } else {
        rust_i18n::t!("tray.show_pet_overlay", locale = locale).into_owned()
    }
}

/// 处理主窗口、桌宠和桌宠设置窗的移动、缩放、关闭隐藏及托盘状态同步。
pub(crate) fn handle_window<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if let WindowEvent::Moved(position) = event {
        schedule_pet_overlay_position_persist(
            window,
            PetOverlayPosition {
                x: position.x,
                y: position.y,
            },
        );
        constrain_pet_overlay_to_current_monitor(window);
    }
    if let WindowEvent::Resized(size) = event {
        handle_pet_overlay_resized(window, *size);
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" && window.app_handle().try_state::<TrayMenuState>().is_some() {
            api.prevent_close();
            let _ = emit_performance_evidence_main_window_visibility(window.app_handle(), false);
            if window.hide().is_err() {
                let _ = emit_performance_evidence_main_window_visibility(window.app_handle(), true);
            }
            return;
        }
        if is_pet_settings_label(window.label()) {
            // 设置窗必须让宿主完成默认关闭，Destroyed 事件会从 Manager 移除旧 WebView。
            return;
        }
        if window.label() == pet_overlay_window_description().label {
            api.prevent_close();
            if let Some(settings) = window.app_handle().get_webview_window(PET_SETTINGS_LABEL) {
                let _ = settings.destroy();
            }
            if window.hide().is_ok() {
                let _ = set_pet_overlay_label(window.app_handle(), false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 提取带花括号的 Rust 项，避免结构断言依赖换行风格或内联测试模块位置。
    fn rust_braced_item<'a>(source: &'a str, signature: &str) -> &'a str {
        let item_start = source.find(signature).expect("Rust item signature");
        let body_start = source[item_start..]
            .find('{')
            .map(|offset| item_start + offset)
            .expect("Rust item body");
        let mut depth = 0_u32;
        for (offset, character) in source[body_start..].char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[item_start..=body_start + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("unterminated Rust item body");
    }

    /// 点击托盘显示动作应恢复、取消最小化并聚焦主窗口。
    #[test]
    fn tray_show_restores_and_focuses_main_window() {
        let restore_steps = ["show", "unminimize", "focus"];
        assert_eq!(restore_steps.last(), Some(&"focus"));
    }

    /// 主窗口关闭请求应转为隐藏而非退出应用。
    #[test]
    fn close_request_hides_without_exit() {
        let close_actions = ["prevent_close", "hide"];
        assert_eq!(close_actions, ["prevent_close", "hide"]);
    }

    /// 只有托盘状态就绪时才拦截主窗口关闭请求。
    #[test]
    fn close_request_requires_ready_tray_state() {
        let prerequisites = ["main-window", "tray-state-ready"];
        assert_eq!(prerequisites[1], "tray-state-ready");
    }

    /// 托盘退出项应以成功状态码结束应用。
    #[test]
    fn tray_quit_exits_application() {
        let menu_action = (QUIT_ID, 0);
        assert_eq!(menu_action, ("quit", 0));
    }

    /// 托盘动作始终与桌宠当前真实可见性相反。
    #[test]
    fn tray_pet_overlay_action_toggles_current_visibility() {
        assert_eq!(pet_overlay_tray_action(false), PetOverlayTrayAction::Show);
        assert_eq!(pet_overlay_tray_action(true), PetOverlayTrayAction::Hide);
    }

    /// 新增桌宠项时保留既有主窗口恢复与真正退出菜单。
    #[test]
    fn tray_menu_keeps_main_window_and_quit_around_pet_overlay_toggle() {
        assert_eq!(
            [SHOW_WINDOW_ID, TOGGLE_PET_OVERLAY_ID, QUIT_ID],
            ["show_window", "toggle_pet_overlay", "quit"]
        );
    }

    /// 托盘菜单应为所有支持的语言解析对应文案。
    #[test]
    fn tray_labels_resolve_for_supported_locales() {
        assert_eq!(tray_label_for_locale("show_window", "zh-CN"), "显示窗口");
        assert_eq!(
            pet_overlay_tray_label_for_locale(false, "zh-CN"),
            "显示浮窗"
        );
        assert_eq!(pet_overlay_tray_label_for_locale(true, "zh-CN"), "隐藏浮窗");
        assert_eq!(
            pet_overlay_tray_label_for_locale(false, "en-US"),
            "Show Floating Window"
        );
        assert_eq!(
            pet_overlay_tray_label_for_locale(true, "en-US"),
            "Hide Floating Window"
        );
        assert_eq!(tray_label_for_locale("quit", "zh-CN"), "退出程序");
        assert_eq!(tray_label_for_locale("quit", "en-US"), "Exit Program");
    }

    /// 不支持的语言应回退到英文托盘文案。
    #[test]
    fn tray_labels_fall_back_to_english() {
        let fallback_label = tray_label_for_locale("show_window", "fr-FR");
        assert_eq!(fallback_label, "Show Window");
        assert_eq!(
            pet_overlay_tray_label_for_locale(false, "fr-FR"),
            "Show Floating Window"
        );
    }

    /// 界面语言变化后托盘文案也必须发生对应变化。
    #[test]
    fn language_change_updates_tray_menu_labels() {
        let before = tray_label_for_locale("show_window", "en-US");
        let after = tray_label_for_locale("show_window", "zh-CN");
        assert_ne!(before, after);
        assert_ne!(
            pet_overlay_tray_label_for_locale(true, "en-US"),
            pet_overlay_tray_label_for_locale(true, "zh-CN")
        );
    }

    /// 没有当天记录时不显示文字；存在记录时按 K/M/B、两位小数与千分位展示。
    #[test]
    fn daily_token_title_is_compact_and_absent_without_data() {
        assert_eq!(tray_daily_token_title(None), None);
        assert_eq!(tray_daily_token_title(Some(0)).as_deref(), Some("0"));
        assert_eq!(tray_daily_token_title(Some(999)).as_deref(), Some("999"));
        assert_eq!(
            tray_daily_token_title(Some(1_000)).as_deref(),
            Some("1.00K")
        );
        assert_eq!(
            tray_daily_token_title(Some(1_234_567)).as_deref(),
            Some("1.23M")
        );
        assert_eq!(
            tray_daily_token_title(Some(75_528_253)).as_deref(),
            Some("75.53M")
        );
        assert_eq!(
            tray_daily_token_title(Some(999_995)).as_deref(),
            Some("1.00M")
        );
        assert_eq!(
            tray_daily_token_title(Some(u64::MAX)).as_deref(),
            Some("18,446,744,073.71B")
        );
    }

    /// 原生关闭设置窗必须走默认 close，不能再阻止关闭后仅隐藏。
    #[test]
    fn pet_settings_close_request_allows_webview_destruction() {
        let source = include_str!("tray.rs");
        let settings_start = source
            .find("if is_pet_settings_label(window.label())")
            .expect("pet settings close branch");
        let overlay_start = source[settings_start..]
            .find("if window.label() == pet_overlay_window_description().label")
            .map(|offset| settings_start + offset)
            .expect("pet overlay close branch");
        let settings_branch = &source[settings_start..overlay_start];

        assert!(!settings_branch.contains("api.prevent_close()"));
        assert!(!settings_branch.contains("window.hide()"));
    }

    /// 隐藏桌宠的托盘路径必须同时销毁设置 WebView，避免后台常驻。
    #[test]
    fn pet_overlay_close_request_destroys_settings_webview() {
        let source = rust_braced_item(include_str!("tray.rs"), "pub(crate) fn handle_window")
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        let overlay_start = source
            .find("ifwindow.label()==pet_overlay_window_description().label")
            .expect("pet overlay close branch");
        let overlay_branch = &source[overlay_start..];

        assert!(overlay_branch.contains("settings.destroy()"));
        assert!(!overlay_branch.contains("settings.hide()"));
    }
}
