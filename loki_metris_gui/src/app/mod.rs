//! 原生壳层只拥有导航、外观和设备偏好；领域状态始终从直接依赖的共享核心读取。

mod settings;
mod dashboard;

mod about;


use crate::{assets, preferences::{LanguageChoice, Preferences, ThemeChoice, Writer}};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Render, Subscription, Window, WindowAppearance, assets::IconName, base::TestSupportExt as _, component::{ActiveTheme, Theme, ThemeMode}, div, img, prelude::*, px};

/// 侧栏内边距同时确定顶部 Logo 与边界控件的垂直原点。
const SIDEBAR_INSET: f32 = 6.;
/// 页面枚举只包含本次初始化实际启用的页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Settings,
    About,

}

/// 所有壳层可变视图状态由唯一 GPUI 实体拥有，不镜像业务数据。
pub struct Shell {
    dashboard: dashboard::Dashboard,
    page: Page,
    viewport_width: f32,
    preferences: Preferences,
    writer: Writer,
    locale: &'static str,
    _appearance: Subscription,
    _bounds: Subscription,
    _activation: Subscription,
    _visibility: Subscription,
    window_active: bool,
    window_visible: bool,
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
        let activation = cx.observe_window_activation(window, |this,window,cx|this.update_window_visibility(window,cx));
        let visibility = cx.observe_window_visibility(window, |this,_,window,cx|this.update_window_visibility(window,cx));
        let locale = preferences.locale();
        let mut value = Self { page: Page::Dashboard, viewport_width: 1440., dashboard: dashboard::Dashboard::default(), preferences, writer, locale, _appearance: appearance, _bounds: bounds_subscription,
            _activation:activation,_visibility:visibility,window_active:window.is_window_active(),window_visible:window.is_visible(),
            show_update_status: false, show_release_notes: false,
        };
        value.remember_window(window);
        value.apply_language();
        value.apply_theme(window, cx);
        value
    }

    /// 后台或完全不可见窗口暂停页面轮询；恢复前台后由既有计时器补读。
    fn update_window_visibility(&mut self,window:&Window,cx:&mut Context<Self>) {
        self.window_active=window.is_window_active();self.window_visible=window.is_visible();cx.notify();
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
        Theme::update(cx, |theme| {
            let color = |light, dark_color| gpui_kit::rgb(if dark { dark_color } else { light }).into();
            theme.font_size = px(16.);
            theme.font_family = "Geist".into();
            theme.mono_font_family = "Geist Mono".into();
            theme.colors.background = color(0xf7f4ef, 0x131211);
            theme.colors.foreground = color(0x2d241e, 0xf8f8f8);
            theme.colors.secondary = color(0xfffdf9, 0x1e1c1a);
            theme.colors.muted = color(0xfcfaf7, 0x26272b);
            theme.colors.muted_foreground = color(0x847b72, 0x8a8784);
            theme.colors.border = color(0xe6ddd1, 0x332f2a);
            theme.colors.primary = color(0xc43b2d, 0xd95a44);
            theme.colors.primary_foreground = color(0xffffff, 0xffffff);
            theme.colors.accent = color(0xfbe2d6, 0x3c241d);
            theme.colors.accent_foreground = color(0x94291f, 0xffb18e);
            theme.colors.primary_hover = color(0xaa3026, 0xe16b57);
            theme.colors.primary_active = color(0x94291f, 0xc34b39);
            theme.colors.secondary_foreground = theme.colors.foreground;
            theme.colors.secondary_hover = theme.colors.muted;
            theme.colors.secondary_active = theme.colors.accent;
            theme.colors.button = theme.colors.secondary;
            theme.colors.button_foreground = theme.colors.foreground;
            theme.colors.button_hover = theme.colors.muted;
            theme.colors.button_active = theme.colors.accent;
            theme.colors.button_primary = theme.colors.primary;
            theme.colors.button_primary_foreground = theme.colors.primary_foreground;
            theme.colors.button_primary_hover = theme.colors.primary_hover;
            theme.colors.button_primary_active = theme.colors.primary_active;
            theme.colors.button_secondary = theme.colors.secondary;
            theme.colors.button_secondary_foreground = theme.colors.foreground;
            theme.colors.button_secondary_hover = theme.colors.secondary_hover;
            theme.colors.button_secondary_active = theme.colors.secondary_active;
            theme.colors.tab_bar = color(0xf9f7f4, 0x191a1c);
            theme.colors.tab_bar_segmented = theme.colors.secondary;
            theme.colors.tab_foreground = theme.colors.muted_foreground;
            theme.colors.tab_active = theme.colors.accent;
            theme.colors.tab_active_foreground = theme.colors.accent_foreground;
            theme.colors.table = theme.colors.secondary;
            theme.colors.table_head = theme.colors.secondary;
            theme.colors.table_head_foreground = theme.colors.foreground;
            theme.colors.table_row_border = color(0xe6ddd1, 0x424242);
        });
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
        if self.page != page {self.dashboard.sources.reset();}
        self.page = page;
        if page==Page::Dashboard {self.reload_usage(false,cx);}
        cx.notify();
    }

    /// 页面标题来自当前实际页面对应的词典键。
    fn page_title(&self) -> String {
        self.t(match self.page {
            Page::Dashboard => "navigation.dashboard",
            Page::Settings => "navigation.settings",
            Page::About => "navigation.about",

        })
    }

    /// 参考产品使用固定紧凑侧栏，整项 Button 拥有点击和辅助标签。
    fn navigation_item(&self, page: Page, id: &'static str, key: &str, icon: IconName, cx: &mut Context<Self>) -> AnyElement {
        let label = self.t(key);
        gpui_kit::base::Button::new(id).h_auto().min_h(px(56.)).py_1().px_0().w_full().flex().items_center().justify_center().rounded(px(12.))
            .accessibility_label(label.clone())
            .child(div().w_full().h_auto().flex_shrink_0().flex().flex_col().items_center().gap_1()
                .child(gpui_kit::component::Icon::new(icon).size(px(22.)))
                .child(div().w(px(67.)).whitespace_normal().text_center().text_size(px(11.)).line_height(gpui_kit::relative(1.25)).font_weight(FontWeight::NORMAL).child(label)))
            .when(self.page == page, |button| button.bg(cx.theme().accent).text_color(cx.theme().accent_foreground).rounded(px(12.)))
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
            .into_any_element()
    }

    /// 侧栏分隔线沿用参考的中性色，不占业务导航项内部间距。
    fn sidebar_divider(&self, cx: &Context<Self>) -> AnyElement {
        div().h(px(1.)).flex_shrink_0().w_full().my_2()
            .bg(if cx.theme().mode.is_dark() { gpui_kit::rgb(0x424242) } else { gpui_kit::rgb(0xdee2e6) }).into_any_element()
    }

    /// 功能区向下增长，支持区保持已选赞助、设置、已选关于的稳定顺序。
    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let width = 80.;
        let logo_size = 36.;
        let identity = div().id("sidebar-identity").test_support().relative().w_full().flex_shrink_0().flex().flex_col().items_center().justify_center().gap_2()
            .py_2()
            .child(div().id("sidebar-logo").test_support().size(px(logo_size)).flex_shrink_0().child(img(assets::LOGO).size_full()))
            .child(div().text_size(px(10.)).text_color(cx.theme().muted_foreground).child(self.version()))
            ;
        let mut sidebar = div().id("sidebar").test_support().w(px(width)).h_full().min_h_0().flex_shrink_0().flex().flex_col()
            .p(px(SIDEBAR_INSET)).border_r_1().border_color(cx.theme().border).bg(cx.theme().secondary)
            .child(identity).child(self.sidebar_divider(cx));
        sidebar = sidebar.child(self.navigation_item(Page::Dashboard, "nav-dashboard", "navigation.dashboard", IconName::LayoutDashboard, cx));
        sidebar = sidebar.child(div().flex_1());

        sidebar = sidebar.child(self.sidebar_divider(cx));
        sidebar = sidebar.child(self.navigation_item(Page::Settings, "nav-settings", "navigation.settings", IconName::Settings, cx));
        sidebar = sidebar.child(self.navigation_item(Page::About, "nav-about", "navigation.about", IconName::Info, cx));
        sidebar.into_any_element()
    }
}

impl Render for Shell {
    /// 原生窗口标题与视图元数据由同一身份和当前语言派生。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.viewport_width = window.viewport_size().width.into();
        window.set_window_title(&format!("{} {} QQ:2222980", self.name(), self.version()));
        let content = match self.page {
            Page::Dashboard => self.dashboard_page(window,cx),
            Page::Settings => self.settings_page(cx),
            Page::About => self.about_page(cx),

        };
        let content = div().id("page-content").test_support().flex_1().min_w_0().min_h_0().overflow_y_scroll().flex().flex_col()
            .p(px(if self.viewport_width < 992. { 20. } else { 32. }))
            .child(content);
        div().id("application-shell").test_support().relative().size_full().min_w_0().min_h_0().overflow_hidden().flex().bg(cx.theme().background).text_size(px(14.)).text_color(cx.theme().foreground)
            .child(self.sidebar(cx))
            .child(div().id("main-content").test_support().flex_1().min_w_0().h_full().flex().flex_col()
                .when(self.page != Page::Dashboard, |column| column.child(div().id("page-heading").test_support().h(px(88.)).flex_shrink_0().px_8().flex().items_center().border_b_1().border_color(cx.theme().border)
                    .child(div().text_size(px(26.)).font_weight(FontWeight::BOLD).child(self.page_title()))))
                .child(content))
            .when_some(self.source_dialog(cx),|shell,dialog|shell.child(dialog))

    }
}


/// 保留原生窗口左上角，尺寸统一采用内容视口而不是含装饰的外框。
fn window_geometry(frame: gpui_kit::Bounds<gpui_kit::Pixels>, viewport: gpui_kit::Size<gpui_kit::Pixels>) -> [f32; 4] {
    [frame.origin.x.into(), frame.origin.y.into(), viewport.width.into(), viewport.height.into()]
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
