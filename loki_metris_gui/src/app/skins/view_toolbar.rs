//! 皮肤页顶部工具栏：宿主页签、搜索框与导入 / 新建 / 刷新按钮。

use super::*;

impl Shell {
    /// 组装工具栏。页签支持键盘左右 / Home / End 切换；忙碌或弹窗打开时一律禁用，
    /// 避免在写操作进行中切换宿主。`hosts` 为空的情况由调用方先处理。
    pub(super) fn skins_toolbar(&self, hosts: Vec<SkinHostKind>, writable: bool, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Div {
        let mut toolbar = div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px(px(20.))
            .pt(rems(0.65))
            .pb(rems(0.55))
            .border_b_1()
            .border_color(rgb(if cx.theme().mode.is_dark() { 0x38332e } else { 0xeadfd0 }))
            // 历史顶部为不占布局的 inset 高光，不能用上边框撑高整组卡片。
            .shadow(vec![BoxShadow::new(px(0.), px(1.), rgb(0xffffff)
                .opacity(if cx.theme().mode.is_dark() { 0.07 } else { 0.75 }).into()).inset()])
            .bg(toolbar::background(cx.theme().mode.is_dark()));
        let focuses: Vec<_> = hosts.iter().map(|host| {
            window.use_keyed_state(format!("skin-host-focus-{host:?}"), cx, |_, cx| cx.focus_handle())
                .read(cx).clone()
        }).collect();
        let keyboard_hosts = hosts.clone();
        let keyboard_focuses = focuses.clone();
        let mut tabs = div()
            .id("skin-host-tabs")
            .flex()
            .items_center()
            .gap_1()
            .role(Role::TabList)
            .aria_label(self.t("skins.host.tabs"))
            .on_key_down(cx.listener(move |this, event: &gpui_kit::KeyDownEvent, window, cx| {
                if this.skins.busy || this.skins.modal.is_some() { return; }
                let Some(index) = keyboard_focuses.iter().position(|focus| focus.is_focused(window)) else { return; };
                let (next, activate) = match event.keystroke.key.as_str() {
                    "left" => ((index + keyboard_hosts.len() - 1) % keyboard_hosts.len(), true),
                    "right" => ((index + 1) % keyboard_hosts.len(), true),
                    "home" => (0, false),
                    "end" => (keyboard_hosts.len() - 1, false),
                    _ => return,
                };
                window.prevent_default();
                keyboard_focuses[next].focus(window, cx);
                if activate { this.select_skin_host(keyboard_hosts[next], cx); }
                cx.stop_propagation();
            }));
        for (index, host) in hosts.into_iter().enumerate() {
            tabs = tabs.child(
                toolbar::host_tab(
                gpui_kit::base::Button::new(format!("skin-host-{host:?}"))
                    .role(Role::Tab)
                    .aria_selected(self.skins.host == Some(host))
                    .tab_index(if self.skins.host == Some(host) { 0 } else { -1 })
                    .track_focus(&focuses[index])
                    .accessibility_label(host.display_name())
                    .child(host.display_name())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_skin_host(host, cx);
                    })),
                    format!("skin-host-color-{host:?}"),
                    focuses[index].clone(),
                    self.skins.host == Some(host),
                    self.skins.busy || self.skins.modal.is_some(),
                ),
            );
        }
        toolbar = toolbar.child(tabs).child(div().flex_1().min_w(px(12.)));
        if let Some(input) = &self.skins.search {
            toolbar = toolbar.child(
                Input::new(input)
                    .aria_label(self.t("skins.search.label"))
                    .prefix(Icon::new(IconName::Search).size(px(16.))
                        .text_color(rgb(if cx.theme().mode.is_dark() { 0x828282 } else { 0x868e96 })))
                    .bg(rgb(if cx.theme().mode.is_dark() { 0x2e2e2e } else { 0xffffff }))
                    .border_color(rgb(if cx.theme().mode.is_dark() { 0x424242 } else { 0xced4da }))
                    .h(px(36.))
                    .rounded(px(12.))
                    .w(px(320.))
                    .max_w(px(320.)),
            );
        }
        toolbar = toolbar
            .child(
                SurfaceButton::new("skin-import", SurfaceButtonVariant::Default, cx.theme().mode.is_dark())
                    .historical_size(HistoricalButtonSize::Small)
                    .left_icon(Icon::new(IconName::ArrowUp).size(px(18.)).data(&b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='M12 15V3m-4 4 4-4 4 4M4 15v6h16v-6'/></svg>"[..]))
                    .left_icon_size(px(16.))
                    .w(px(86.))
                    .h(px(36.))
                    .rounded(px(12.))
                    .label(self.t("skins.action.import"))
                    .disabled(!writable || self.skins.modal.is_some())
                    .on_click(cx.listener(|this, _, _, cx| this.choose_skin_import(cx))),
            )
            .child(
                Button::new("skin-create")
                    .primary()
                    .icon(Icon::new(IconName::Plus).size(px(18.)))
                    .w(px(114.))
                    .h(px(36.))
                    .rounded(px(12.))
                    .label(self.t("skins.action.create"))
                    .disabled(!writable)
                    .on_click(cx.listener(|this, _, window, cx| this.open_skin_create(window, cx))),
            )
            .child(
                // 周期查询由refresh_skins的loading守卫合并；临时禁用会夺走键盘焦点。
                toolbar::icon_action(toolbar::default_icon_button("skin-refresh", self.skins.busy, cx)
                    .child(Icon::new(IconName::RefreshCw).size(px(18.)))
                    .rounded(px(12.))
                    .accessibility_label(self.t("skins.action.refresh"))
                    .size(px(36.))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_skins(true, cx))),
                    "skin-refresh-press", self.skins.busy),
            );
        toolbar
    }
}
