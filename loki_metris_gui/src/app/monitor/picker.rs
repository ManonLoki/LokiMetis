//! 行为图片选择器以内嵌面板承载筛选、单图直传和真实图库选择。

use super::{
    Shell,
    management::behavior_key,
    state::{Category, ImageEntry},
};
use gpui_kit::{
    AnyElement, Context, Corners, FontWeight, IntoElement, ObjectFit, Window,
    base::TestSupportExt as _,
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
    },
    div,
    prelude::*,
    px, rgb,
};
use loki_metris_core::{AiTool, HookBehavior};

impl Shell {
    /// 图片选择器以内嵌面板展开，独立筛选、清空和直接上传都只编辑本草稿。
    pub(super) fn monitor_image_picker(
        &self,
        tool: AiTool,
        behavior: HookBehavior,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = &self.monitor.catalog;
        let key = behavior_key(behavior);
        let selected = state
            .images
            .iter()
            .find(|image| image.id == value && image.source.is_ok());
        let opened = state.picker == Some(behavior);
        let disabled = state.images_loading || state.uploading;
        let mut field = div().w_full().flex().flex_col().gap_2();
        field = field.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(14.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(self.t("monitor.picker.field"))
                        .child(div().text_color(cx.theme().danger).child(" *")),
                )
                .when(!value.trim().is_empty(), |row| {
                    row.child(
                        Button::new(format!("monitor-picker-clear-{key}"))
                            .label("×")
                            .ghost()
                            .accessibility_label(self.t("monitor.picker.clearAria"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_monitor_image(tool, behavior, String::new(), cx)
                            })),
                    )
                }),
        );
        let mut trigger = gpui_kit::base::Button::new(format!("monitor-picker-trigger-{key}"))
            .selected(opened)
            .disabled(disabled)
            .w_full()
            .h(px(132.))
            .p_0()
            .relative()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(rgb(if cx.theme().mode.is_dark() {
                0x1e1c1a
            } else {
                0xf0ebe3
            }))
            .accessibility_label(self.t("monitor.picker.field"));
        if let Some(selected) = selected {
            trigger = trigger
                .child(
                    self.monitor_image_preview(
                        selected,
                        format!("monitor-picker-current-{key}-{}", selected.id),
                        ObjectFit::Contain,
                        16.,
                        Corners::all(px(11.)),
                        window,
                        cx,
                    )
                    .0,
                )
                .child(
                    div()
                        .absolute()
                        .right_2()
                        .bottom_2()
                        .px_2()
                        .py_1()
                        .rounded(px(8.))
                        .bg(rgb(0x14141a).opacity(0.84))
                        .text_color(rgb(0xffffff))
                        .text_size(px(12.))
                        .child(self.t("monitor.picker.change")),
                );
        } else {
            trigger = trigger.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child(self.t(if disabled {
                        "monitor.picker.loading"
                    } else {
                        "monitor.picker.clickChoose"
                    })),
            );
        }
        field = field.child(trigger.on_click(cx.listener(move |this, _, _, cx| {
            this.monitor.catalog.picker = if opened { None } else { Some(behavior) };
            this.monitor.catalog.picker_category = Category::All;
            cx.notify();
        })));
        if !opened {
            return field.into_any_element();
        }
        let visible = state.picker_count(state.picker_category);
        let total = state.picker_count(Category::All);
        let count = if state.picker_category == Category::All {
            self.t("monitor.picker.imagesTotal")
                .replace("%{count}", &total.to_string())
        } else {
            self.t("monitor.picker.imagesFiltered")
                .replace("%{visible}", &visible.to_string())
                .replace("%{total}", &total.to_string())
        };
        let mut panel = div()
            .id(format!("monitor-picker-panel-{key}"))
            .test_support()
            .w_full()
            .p_3()
            .rounded(px(12.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.t("monitor.picker.chooseTitle")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(count),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new(format!("monitor-picker-upload-{key}"))
                                    .label(self.t(if state.uploading {
                                        "monitor.picker.uploading"
                                    } else {
                                        "monitor.picker.uploadSingle"
                                    }))
                                    .accessibility_label(self.t("monitor.picker.uploadSingleAria"))
                                    .disabled(disabled)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.upload_monitor_images(Some((tool, behavior)), cx)
                                    })),
                            )
                            .child(
                                Button::new(format!("monitor-picker-close-{key}"))
                                    .label("×")
                                    .ghost()
                                    .accessibility_label(self.t("monitor.picker.closeAria"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.monitor.catalog.picker = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(self.monitor_categories(true, cx));
        let mut options = div()
            .id(format!("monitor-picker-options-{key}"))
            .test_support()
            .w_full()
            .max_h(px(300.))
            .overflow_y_scroll()
            .grid()
            .grid_cols(4)
            .gap_2();
        for entry in state
            .images
            .iter()
            .filter(|image| state.picker_category.accepts(image.format) && image.source.is_ok())
        {
            options =
                options.child(self.monitor_picker_option(tool, behavior, value, entry, window, cx));
        }
        if visible == 0 {
            panel = panel.child(
                div()
                    .p_4()
                    .text_center()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child(self.t("monitor.picker.emptyCategory")),
            );
        } else {
            panel = panel.child(options);
        }
        field.child(panel).into_any_element()
    }

    /// 每个选项使用独立稳定 img ID，不把选择区域包成另一个可点击卡片。
    fn monitor_picker_option(
        &self,
        tool: AiTool,
        behavior: HookBehavior,
        value: &str,
        entry: &ImageEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = behavior_key(behavior);
        let id = entry.id.clone();
        let selected = entry.id == value;
        let preview = self
            .monitor_image_preview(
                entry,
                format!("monitor-picker-option-img-{key}-{}", entry.id),
                ObjectFit::Contain,
                12.,
                Corners::all(px(10.)),
                window,
                cx,
            )
            .0;
        gpui_kit::base::Button::new(format!("monitor-picker-option-{key}-{}", entry.id))
            .selected(selected)
            .w_full()
            .aspect_ratio(1.)
            .p_0()
            .relative()
            .overflow_hidden()
            .rounded(px(12.))
            .border_2()
            .border_color(if selected {
                cx.theme().primary
            } else {
                cx.theme().border.opacity(0.)
            })
            .accessibility_label(
                self.t("monitor.picker.selectAria")
                    .replace("%{filename}", &entry.filename),
            )
            .child(preview)
            .when(selected, |button| {
                button.child(
                    div()
                        .absolute()
                        .top_1()
                        .right_1()
                        .size(px(22.))
                        .rounded_full()
                        .bg(cx.theme().primary)
                        .text_color(cx.theme().primary_foreground)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child("✓"),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_monitor_image(tool, behavior, id.clone(), cx)
            }))
            .into_any_element()
    }
}
