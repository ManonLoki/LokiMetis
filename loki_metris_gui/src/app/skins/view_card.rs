//! 皮肤卡片：预览图、元数据徽章、应用 / 停止按钮与用户皮肤的更多操作菜单。

use super::*;
use crate::app::skins::SkinEntry;
use loki_metris_core::SkinReference;

impl Shell {
    /// 渲染一张皮肤卡片。`active` 为当前正在应用的皮肤，用于把“应用”切换成“停止”；
    /// 内置皮肤不可勾选、不可导出或删除，只有用户皮肤才有选择框与更多操作。
    pub(super) fn skin_card(&self, entry: &SkinEntry, active: &Option<SkinReference>, writable: bool, cx: &mut Context<Self>) -> AnyElement {
        let skin = entry.skin.clone();
        let reference = skin_reference(&skin);
        let user = skin.source == SkinSource::User;
        let selected = self.skins.selected.contains(&reference);
        let current = active.as_ref() == Some(&reference);
        // 原160px padding-box的R19同时裁切后代绘制与真实Hitbox（macOS）。
        let mut preview = crate::app::skins::preview::section_frame(
            div()
                .relative()
                .rounded_t(px(19.))
                .h(px(160.))
                .w_full()
                .flex_shrink_0()
                .overflow_hidden()
                .bg(rgb(0x808b9a)),
        );
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
        ).priority(if current { crate::app::surface::CardPriority::Primary } else { crate::app::surface::CardPriority::Secondary })
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
        ).into_any_element()
    }
}
