//! 原生壳层只拥有导航、外观和设备偏好；领域状态始终从直接依赖的共享核心读取。

mod settings;

mod about;


use crate::{assets, preferences::{LanguageChoice, Preferences, ThemeChoice, Writer}};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Render, Subscription, Window, WindowAppearance, assets::IconName, base::TestSupportExt as _, component::{ActiveTheme, Theme, ThemeMode, button::{Button, ButtonVariants}}, div, img, prelude::*, px};

/// 侧栏内边距同时确定顶部 Logo 与边界控件的垂直原点。
const SIDEBAR_INSET: f32 = 6.;
/// 保留详细侧栏原有 pt_7 的 1.75rem 顶部留白，按钮按窗口真实 rem 换算。
const SIDEBAR_LOGO_TOP: f32 = 1.75;

/// 页面枚举只包含本次初始化实际启用的页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Settings,
    About,

}

/// 所有壳层可变视图状态由唯一 GPUI 实体拥有，不镜像业务数据。
pub struct Shell {
    page: Page,
    preferences: Preferences,
    writer: Writer,
    locale: &'static str,
    _appearance: Subscription,
    _bounds: Subscription,
    show_update_status: bool,
    show_release_notes: bool,
}

impl Shell {
    /// 建立真实系统外观观察和最后窗口关闭时的状态保存。
    pub fn new(preferences: Preferences, writer: Writer, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        let appearance_entity = weak.clone();
        let appearance = window.observe_window_appearance(move |window, cx| {
            let _ = appearance_entity.update(cx, |this, cx| {
                if this.preferences.theme == ThemeChoice::System {
                    this.apply_theme(window, cx);
                    cx.notify();
                }
            });
        });
        window.on_window_should_close(cx, move |window, cx| {
            let _ = weak.update(cx, |this, _| {
                this.remember_window(window);
            });
            !crate::native::hide_if_available(cx)
        });
        let bounds_subscription = cx.observe_window_bounds(window, |this, window, _| { this.remember_window(window); });
        let locale = preferences.locale();
        let mut value = Self { page: Page::Settings, preferences, writer, locale, _appearance: appearance, _bounds: bounds_subscription,
            show_update_status: false, show_release_notes: false,
        };
        value.remember_window(window);
        value.apply_language();
        value.apply_theme(window, cx);
        value
    }

    /// 保存内容尺寸与有界后台快照，直接退出也能恢复最新状态且不叠加原生标题栏。
    fn remember_window(&mut self, window: &Window) {
        if window.is_fullscreen() { return }
        self.preferences.maximized = window.is_maximized();
        if !self.preferences.maximized {
            self.preferences.bounds = Some(window_geometry(window.bounds(), window.viewport_size()));
        }
        self.writer.save(&self.preferences);
    }

    /// 当前 locale 决定中英文展示名称，版本始终来自实际 Cargo 包。
    fn name(&self) -> &'static str {
        if self.locale == "zh-CN" { "诡秘神谕" } else { "LokiMetris" }
    }

    /// 所有可见文案均从编译期本地词典解析。
    fn t(&self, key: &str) -> String {
        rust_i18n::t!(key, locale = self.locale).to_string()
    }

    /// 固定版本显示只允许一个小写前缀。
    fn version(&self) -> String {
        format!("v{}", env!("CARGO_PKG_VERSION").trim_start_matches(['v', 'V']))
    }

    /// 系统选择绑定真实原生外观，显式选择直接覆盖当前 Kit 主题。
    fn apply_theme(&self, window: &Window, cx: &mut Context<Self>) {
        let dark = match self.preferences.theme {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::System => matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark),
        };
        Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    }

    /// 组件库词典与应用词典使用相同的有效语言。
    fn apply_language(&self) {
        gpui_kit::component::set_locale(if self.locale == "zh-CN" { "zh-CN" } else { "en" });
    }

    /// 主题按钮只修改设备偏好，并立即应用当前窗口外观。
    fn select_theme(&mut self, theme: ThemeChoice, window: &Window, cx: &mut Context<Self>) {
        self.preferences.theme = theme;
        self.apply_theme(window, cx);
        self.writer.save(&self.preferences);
        cx.notify();
    }

    /// 语言切换同步应用和组件词典，同时保留当前页面。
    fn select_language(&mut self, language: LanguageChoice, cx: &mut Context<Self>) {
        self.preferences.language = language;
        self.locale = self.preferences.locale();
        crate::native::set_language(self.locale, cx);
        self.apply_language();
        self.writer.save(&self.preferences);
        cx.refresh_windows();
        cx.notify();
    }

    /// 页面切换不创建新的数据副本或重置设备偏好。
    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    /// 页面标题来自当前实际页面对应的词典键。
    fn page_title(&self) -> String {
        self.t(match self.page {
            Page::Settings => "navigation.settings",
            Page::About => "navigation.about",

        })
    }

    /// compact 永远显示名称；detailed 按自身持久偏好展示或收起。
    fn sidebar_width(&self) -> f32 {
        if true { 80. } else if self.preferences.collapsed { 76. } else { 248. }
    }

    /// Logo 与边界按钮共享当前详细侧栏档位，避免折叠后垂直中心漂移。
    fn sidebar_logo_size(&self) -> f32 {
        if true { 36. } else if self.preferences.collapsed { 44. } else { 72. }
    }

    /// 具有真实按钮语义的整个菜单项直接拥有点击，不借助父级代理。
    fn navigation_item(&self, page: Page, id: &'static str, key: &str, icon: IconName, cx: &mut Context<Self>) -> AnyElement {
        let label = self.t(key);
        let compact = true;
        let collapsed = !compact && self.preferences.collapsed;
        if compact {
            Button::new(id).ghost().h(px(56.)).w_full()
                .tooltip(label.clone()).accessibility_label(label.clone())
                .child(div().w_full().flex().flex_col().items_center().gap_1()
                    .child(gpui_kit::component::Icon::new(icon).size(px(22.)))
                    .child(div().w_full().text_center().text_size(px(11.)).child(label)))
                .when(self.page == page, |button| button.primary())
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
                .into_any_element()
        } else {
            Button::new(id).ghost().h(px(44.)).w_full()
                .tooltip(label.clone()).accessibility_label(label.clone())
                .child(div().id((id, 0usize)).test_support().w_full().min_w_0().flex().items_center().gap_2()
                    .when(collapsed, |row| row.justify_center())
                    .when(!collapsed, |row| row.justify_start())
                    .child(div().id((id, 1usize)).test_support().flex_shrink_0()
                        .child(gpui_kit::component::Icon::new(icon).size(px(22.))))
                    .when(!collapsed, |row| row.child(div().id((id, 2usize)).test_support().min_w_0().child(label))))
                .when(self.page == page, |button| button.primary())
                .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
                .into_any_element()
        }
    }

    /// 功能区向下增长，支持区保持已选赞助、设置、已选关于的稳定顺序。
    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let width = self.sidebar_width();
        let logo_size = self.sidebar_logo_size();
        let identity = div().id("sidebar-identity").test_support().relative().w_full().flex_shrink_0().flex().flex_col().items_center().justify_center().gap_2()
            .when(true, |header| header.py_3())
            .when(!true, |header| header.pt(gpui_kit::rems(SIDEBAR_LOGO_TOP)).pb_3())
            .child(div().id("sidebar-logo").test_support().size(px(logo_size)).flex_shrink_0().child(img(assets::LOGO).size_full()))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(self.version()))
            .when(!true && !self.preferences.collapsed, |header| header.child(div().w_full().min_w_0().whitespace_normal().text_center().text_sm().font_weight(FontWeight::SEMIBOLD).child(self.name())))
            ;
        let mut sidebar = div().id("sidebar").test_support().w(px(width)).h_full().min_h_0().flex_shrink_0().flex().flex_col()
            .p(px(SIDEBAR_INSET)).gap_2().border_r_1().border_color(cx.theme().border).bg(cx.theme().secondary)
            .child(identity);
        sidebar = sidebar.child(div().flex_1());

        sidebar = sidebar.child(self.navigation_item(Page::Settings, "nav-settings", "navigation.settings", IconName::Settings, cx));
        sidebar = sidebar.child(self.navigation_item(Page::About, "nav-about", "navigation.about", IconName::Info, cx));
        sidebar.into_any_element()
    }
}

impl Render for Shell {
    /// 原生窗口标题与视图元数据由同一身份和当前语言派生。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_window_title(&format!("{} {} QQ:2222980", self.name(), self.version()));
        let _definition = loki_metris_core::product_definition();
        let content = match self.page {
            Page::Settings => self.settings_page(cx),
            Page::About => self.about_page(cx),

        };
        let content = div().id("page-content").test_support().flex_1().min_w_0().min_h_0().overflow_y_scroll().flex().flex_col()
            .when(false, |page| page.p_0())
            .when(!(false), |page| page.p_8())
            .child(content);
        div().id("application-shell").test_support().relative().size_full().min_w_0().min_h_0().overflow_hidden().flex().bg(cx.theme().background).text_color(cx.theme().foreground)
            .child(self.sidebar(cx))
            .child(div().id("main-content").test_support().flex_1().min_w_0().h_full().flex().flex_col()
                .when(!(false), |column| column.child(div().id("page-heading").test_support().h(px(88.)).flex_shrink_0().px_8().flex().items_center().border_b_1().border_color(cx.theme().border)
                    .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).child(self.page_title()))))
                .child(content))

    }
}


/// 保留原生窗口左上角，尺寸统一采用内容视口而不是含装饰的外框。
fn window_geometry(frame: gpui_kit::Bounds<gpui_kit::Pixels>, viewport: gpui_kit::Size<gpui_kit::Pixels>) -> [f32; 4] {
    [frame.origin.x.into(), frame.origin.y.into(), viewport.width.into(), viewport.height.into()]
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
