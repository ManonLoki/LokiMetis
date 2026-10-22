//! 主线程原生托盘，提供双语窗口恢复、桌宠、通知、自启与退出入口。

use std::io;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, menu::{Menu, MenuEvent, MenuItem, CheckMenuItem}};

/// 原生对象只由 GPUI 线程持有，退出在更新栈外释放。
pub struct Tray { icon: TrayIcon, show: MenuItem, quit: MenuItem, pet:MenuItem, pet_visible:bool, pet_settings:MenuItem, mouse:CheckMenuItem, mouse_state:crate::pet::MousePassthroughState, notification:CheckMenuItem, notification_state:super::notifications::State, autostart:CheckMenuItem, autostart_state:super::autostart::State, locale:String,
    #[cfg(target_os = "linux")]
    restore: super::tray_linux::RestoreEntry,
}

/// 托盘事件只投递明确宿主意图，业务状态与持久化由各自 owner 处理。
pub enum Event {
    /// 显示主窗。
    Show,
    /// 退出应用。
    Quit,
    /// 切换桌宠可见。
    TogglePet,
    /// 打开桌宠设置。
    OpenPetSettings,
    /// 切换桌宠鼠标穿透。
    ToggleMousePassthrough,
    /// 切换系统通知。
    ToggleNotification,
    /// 切换登录自启。
    ToggleAutostart,
}

impl Tray {
    /// Windows 对象创建成功不保证 Explorer 已登记图标；关窗时查询真实恢复入口。
    pub fn restore_entry_available(&self) -> bool {
        #[cfg(windows)]
        { self.icon.rect().is_some() }
        #[cfg(target_os = "linux")]
        { self.restore.available() }
        #[cfg(not(any(windows, target_os = "linux")))]
        { true }
    }
    /// 必须在运行中的原生 UI loop 创建，图标来自用户选择的应用 Logo。
    pub fn new(locale: &str, executor: gpui_kit::BackgroundExecutor) -> io::Result<Self> {
        #[cfg(not(target_os = "linux"))]
        let _ = executor;
        let (show, quit, name) = labels(locale);
        let show = MenuItem::with_id("show_window", show, true, None);
        let quit = MenuItem::with_id("quit", quit, true, None);
        let pet=MenuItem::with_id("toggle_pet_overlay",&rust_i18n::t!("tray.hide_pet_overlay",locale=locale),true,None);
        let notification=CheckMenuItem::with_id("toggle_notification",&rust_i18n::t!("settings.system_notification",locale=locale),true,false,None);
        let autostart=CheckMenuItem::with_id("toggle_autostart",&rust_i18n::t!("settings.autostart",locale=locale),false,false,None);
        let pet_settings=MenuItem::with_id("open_pet_settings",&rust_i18n::t!("monitor.pet.settingsTitle",locale=locale),true,None);
        let mouse=CheckMenuItem::with_id("toggle_pet_mouse_passthrough",&rust_i18n::t!("monitor.pet.mousePassthrough",locale=locale),false,false,None);
        let menu = Menu::new();
        menu.append_items(&[&show, &pet, &pet_settings, &mouse, &autostart, &notification, &quit]).map_err(io::Error::other)?;
        let pixels = include_bytes!("../../assets/tray.rgba").to_vec();
        let icon = Icon::from_rgba(pixels, 32, 32).map_err(io::Error::other)?;
        let icon = TrayIconBuilder::new().with_id("main_tray").with_menu(Box::new(menu))
            .with_menu_on_left_click(false).with_tooltip(name).with_icon(icon).build().map_err(io::Error::other)?;
        Ok(Self { icon, show, quit, pet, pet_visible:true,pet_settings,mouse,mouse_state:Default::default(),notification,notification_state:Default::default(),autostart,autostart_state:Default::default(),locale:locale.into(),
            #[cfg(target_os = "linux")]
            restore: super::tray_linux::RestoreEntry::new(executor),
        })
    }

    /// 当前 UI locale 是唯一语言来源，未知值回退英文。
    pub fn set_language(&mut self, locale: &str) {
        self.locale=locale.into();
        self.set_pet_visibility(self.pet_visible);
        self.set_mouse_passthrough(self.mouse_state);
        self.pet_settings.set_text(&rust_i18n::t!("monitor.pet.settingsTitle",locale=locale));
        self.set_notification(&self.notification_state.clone());
        self.set_autostart(&self.autostart_state.clone());
        let (show, quit, name) = labels(locale);
        self.show.set_text(show);
        self.quit.set_text(quit);
        let _ = self.icon.set_tooltip(Some(name));
    }

    /// 同一菜单项随真实会话显隐切换，语言来自当前应用。
    pub fn set_pet_visibility(&mut self,visible:bool) {self.pet_visible=visible;self.pet.set_text(&rust_i18n::t!(if visible {"tray.hide_pet_overlay"} else {"tray.show_pet_overlay"},locale=&self.locale));}

    /// 仅根据controller内存状态更新菜单，配置窗在穿透期间仍可独立打开。
    pub fn set_mouse_passthrough(&mut self,state:crate::pet::MousePassthroughState) {
        self.mouse.set_checked(state.confirmed == Some(true));
        self.mouse.set_enabled(state.available && !state.busy);
        self.mouse.set_text(&rust_i18n::t!(if state.available && state.confirmed.is_none() {"monitor.pet.mousePassthroughRecover"} else {"monitor.pet.mousePassthrough"},locale=&self.locale));
        self.pet_settings.set_enabled(state.available);
        self.mouse_state=state;
    }

    /// 勾选仅代表已确认本地偏好，忙碌和权限提示单独映射。
    pub fn set_notification(&mut self,state:&super::notifications::State) {
        self.notification.set_checked(state.enabled);self.notification.set_enabled(!state.busy);
        self.notification.set_text(super::notifications::menu_label(&self.locale,state));self.notification_state=state.clone();
    }
    /// 自启勾选只来自真实 OS 回执；忙碌禁用，未知或失败允许显式重新读取。
    pub fn set_autostart(&mut self, state: &super::autostart::State) {
        let (checked, enabled, label) = autostart_projection(&self.locale, state);
        self.autostart.set_checked(checked);
        self.autostart.set_enabled(enabled);
        self.autostart.set_text(label);
        self.autostart_state = state.clone();
    }
    /// 有界读取托盘与菜单事件，避免输入风暴阻塞 UI。
    pub fn events(&self) -> Vec<Event> {
        let mut result = Vec::new();
        for event in MenuEvent::receiver().try_iter().take(64) {
            match event.id.as_ref() { "show_window" => result.push(Event::Show), "quit" => result.push(Event::Quit), "toggle_pet_overlay" => result.push(Event::TogglePet), "open_pet_settings"=>result.push(Event::OpenPetSettings), "toggle_pet_mouse_passthrough"=>result.push(Event::ToggleMousePassthrough), "toggle_notification"=>result.push(Event::ToggleNotification), "toggle_autostart"=>result.push(Event::ToggleAutostart), _ => {} }
        }
        for event in TrayIconEvent::receiver().try_iter().take(64) {
            if event.id().as_ref() == "main_tray" && matches!(event,
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }) {
                result.push(Event::Show);
            }
        }
        result
    }
}

/// 菜单投影与点击策略共享 OS 状态，未知不冒充关闭、待写意图不冒充已开启。
fn autostart_projection(locale: &str, state: &super::autostart::State) -> (bool, bool, String) {
    let key = if state.busy { "tray.autostart_busy" } else if state.error || state.enabled.is_none() { "tray.autostart_retry" } else { "settings.autostart" };
    (state.enabled == Some(true), !state.busy, rust_i18n::t!(key, locale=locale).to_string())
}

/// 标签由同一 rust-i18n 词典解析，保持系统菜单与原生页面语言一致。
fn labels(locale: &str) -> (String, String, String) {
    let locale = if locale == "zh-CN" { "zh-CN" } else { "en-US" };
    (rust_i18n::t!("tray.show_window", locale = locale).to_string(), rust_i18n::t!("tray.quit", locale = locale).to_string(),
        if locale == "zh-CN" { "诡秘神谕" } else { "LokiMetris" }.into())
}

#[cfg(test)]
mod tests {
    use super::labels;
    /// 支持语言与未知 locale 均能解析菜单，不要求测试创建真实托盘。
    #[test]
    fn tray_labels_resolve_and_unknown_locale_falls_back_to_english() {
        assert_eq!(labels("zh-CN").0, "显示窗口");
        assert_eq!(labels("en-US").1, "Quit");
        assert_eq!(labels("unknown"), labels("en-US"));
    }
    /// 已确认状态决定勾选；待写、读取失败与未知状态保留真实事实和恢复入口。
    #[test]
    fn tray_autostart_projects_confirmed_os_state_and_recovery() {
        use super::super::autostart::State;
        let mut state = State::default();
        assert_eq!(super::autostart_projection("zh-CN", &state), (false, true, "开机自启（状态不可用，点击重试）".into()));
        state.enabled = Some(false);
        state.busy = true;
        state.desired = Some(true);
        let (checked, enabled, _) = super::autostart_projection("zh-CN", &state);
        assert!(!checked && !enabled);
        state.busy = false;
        state.enabled = Some(true);
        assert_eq!(super::autostart_projection("en-US", &state), (true, true, "Start at login".into()));
        state.error = true;
        assert!(super::autostart_projection("zh-CN", &state).2.contains("重试"));
    }
}
