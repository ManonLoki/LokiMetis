//! 主线程原生托盘，提取参考项目的双语菜单和窗口恢复接线，移除全部产品菜单。

use std::io;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent, menu::{Menu, MenuEvent, MenuItem, CheckMenuItem}};

/// 原生对象只由 GPUI 线程持有，退出在更新栈外释放。
pub struct Tray { icon: TrayIcon, show: MenuItem, quit: MenuItem, pet:MenuItem, pet_visible:bool, notification:CheckMenuItem, notification_state:super::notifications::State, locale:String }

/// 托盘输入只拥有恢复与退出两种宿主语义，不承载业务动作。
pub enum Event { Show, Quit, TogglePet, ToggleNotification }

impl Tray {
    /// 必须在运行中的原生 UI loop 创建，图标来自用户选择的应用 Logo。
    pub fn new(locale: &str) -> io::Result<Self> {
        let (show, quit, name) = labels(locale);
        let show = MenuItem::with_id("show_window", show, true, None);
        let quit = MenuItem::with_id("quit", quit, true, None);
        let pet=MenuItem::with_id("toggle_pet_overlay",rust_i18n::t!("tray.hide_pet_overlay",locale=locale).to_string(),true,None);
        let notification=CheckMenuItem::with_id("toggle_notification",rust_i18n::t!("settings.system_notification",locale=locale).to_string(),true,false,None);
        let menu = Menu::new();
        menu.append_items(&[&show, &pet, &notification, &quit]).map_err(io::Error::other)?;
        let pixels = include_bytes!("../../assets/tray.rgba").to_vec();
        let icon = Icon::from_rgba(pixels, 32, 32).map_err(io::Error::other)?;
        let icon = TrayIconBuilder::new().with_id("main_tray").with_menu(Box::new(menu))
            .with_menu_on_left_click(false).with_tooltip(name).with_icon(icon).build().map_err(io::Error::other)?;
        Ok(Self { icon, show, quit, pet, pet_visible:true,notification,notification_state:Default::default(),locale:locale.into() })
    }

    /// 当前 UI locale 是唯一语言来源，未知值回退英文。
    pub fn set_language(&mut self, locale: &str) {
        self.locale=locale.into();
        self.set_pet_visibility(self.pet_visible);
        self.set_notification(&self.notification_state.clone());
        let (show, quit, name) = labels(locale);
        self.show.set_text(show);
        self.quit.set_text(quit);
        let _ = self.icon.set_tooltip(Some(name));
    }

    /// 同一菜单项随真实会话显隐切换，语言来自当前应用。
    pub fn set_pet_visibility(&mut self,visible:bool) {self.pet_visible=visible;self.pet.set_text(rust_i18n::t!(if visible {"tray.hide_pet_overlay"} else {"tray.show_pet_overlay"},locale=&self.locale).to_string());}

    /// 勾选仅代表已确认本地偏好，忙碌和权限提示单独映射。
    pub fn set_notification(&mut self,state:&super::notifications::State) {
        self.notification.set_checked(state.enabled);self.notification.set_enabled(!state.busy);
        self.notification.set_text(super::notifications::menu_label(&self.locale,state));self.notification_state=state.clone();
    }
    /// 有界读取托盘与菜单事件，避免输入风暴阻塞 UI。
    pub fn events(&self) -> Vec<Event> {
        let mut result = Vec::new();
        for event in MenuEvent::receiver().try_iter().take(64) {
            match event.id.as_ref() { "show_window" => result.push(Event::Show), "quit" => result.push(Event::Quit), "toggle_pet_overlay" => result.push(Event::TogglePet), "toggle_notification"=>result.push(Event::ToggleNotification), _ => {} }
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
}
