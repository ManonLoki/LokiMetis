//! 原生壳层只拥有导航、外观和设备偏好；领域状态始终从直接依赖的共享核心读取。

mod settings;
mod surface;
mod monitor;
mod skins;
mod dashboard;
mod keyboard_entry;
mod window_title;
#[cfg(target_os="macos")]
mod window_state;
#[cfg(all(windows, not(test)))]
#[path = "window_state_windows.rs"]
mod window_state;




use crate::{assets, preferences::{Preferences, ThemeChoice, Writer}};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Render, Subscription, Window, WindowAppearance, base::TestSupportExt as _, component::{ActiveTheme, Theme, ThemeMode}, div, img, prelude::*, px};

/// 侧栏内边距同时确定顶部 Logo 与边界控件的垂直原点。
const SIDEBAR_INSET: f32 = 6.;
/// 页面枚举只包含本次初始化实际启用的页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Monitor,
    Skins,
    Settings,

}

/// 页面滚动只拥有原生 handle 和弱壳层租约，不能形成根视图引用环。
#[derive(Clone)]
pub(in crate::app) struct PageScroll {
    handle: gpui_kit::ScrollHandle,
    owner: gpui_kit::WeakEntity<Shell>,
    revision: std::rc::Rc<std::cell::Cell<u64>>,
    manual_revision: std::rc::Rc<std::cell::Cell<u64>>,
    regions: std::rc::Rc<std::cell::RefCell<PageRegions>>,
}

/// 当前实际文档及粘滞绘制边界只属于展示，不改变完整视口的原生焦点可见判定。
#[derive(Default)]
struct PageRegions {
    container: Option<gpui_kit::Bounds<gpui_kit::Pixels>>,
    header: Option<gpui_kit::Bounds<gpui_kit::Pixels>>,
    footer: Option<gpui_kit::Bounds<gpui_kit::Pixels>>,
    footer_layout: Option<gpui_kit::LayoutId>,
}

/// 所有壳层可变视图状态由唯一 GPUI 实体拥有，不镜像业务数据。
pub struct Shell {
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    window_state: window_state::MainWindowState,
    monitor:monitor::MonitorView,
    skins:skins::Skins,
    native_window:gpui_kit::AnyWindowHandle,
    agent_settings:settings::AgentSettingsState,
    preference_save:settings::PreferenceSaveState,
    notification_details_expanded: bool,
    repository:settings::RepositoryState,
    dashboard: dashboard::Dashboard,
    page: Page,
    page_scroll: PageScroll,
    viewport_width: f32,
    preferences: Preferences,
    writer: Writer,
    locale: &'static str,
    last_requested_title_locale: Option<&'static str>,
    _appearance: Subscription,
    _bounds: Subscription,
    _activation: Subscription,
    _visibility: Subscription,
    _keyboard_entry: Subscription,
    window_active: bool,
    window_visible: bool,
    release_notes_modal: Option<settings::ReleaseNotesModal>,
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
            let _ = weak.update(cx, |this, cx| {
                this.remember_window(window, cx);
            });
            !crate::native::hide_if_available(cx)
        });
        let bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            #[cfg(any(target_os="macos", all(windows, not(test))))]
            { let _ = (window, cx); this.note_native_geometry_change(); }
            #[cfg(not(any(target_os="macos", all(windows, not(test)))))]
            { this.remember_window(window, cx); }
        });
        let activation = cx.observe_window_activation(window, |this,window,cx| {
            this.update_window_visibility(window,cx);
            // 可见性回调也会更新window_active；原生激活事件不能再按该缓存去重。
            #[cfg(target_os="macos")]
            crate::native::refresh_menu_after_activation(cx);
        });
        let visibility = cx.observe_window_visibility(window, |this,_,window,cx|this.update_window_visibility(window,cx));
        let keyboard_entry = keyboard_entry::install(window.window_handle(), cx);
        let locale = preferences.locale();
        let mut value = Self {
            #[cfg(any(target_os="macos", all(windows, not(test))))]
            window_state: window_state::MainWindowState::capture(window),
            skins:Default::default(), native_window:window.window_handle(), monitor:Default::default(), agent_settings:Default::default(), preference_save:Default::default(), repository:Default::default(), page: Page::Dashboard, page_scroll: PageScroll { handle: Default::default(), owner: cx.weak_entity(), revision: Default::default(), manual_revision: Default::default(), regions: Default::default() }, viewport_width: 1440., dashboard: dashboard::Dashboard::default(), preferences, writer, locale, _appearance: appearance, _bounds: bounds_subscription,
            _activation:activation,_visibility:visibility,window_active:window.is_window_active(),window_visible:window.is_visible(),
            _keyboard_entry:keyboard_entry,
            last_requested_title_locale: None,
            notification_details_expanded: false,
            release_notes_modal: None,
        };
        value.remember_window(window, cx);
        value.apply_language();
        value.apply_theme(window, cx);
        value
    }

    /// 后台或完全不可见窗口暂停页面轮询；恢复前台后由既有计时器补读。
    fn update_window_visibility(&mut self,window:&mut Window,cx:&mut Context<Self>) {
        if !self.window_active && window.is_window_active() {crate::native::refresh_notification_permission(cx);if let Some(service)=&self.dashboard.service {service.refresh_today();}}
        self.window_active=window.is_window_active();self.window_visible=window.is_visible();
        if !self.window_visible {self.release_monitor_images(window,cx);}
        cx.notify();
    }

    /// 保存内容尺寸与有界后台快照，直接退出也能恢复最新状态且不叠加原生标题栏。
    pub(crate) fn remember_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        #[cfg(any(target_os="macos", all(windows, not(test))))]
        { self.remember_native_window_at_boundary(window, cx); }
        #[cfg(not(any(target_os="macos", all(windows, not(test)))))]
        {
        let _ = cx;
        // 原生 v2 尚未支持的平台保留完整可信状态，不能用占位反馈覆盖它。
        if self.preferences.window_state_v2.is_some() { return; }
        if window.is_fullscreen() || !window.is_visible() { return }
        #[cfg(windows)]
        if !crate::native::allows_geometry_save(window) { return; }
        self.preferences.maximized = window.is_maximized();
        if !self.preferences.maximized {
            self.preferences.bounds = Some(window_geometry(window.bounds(), window.viewport_size()));
        }
        self.writer.save_window(self.preferences.bounds,self.preferences.maximized);
        }
    }

    /// 退出先保存已稳定状态，再撤销所有迟到的原生显示动作；恢复中保留最后可信几何。
    pub(crate) fn save_window_before_shutdown(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.remember_window(window, cx);
        #[cfg(any(target_os="macos", all(windows, not(test))))]
        { self.cancel_window_restore(); }
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
            theme.colors.foreground = color(0x141414, 0xf8f9fa);
            theme.colors.secondary = color(0xfffdf9, 0x1e1c1a);
            theme.colors.muted = color(0xfcfaf7, 0x26272b);
            theme.colors.muted_foreground = color(0x868e96, 0x828282);
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
            // 表格和表头沿用外层品牌材质，方形子层不得遮住面板圆角。
            theme.colors.table = theme.colors.secondary.opacity(0.);
            theme.colors.table_head = theme.colors.secondary.opacity(0.);
            theme.colors.table_head_foreground = theme.colors.foreground;
            theme.colors.table_row_border = color(0xe6ddd1, 0x424242);
        });
    }

    /// 组件库词典与应用词典使用相同的有效语言。
    fn apply_language(&self) {
        gpui_kit::component::set_locale(if self.locale == "zh-CN" { "zh-CN" } else { "en" });
    }

    /// 页面切换不创建新的数据副本或重置设备偏好。
    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {self.page_scroll.invalidate();self.dashboard.sources.reset();self.dashboard.usage_settings.reset();self.release_notes_modal=None;}
        if self.page==Page::Skins&&page!=Page::Skins {self.leave_skins();}
        self.page = page;
        if page==Page::Skins {self.refresh_skins(true,cx);}
        if page==Page::Monitor {self.refresh_monitor(true,cx);}
        if page==Page::Settings {self.load_agent_settings(cx);crate::native::refresh_notification_permission(cx);}
        if page==Page::Dashboard {self.reload_usage(false,cx);}
        cx.notify();
    }

    /// 参考产品使用固定紧凑侧栏，整项 Button 拥有点击和辅助标签。
    fn navigation_item(&self, page: Page, id: &'static str, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let label = self.t(key);
        let icon=gpui_kit::component::Icon::default().data(match page {
            Page::Dashboard=>&include_bytes!("../../assets/icons/squares-four.svg")[..],
            Page::Monitor=>&include_bytes!("../../assets/icons/chart-line-up.svg")[..],
            Page::Skins=>&include_bytes!("../../assets/icons/palette.svg")[..],
            Page::Settings=>&include_bytes!("../../assets/icons/gear-six.svg")[..],
        });
        gpui_kit::base::Button::new(id).h_auto().min_h(px(56.)).py_1().px_0().w_full().flex().items_center().justify_center().rounded(px(12.))
            .accessibility_label(label.clone())
            .child(div().w_full().h_auto().flex_shrink_0().flex().flex_col().items_center().gap_1()
                .child(icon.size(px(22.)))
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

    /// 三个业务入口向下增长，公共设置固定在底部。
    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let width = 80.;
        let logo_size = 36.;
        let identity = div().id("sidebar-identity").test_support().relative().w_full().flex_shrink_0().flex().flex_col().items_center().justify_center().gap_2()
            .py_2()
            .child(div().id("sidebar-logo").test_support().size(px(logo_size)).flex_shrink_0().child(img(assets::LOGO).size_full()))
            .child(div().text_size(px(10.)).line_height(px(10.)).text_color(cx.theme().muted_foreground).child(self.version()))
            ;
        let mut sidebar = div().id("sidebar").test_support().w(px(width)).h_full().min_h_0().flex_shrink_0().flex().flex_col()
            .p(px(SIDEBAR_INSET)).border_r_1().border_color(cx.theme().border).bg(cx.theme().secondary)
            .child(identity).child(self.sidebar_divider(cx));
        sidebar = sidebar.child(self.navigation_item(Page::Dashboard, "nav-dashboard", "navigation.dashboard", cx));
        sidebar = sidebar.child(self.navigation_item(Page::Monitor,"nav-monitor","navigation.monitor",cx));
        sidebar = sidebar.child(self.navigation_item(Page::Skins,"nav-skins","navigation.skins",cx));
        sidebar = sidebar.child(div().flex_1());

        sidebar = sidebar.child(self.sidebar_divider(cx));
        sidebar = sidebar.child(self.navigation_item(Page::Settings, "nav-settings", "navigation.settings", cx));
        sidebar.into_any_element()
    }
}

impl Render for Shell {
    /// 原生窗口标题与视图元数据由同一身份和当前语言派生。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.viewport_width = window.viewport_size().width.into();
        *self.page_scroll.regions.borrow_mut() = PageRegions::default();
        self.sync_usage_setting_inputs(window,cx);
        self.sync_monitor_inputs(window,cx);
        self.sync_monitor_hook_inputs(window,cx);
        self.sync_skin_inputs(window,cx);
        self.begin_monitor_image_frame();
        self.request_window_title(window);
        let content = match self.page {
            Page::Dashboard => self.dashboard_page(window,cx),
            Page::Monitor => self.monitor_page(window,cx),
            Page::Skins => self.skins_page(window,cx),
            Page::Settings => self.settings_page(cx),

        };
        self.finish_monitor_image_frame(window,cx);
        let content = div().id("page-content").test_support().flex_1().min_w_0().min_h_0().overflow_y_scroll().track_scroll(&self.page_scroll.handle)
            .on_scroll_wheel({let scroll=self.page_scroll.clone();move|_,_,_|scroll.cancel_pending_reveal()})
            .flex().flex_col()
            .p(px(if self.viewport_width < 992. { 20. } else { 32. }))
            .child(content);
        // 原生模态打开时只隔离背景的辅助树，仍保留其绘制和控件身份。
        let modal_open=self.release_notes_modal.is_some() || self.skin_modal_open() || self.source_modal_open();
        let underlay=div().id("application-underlay").role(gpui_kit::Role::Group).size_full().min_w_0().min_h_0().flex()
            .a11y_synthetic_children(move|builder| {if modal_open {builder.parent_node().set_hidden();}})
            .child(self.sidebar(cx))
            .child(div().id("main-content").test_support().flex_1().min_w_0().h_full().flex().flex_col().child(content));
        div().id("application-shell").test_support().relative().size_full().min_w_0().min_h_0().overflow_hidden().flex().bg(cx.theme().background).text_size(px(14.)).text_color(cx.theme().foreground)
            .child(underlay)
            .when_some(self.skin_dialog(cx),|shell,dialog|shell.child(dialog))
            .when_some(self.source_dialog(cx),|shell,dialog|shell.child(dialog))
            .when_some(self.release_notes_dialog(window,cx),|shell,dialog|shell.child(dialog))

    }
}


/// 保留原生窗口左上角，尺寸统一采用内容视口而不是含装饰的外框。
#[cfg(any(test, not(any(target_os="macos", windows))))]
fn window_geometry(frame: gpui_kit::Bounds<gpui_kit::Pixels>, viewport: gpui_kit::Size<gpui_kit::Pixels>) -> [f32; 4] {
    [frame.origin.x.into(), frame.origin.y.into(), viewport.width.into(), viewport.height.into()]
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
mod keyboard_entry_tests;
