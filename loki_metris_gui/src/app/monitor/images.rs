//! 本机图库和复用的原生图片投影；磁盘读取与解码都不发生在 render 内。

use super::{
    Shell,
    state::{Category, ImageEntry},
};
use crate::monitor::{image_decode::MonitorImageDecoder, image_playback::MonitorImageView};
use gpui_kit::{
    AnyElement, Bounds, Context, FontWeight, IntoElement, ObjectFit, Role, Window,
    base::TestSupportExt as _,
    canvas,
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
        menu::{DropdownMenu, PopupMenuItem},
    },
    div, fill, point,
    prelude::*,
    px, rgb, size,
};
use loki_metris_core::HookError;

impl Shell {
    /// 领域错误只用稳定翻译键及受控参数展示。
    pub(super) fn monitor_error_text(&self, error: &HookError) -> String {
        let mut label = self.t(error.code);
        for (key, value) in &error.params {
            label = label.replace(&format!("%{{{key}}}"), value);
        }
        label
    }

    /// 真实查询失败有明确重试入口。
    pub(super) fn monitor_catalog_error(
        &self,
        error: &HookError,
        profiles: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = self.monitor_error_text(error);
        div()
            .id("monitor-catalog-error")
            .w_full()
            .rounded(px(12.))
            .p_4()
            .bg(cx.theme().danger.opacity(0.1))
            .text_color(cx.theme().danger)
            .role(Role::Alert)
            .aria_label(label.clone())
            .child(label)
            .child(
                Button::new("monitor-catalog-retry")
                    .label(self.t("ui.retry"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if profiles {
                            this.load_monitor_profiles(cx);
                            this.load_agent_settings(cx);
                        } else {
                            this.load_monitor_images(cx);
                        }
                    })),
            )
            .into_any_element()
    }

    /// 修改回执不乐观显示成功，取消文件选择器没有提示。
    pub(super) fn monitor_feedback(&self, cx: &Context<Self>) -> Option<AnyElement> {
        self.monitor.catalog.feedback.as_ref().map(|feedback| {
            let (label, color, role) = match feedback {
                Ok(label) => (label.clone(), rgb(0x20c997), Role::Status),
                Err(error) => (
                    self.monitor_error_text(error),
                    cx.theme().danger.into(),
                    Role::Alert,
                ),
            };
            div()
                .id("monitor-catalog-feedback")
                .test_support()
                .role(role)
                .aria_label(label.clone())
                .w_full()
                .p_4()
                .rounded(px(12.))
                .bg(color.opacity(0.1))
                .text_color(color)
                .child(label)
                .into_any_element()
        })
    }

    /// 格式分段同时用于图库与内嵌选择器，各自持有独立筛选值。
    pub(super) fn monitor_categories(&self, picker: bool, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.monitor.catalog;
        let current = if picker {
            state.picker_category
        } else {
            state.category
        };
        let mut row = div()
            .id(if picker {
                "monitor-picker-filter"
            } else {
                "monitor-images-filter"
            })
            .test_support()
            .role(Role::Group)
            .aria_label(self.t("monitor.images.filterAria"))
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(px(10.))
            .bg(cx.theme().muted);
        for (index, category) in Category::ALL.into_iter().enumerate() {
            let count = if picker {
                state.picker_count(category)
            } else {
                state.count(category)
            };
            let label = if category == Category::All {
                self.t(if picker {
                    "monitor.picker.allCount"
                } else {
                    "monitor.images.allCount"
                })
                .replace("%{count}", &count.to_string())
            } else {
                format!(
                    "{} {count}",
                    match category {
                        Category::Jpeg => "JPEG",
                        Category::Png => "PNG",
                        _ => "GIF",
                    }
                )
            };
            row = row.child(
                gpui_kit::base::Button::new((
                    if picker {
                        "monitor-picker-category"
                    } else {
                        "monitor-image-category"
                    },
                    index,
                ))
                .selected(category == current)
                .h(px(if picker { 28. } else { 32. }))
                .px_3()
                .rounded(px(7.))
                .accessibility_label(label.clone())
                .child(label)
                .when(picker, |button| button.flex_1())
                .when(category == current, |button| {
                    button.bg(cx.theme().secondary)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if picker {
                        this.monitor.catalog.picker_category = category;
                    } else {
                        this.monitor.catalog.category = category;
                    }
                    cx.notify();
                })),
            );
        }
        row.into_any_element()
    }

    /// 按实际解码结果渲染图片；加载中与损坏状态不伪装为可用缩略图。
    pub(super) fn monitor_image_preview(
        &self,
        entry: &ImageEntry,
        id: String,
        fit: ObjectFit,
        checker: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Option<(i32, i32)>) {
        let result = match &entry.source {
            Ok(source) if entry.residency.requested.get() => {
                window.get_asset::<MonitorImageDecoder>(source, cx)
            }
            Ok(_) => None,
            Err(_) => Some(Err("error.monitor.imagesReadFailed")),
        };
        let epoch = self.monitor.catalog.frame_epoch.get();
        entry.residency.register(id.clone(), epoch);
        let demand_entry = entry.clone();
        let demand_id = id.clone();
        let mut preview = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(checkerboard(checker, cx));
        // 独立画布先按实际布局决定需求，未进入视口的格子不会启动任何解码。
        preview = preview.child(
            canvas(
                move |bounds, window, cx| {
                    let visible = bounds.intersect(&window.content_mask().bounds);
                    let visible = window.is_visible()
                        && visible.size.width > px(0.)
                        && visible.size.height > px(0.);
                    if let Some(placement) = demand_entry
                        .residency
                        .placements
                        .borrow_mut()
                        .get_mut(&demand_id)
                    {
                        placement.1 = visible;
                    }
                    if visible {
                        if let Ok(source) = &demand_entry.source {
                            demand_entry.residency.requested.set(true);
                            let _ = window.use_asset::<MonitorImageDecoder>(source, cx);
                        }
                    } else if demand_entry.residency.requested.get()
                        && !demand_entry.residency.cleanup_pending.replace(true)
                    {
                        let entry = demand_entry.clone();
                        window.defer(cx, move |window, cx| {
                            entry.residency.cleanup_pending.set(false);
                            if !entry.residency.any_visible() {
                                release_image(&entry, window, cx);
                            }
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );
        let dimensions = match result {
            Some(Ok(image)) => {
                let dimensions = image.natural_size;
                let epoch = self.monitor.catalog.frame_epoch.get();
                let mut views = entry.views.borrow_mut();
                let (_, view) = views
                    .entry(id.clone())
                    .and_modify(|(seen, _)| *seen = epoch)
                    .or_insert_with(|| {
                        let source = entry.source.as_ref().expect("decoded source").clone();
                        (
                            epoch,
                            cx.new(|cx| MonitorImageView::new(source, fit, window, cx)),
                        )
                    });
                preview = preview.child(
                    div()
                        .id(id)
                        .role(Role::Image)
                        .aria_label(entry.filename.clone())
                        .size_full()
                        .child(view.clone()),
                );
                Some((dimensions.width.0, dimensions.height.0))
            }
            other => {
                let label = self.t(if other.is_some() {
                    "monitor.images.unavailable"
                } else {
                    "monitor.images.reading"
                });
                preview = preview.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .px_2()
                        .text_center()
                        .text_size(px(12.))
                        .text_color(cx.theme().muted_foreground)
                        .child(label),
                );
                None
            }
        };
        (preview.into_any_element(), dimensions)
    }

    /// 图片管理支持真实导入、刷新、格式筛选和受引用约束的删除。
    pub(super) fn monitor_images(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.monitor.catalog;
        if let Some(error) = &state.images_error {
            return self.monitor_catalog_error(error, false, cx);
        }
        let mut toolbar = div()
            .id("monitor-images-toolbar")
            .test_support()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_3();
        if !state.images.is_empty() {
            let visible = state.count(state.category);
            let text = if state.category == Category::All {
                self.t("monitor.images.imagesTotal")
                    .replace("%{count}", &state.images.len().to_string())
            } else {
                self.t("monitor.images.imagesFiltered")
                    .replace("%{visible}", &visible.to_string())
                    .replace("%{total}", &state.images.len().to_string())
            };
            toolbar = toolbar.child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(self.monitor_categories(false, cx))
                    .child(
                        div()
                            .id("monitor-images-count")
                            .role(Role::Status)
                            .aria_label(text.clone())
                            .text_size(px(14.))
                            .text_color(cx.theme().muted_foreground)
                            .child(text),
                    ),
            );
        }
        toolbar = toolbar.child(
            div()
                .flex_shrink_0()
                .flex()
                .gap_3()
                .child(
                    Button::new("monitor-images-refresh")
                        .label(self.t("monitor.images.refresh"))
                        .outline()
                        .disabled(
                            state.images_loading || state.uploading || state.deleting.is_some(),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            // 显式刷新也重试此前预算不足或解码失败的缓存结果。
                            this.release_monitor_images(window, cx);
                            this.load_monitor_images(cx);
                        })),
                )
                .child(
                    Button::new("monitor-images-upload")
                        .label(self.t("monitor.images.upload"))
                        .primary()
                        .disabled(state.uploading || state.deleting.is_some())
                        .on_click(
                            cx.listener(|this, _, _, cx| this.upload_monitor_images(None, cx)),
                        ),
                ),
        );
        let mut page = div()
            .id("monitor-images")
            .test_support()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_5()
            .child(toolbar)
            .when_some(self.monitor_feedback(cx), |page, feedback| {
                page.child(feedback)
            });
        if !state.images_loaded {
            return page
                .child(
                    div()
                        .p_8()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.t("monitor.picker.loading")),
                )
                .into_any_element();
        }
        if state.images.is_empty() {
            return page
                .child(
                    div()
                        .w_full()
                        .min_h(px(300.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .rounded(px(20.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary)
                        .p_6()
                        .child(
                            div()
                                .text_size(px(20.))
                                .font_weight(FontWeight::BOLD)
                                .child(self.t("monitor.images.emptyTitle")),
                        )
                        .child(
                            div()
                                .max_w(px(360.))
                                .mt_2()
                                .text_center()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.t("monitor.images.emptyDescription")),
                        )
                        .child(
                            Button::new("monitor-images-empty-upload")
                                .label(self.t("monitor.images.chooseMultiple"))
                                .mt_5()
                                .disabled(state.uploading)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.upload_monitor_images(None, cx)
                                })),
                        ),
                )
                .into_any_element();
        }
        let mut grid = div()
            .id("monitor-images-grid")
            .test_support()
            .w_full()
            .flex_shrink_0()
            .grid()
            .grid_cols(if self.viewport_width >= 1408. {
                4
            } else if self.viewport_width >= 992. {
                3
            } else if self.viewport_width >= 576. {
                2
            } else {
                1
            })
            .gap_4();
        let entries = state
            .images
            .iter()
            .filter(|image| state.category.accepts(image.format));
        for entry in entries {
            let (preview, dimensions) = self.monitor_image_preview(
                entry,
                format!("monitor-gallery-img-{}", entry.id),
                ObjectFit::Cover,
                18.,
                window,
                cx,
            );
            let dimension_text = dimensions
                .map(|(width, height)| format!("{width} × {height}"))
                .unwrap_or_else(|| {
                    self.t(if entry.source.is_err() {
                        "monitor.images.unavailable"
                    } else {
                        "monitor.images.reading"
                    })
                });
            let id = entry.id.clone();
            let weak = cx.entity().downgrade();
            let deleting = state.deleting.is_some() || state.uploading;
            let delete_label = self.t("monitor.images.delete");
            let menu = Button::new(format!("monitor-image-actions-{}", entry.id))
                .label("···")
                .ghost()
                .accessibility_label(
                    self.t("monitor.images.actions")
                        .replace("%{name}", &entry.filename),
                )
                .dropdown_menu(move |menu, _, _| {
                    let id = id.clone();
                    let weak = weak.clone();
                    menu.item(
                        PopupMenuItem::new(delete_label.clone())
                            .disabled(deleting)
                            .on_click(move |_, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.delete_monitor_image(id.clone(), cx)
                                });
                            }),
                    )
                });
            grid = grid.child(
                div()
                    .id(format!("monitor-image-card-{}", entry.id))
                    .test_support()
                    .min_w_0()
                    .rounded(px(16.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .aspect_ratio(4. / 3.)
                            .child(preview)
                            .child(
                                div()
                                    .absolute()
                                    .top(px(10.))
                                    .right(px(10.))
                                    .px_2()
                                    .rounded_full()
                                    .text_size(px(10.))
                                    .bg(rgb(0x222222).opacity(0.82))
                                    .text_color(rgb(0xffffff))
                                    .child(entry.format.extension().to_uppercase()),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .p_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(dimension_text),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(self.t("monitor.images.localLibrary")),
                                    ),
                            )
                            .child(menu),
                    ),
            );
        }
        if state.count(state.category) == 0 {
            page = page.child(
                div()
                    .py(px(64.))
                    .text_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.t("monitor.images.emptyCategory")),
            );
        } else {
            page = page.child(grid);
        }
        page.into_any_element()
    }
}

/// 最后一个可见挂载位置离开时，同时释放 CPU 资源与所有动画帧的 GPU 图集。
pub(super) fn release_image(entry: &ImageEntry, window: &mut Window, cx: &mut gpui_kit::App) {
    let mut decoded = None;
    if entry.residency.requested.replace(false) {
        if let Ok(source) = &entry.source {
            decoded = window
                .get_asset::<MonitorImageDecoder>(source, cx)
                .and_then(Result::ok);
            cx.remove_asset::<MonitorImageDecoder>(source);
        }
    }
    for (_, view) in entry.views.borrow().values() {
        let released = view.update(cx, |view, cx| view.release_pixels(cx));
        if decoded.is_none() {
            decoded = released;
        }
    }
    if let Some(decoded) = decoded {
        let _ = window.drop_image(decoded.image.clone());
    }
}

/// 背景棋盘单元保持固定像素尺寸，透明图不被预先合成到主题颜色。
fn checkerboard(cell: f32, cx: &Context<Shell>) -> AnyElement {
    let muted = cx.theme().muted;
    let background = cx.theme().secondary;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            window.paint_quad(fill(bounds, background));
            let width: f32 = bounds.size.width.into();
            let height: f32 = bounds.size.height.into();
            for row in 0..(height / cell).ceil() as usize {
                for column in 0..(width / cell).ceil() as usize {
                    if (row + column) % 2 == 0 {
                        let x = column as f32 * cell;
                        let y = row as f32 * cell;
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin + point(px(x), px(y)),
                                size(px(cell.min(width - x)), px(cell.min(height - y))),
                            ),
                            muted,
                        ));
                    }
                }
            }
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}
