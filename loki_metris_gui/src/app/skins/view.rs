//! 皮肤资源卡保持参考的预览高度、响应式列数与受控菜单。

use super::{ModalKind, Shell, skin_reference};

#[path = "badge.rs"]
mod badge;
use badge::{SkinBadge, skin_badge};
#[path = "toolbar.rs"]
mod toolbar;
use crate::app::surface::{HistoricalButtonSize, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, Role, Window,
    assets::IconName,
    base::{Checkbox, CheckboxState, TestSupportExt as _},
    component::{
        ActiveTheme, Disableable, Icon,
        button::{Button, ButtonVariants},
        input::Input,
        menu::{DropdownMenu, PopupMenuItem},
    },
    div,
    prelude::*,
    px, rems, rgb,
};
use loki_metris_core::{ColorMode, SkinHostKind, SkinPackageType, SkinSource};

pub(super) use super::preview::skin_preview;

impl Shell {
    /// 宿主选择共用同一查询交接；同值不失效查询，保持历史受控Tabs的查询键语义。
    fn select_skin_host(&mut self, host: SkinHostKind, cx: &mut Context<Self>) {
        if self.skins.busy || self.skins.modal.is_some() || self.skins.host == Some(host) { return; }
        self.skins.generation = self.skins.generation.wrapping_add(1);
        self.skins.query.take();
        self.skins.loading = false;
        self.skins.authority.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.skins.host = Some(host);
        self.skins.status = None;
        self.skins.instances.clear();
        self.skins.host_error = None;
        self.skins.last_read = None;
        self.refresh_skins(true, cx);
        cx.notify();
    }
    /// 初次读取失败不展示伪造空库，刷新失败保留已确认卡片并提供重试。
    pub(in crate::app) fn skins_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let hosts = self.skin_hosts();
        let mut page = div()
            .id("skins-page")
            .test_support()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(20.))
            .on_drop(cx.listener(|this, paths: &gpui_kit::ExternalPaths, _, cx| {
                this.prepare_skin_import(paths.paths().to_vec(), cx)
            }));
        if hosts.is_empty() {
            return page
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .child(self.t("skins.host.empty_title")),
                )
                .child(self.t("skins.host.empty_description"))
                .into_any_element();
        }
        let writable = self.skin_writable();
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
                toolbar::icon_action(toolbar::default_icon_button("skin-refresh", self.skins.busy || self.skins.loading, cx)
                    .child(Icon::new(IconName::RefreshCw).size(px(18.)))
                    .rounded(px(12.))
                    .accessibility_label(self.t("skins.action.refresh"))
                    .size(px(36.))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh_skins(true, cx))),
                    "skin-refresh-press", self.skins.busy || self.skins.loading),
            );
        page = page.child(toolbar);
        if let Some(draft) = &self.skins.import {
            if draft.batch.is_none() {
                let (processed, total) = draft.lease.session.progress();
                page = page.child(
                    div().id("skin-import-progress").role(Role::Status).child(
                        self.t("skins.progress.preparing")
                            .replace("%{processed}", &processed.to_string())
                            .replace("%{total}", &total.to_string()),
                    ),
                );
            }
        }
        if let Some(error) = self
            .skins
            .catalog_error
            .as_ref()
            .or(self.skins.host_error.as_ref())
        {
            page = page.child(
                div()
                    .id("skins-query-error")
                    .role(Role::Alert)
                    .p_4()
                    .rounded(px(12.))
                    .bg(cx.theme().danger.opacity(0.1))
                    .text_color(cx.theme().danger)
                    .child(self.t("skins.error.title"))
                    .child(div().mt_1().child(self.skin_error(error)))
                    .child(
                        Button::new("skins-query-retry")
                            .outline()
                            .mt_2()
                            .label(self.t("skins.action.refresh"))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_skins(true, cx))),
                    ),
            );
        }
        if let Some(feedback) = &self.skins.feedback {
            let (message, color) = match feedback {
                Ok(message) => (message.clone(), rgb(0x12b886)),
                Err(error) => (self.skin_error(error), rgb(0xfa5252)),
            };
            page = page.child(
                div()
                    .id("skins-feedback")
                    .role(Role::Alert)
                    .p_4()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color.opacity(0.3))
                    .bg(color.opacity(0.09))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(message)
                    .child(
                        Button::new("skins-dismiss-feedback")
                            .ghost()
                            .label("×")
                            .accessibility_label(self.t("skins.dismiss"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.skins.feedback = None;
                                cx.notify();
                            })),
                    ),
            );
        }
        if !self.skins.selected.is_empty() {
            page = page.child(
                Button::new("skins-delete-selected")
                    .outline()
                    .text_color(cx.theme().danger)
                    .label(
                        self.t("skins.action.delete_selected")
                            .replace("%{count}", &self.skins.selected.len().to_string()),
                    )
                    .disabled(!writable)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_skin_dialog(
                            ModalKind::Delete(this.skins.selected.clone()),
                            window,
                            cx,
                        )
                    })),
            );
        }
        let Some(entries) = &self.skins.entries else {
            return page.child(self.t("skins.loading")).into_any_element();
        };
        let query = self
            .skins
            .search
            .as_ref()
            .map(|input| input.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let filtered: Vec<_> = entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || [&entry.skin.name, &entry.skin.author, &entry.skin.id]
                        .into_iter()
                        .any(|value| value.to_lowercase().contains(&query))
            })
            .collect();
        if filtered.is_empty() {
            return page.child(self.t("skins.catalog.empty")).into_any_element();
        }
        let columns = if self.viewport_width >= 1408. {
            4
        } else if self.viewport_width >= 1200. {
            3
        } else if self.viewport_width >= 768. {
            2
        } else {
            1
        };
        let active = self.active_skin();
        let cards = div()
            .w_full()
            .grid()
            .grid_cols(columns)
            .gap(px(20.))
            .children(filtered.into_iter().map(|entry| {
                let skin = entry.skin.clone();
                let reference = skin_reference(&skin);
                let user = skin.source == SkinSource::User;
                let selected = self.skins.selected.contains(&reference);
                let current = active.as_ref() == Some(&reference);
                let mut preview = div()
                    .relative()
                    .rounded_t(px(19.))
                    .h(px(160.))
                    .w_full()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .bg(rgb(0x808b9a));
                // 原160px padding-box的R19同时裁切后代绘制与真实Hitbox。
                #[cfg(target_os = "macos")]
                {
                    preview = preview.clip_rounded_children();
                }
                if let Some(image) = &entry.preview {
                    preview = preview.child(skin_preview(
                        image.clone().into(),
                        format!("skin-preview-{}-{:?}", skin.id, skin.source).into(),
                        self.t("skins.card.preview_alt")
                            .replace("%{name}", &skin.name),
                        self.skins.preview_residencies.clone(),
                    ));
                }
                if user {
                    let reference = reference.clone();
                    let entity = cx.entity().downgrade();
                    preview = preview.child(
                        Checkbox::new(format!("skin-select-{}", skin.id))
                            .checked(selected)
                            .disabled(!writable)
                            .accessibility_label(
                                self.t("skins.card.select").replace("%{name}", &skin.name),
                            )
                            .absolute()
                            .right(px(-4.))
                            .top(px(-4.))
                            .size(px(20.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(if selected {
                                cx.theme().primary
                            } else {
                                cx.theme().secondary
                            })
                            .child(if selected { "✓" } else { "" })
                            .on_change(move |checked, _, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    this.skins.selected.retain(|item| *item != reference);
                                    if checked == CheckboxState::Checked {
                                        this.skins.selected.push(reference.clone());
                                    }
                                    cx.notify();
                                });
                            }),
                    );
                }
                let apply = skin.clone();
                let mut controls = div().w_full().flex().items_center().gap_2().child(
                    crate::app::surface::SurfaceButton::new(
                        format!("skin-apply-{}-{:?}", skin.id, skin.source),
                        if current {
                            crate::app::surface::SurfaceButtonVariant::LightRed
                        } else {
                            crate::app::surface::SurfaceButtonVariant::LightBrand
                        },
                        cx.theme().mode.is_dark(),
                    )
                    .historical_size(crate::app::surface::HistoricalButtonSize::Small)
                    .flex_1()
                    .rounded(px(12.))
                    .loading(self.skins.busy)
                    .disabled(!writable)
                    .label(self.t(if current {
                        "skins.action.stop"
                    } else {
                        "skins.action.apply"
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if current {
                            this.stop_skin(cx)
                        } else {
                            this.apply_skin(&apply, cx)
                        }
                    })),
                );
                if user {
                    let entity = cx.entity().downgrade();
                    let item = skin.clone();
                    let labels = [
                        self.t("skins.action.open"),
                        self.t("skins.action.export"),
                        self.t("skins.action.convert"),
                        self.t("skins.action.delete"),
                    ];
                    controls = controls.child(
                        Button::new(format!("skin-actions-{}", skin.id))
                            .outline()
                            .label("···")
                            .rounded(px(12.))
                            .size(px(36.))
                            .disabled(!writable)
                            .accessibility_label(
                                self.t("skins.card.actions").replace("%{name}", &skin.name),
                            )
                            .dropdown_menu(move |mut menu, _, _| {
                                for (index, label) in labels.iter().enumerate() {
                                    if index == 2
                                        && item.package_type != SkinPackageType::LegacySkin
                                    {
                                        continue;
                                    }
                                    let entity = entity.clone();
                                    let item = item.clone();
                                    menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                                        move |_, window, cx| {
                                            let _ = entity.update(cx, |this, cx| match index {
                                                0 => this
                                                    .open_skin_directory(skin_reference(&item), cx),
                                                1 => this.export_skin(skin_reference(&item), cx),
                                                2 => this.open_skin_dialog(
                                                    ModalKind::Convert(item.clone()),
                                                    window,
                                                    cx,
                                                ),
                                                _ => this.open_skin_dialog(
                                                    ModalKind::Delete(vec![skin_reference(&item)]),
                                                    window,
                                                    cx,
                                                ),
                                            });
                                        },
                                    ));
                                }
                                menu
                            }),
                    );
                }
                let source = self.t(if user {
                    "skins.source.user"
                } else {
                    "skins.source.builtin"
                });
                let kind = self.t(if skin.package_type == SkinPackageType::Theme {
                    "skins.package.theme"
                } else {
                    "skins.package.legacySkin"
                });
                crate::app::surface::SpotlightCard::new(
                    format!("skin-card-{}-{:?}", skin.id, skin.source),
                    cx.theme().mode.is_dark(),
                )
                .text_color(rgb(if cx.theme().mode.is_dark() {
                    0xc9c9c9
                } else {
                    0x000000
                }))
                .test_support()
                .min_w_0()
                .h_auto()
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(preview)
                .child(
                    div()
                        .id(format!("skin-metadata-{}-{:?}", skin.id, skin.source))
                        .test_support()
                        .role(Role::Label)
                        .aria_value(format!(
                            "{} {} · {} {} {} {}",
                            skin.name,
                            self.t("skins.card.by").replace("%{author}", &skin.author),
                            skin.version,
                            kind,
                            source,
                            skin.supported_color_modes
                                .iter()
                                .map(|mode| self.t(if *mode == ColorMode::Light {
                                    "skins.mode.light"
                                } else {
                                    "skins.mode.dark"
                                }))
                                .collect::<Vec<_>>()
                                .join(" ")
                        ))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_start()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(16.))
                                                .line_height(gpui_kit::relative(1.55))
                                                .font_weight(FontWeight::BOLD)
                                                .child(skin.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(gpui_kit::relative(1.4))
                                                .truncate()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!(
                                                    "{} · {}",
                                                    self.t("skins.card.by")
                                                        .replace("%{author}", &skin.author),
                                                    skin.version
                                                )),
                                        ),
                                )
                                .child(skin_badge(
                                    format!("skin-kind-{}-{:?}", skin.id, skin.source),
                                    kind,
                                    if skin.package_type == SkinPackageType::Theme {
                                        SkinBadge::Theme
                                    } else {
                                        SkinBadge::Legacy
                                    },
                                    cx.theme().mode.is_dark(),
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.))
                                .child(skin_badge(
                                    format!("skin-source-{}-{:?}", skin.id, skin.source),
                                    source,
                                    SkinBadge::Source { user },
                                    cx.theme().mode.is_dark(),
                                ))
                                .children(skin.supported_color_modes.iter().map(|mode| {
                                    skin_badge(
                                        format!("skin-mode-{}-{:?}-{mode:?}", skin.id, skin.source),
                                        self.t(if *mode == ColorMode::Light {
                                            "skins.mode.light"
                                        } else {
                                            "skins.mode.dark"
                                        }),
                                        SkinBadge::Mode,
                                        cx.theme().mode.is_dark(),
                                    )
                                })),
                        )
                        .child(controls),
                )
            }));
        page.child(cards).into_any_element()
    }
    /// 稳定码有专用翻译时优先，未知错误保留服务消息及可重试原因。
    fn skin_error(&self, error: &crate::skins::SkinError) -> String {
        match error.code {
            "skin.choose_instance" | "skin.host_instance_selection_required" => {
                self.t("skins.error.choose_instance")
            }
            "skin.injection_failed" => self.t("skins.error.injection_failed"),
            _ => {
                if self.locale == "zh-CN" {
                    error.message.clone()
                } else {
                    format!("{} ({})", self.t("skins.error.unknown"), error.code)
                }
            }
        }
    }
}
