//! 本机图库和复用的原生图片投影；磁盘读取与解码都不发生在 render 内。

use super::{
    Shell,
    state::{CatalogSuccess, Category, ImageEntry},
};
use crate::monitor::{image_decode::MonitorImageDecoder, image_playback::MonitorImageView,
    image_residency::{self, ImageOwner}};
use gpui_kit::{
    AnyElement, Bounds, ContentMask, Context, Corners, FontWeight, IntoElement, ObjectFit, Pixels,
    Role, Window,
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
                Ok(success) => (
                    match success {
                        CatalogSuccess::ImagesUploaded { count } => self
                            .t("monitor.images.uploaded")
                            .replace("%{count}", &count.to_string()),
                        CatalogSuccess::UploadedAndSelected { filename } => self
                            .t("monitor.picker.uploadedAndSelected")
                            .replace("%{filename}", filename),
                    },
                    rgb(0x20c997),
                    Role::Status,
                ),
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
        corners: Corners<Pixels>,
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
        let demand_owner = ImageOwner::catalog(&entry.residency, window, cx);
        let mut preview = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .child(checkerboard(checker, corners, cx));
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
                            image_residency::acquire(source, &demand_owner, cx);
                            if let Some(result) = window.use_asset::<MonitorImageDecoder>(source, cx) {
                                image_residency::remember(source, &result, cx);
                            }
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
                            cx.new(|cx| {
                                MonitorImageView::new(source, fit, window, cx)
                                    .with_viewport_corners(corners)
                            }),
                        )
                    });
                preview = preview.child(
                    div()
                        .id(id)
                        .test_support()
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
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - images_view.rs：图片管理支持真实导入、刷新、格式筛选和受引用约束的删除。；释放此图库 entry 的挂载需求；App 最后 owner 才回收 CPU 与全部窗口图集。；背景棋盘单元保持固定像素尺寸，透明图不被预先合成到主题颜色。
include!("images_view.rs");
