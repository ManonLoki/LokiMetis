// 本文件是 images.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：图片管理支持真实导入、刷新、格式筛选和受引用约束的删除。；释放此图库 entry 的挂载需求；App 最后 owner 才回收 CPU 与全部窗口图集。；背景棋盘单元保持固定像素尺寸，透明图不被预先合成到主题颜色。

impl Shell {
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
                            for image in &this.monitor.catalog.images {
                                if let Ok(source) = &image.source { image_residency::retry_failed(source, cx); }
                            }
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
                Corners {
                    top_left: px(19.),
                    top_right: px(19.),
                    bottom_left: px(0.),
                    bottom_right: px(0.),
                },
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
                    .rounded(px(20.))
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


#[cfg(test)]
#[path = "image_surface_tests.rs"]
mod rounded_preview_tests;

#[cfg(test)]
#[path = "image_residency_tests.rs"]
mod shared_residency_tests;

/// 释放此图库 entry 的挂载需求；App 最后 owner 才回收 CPU 与全部窗口图集。
pub(super) fn release_image(entry: &ImageEntry, window: &mut Window, cx: &mut gpui_kit::App) {
    entry.residency.requested.set(false);
    for (_, view) in entry.views.borrow().values() {
        view.update(cx, |view, cx| view.set_mounted(false, window, cx));
    }
    if let Ok(source) = &entry.source {
        image_residency::release_catalog(&source.id, &entry.residency, window, cx);
    }
}

/// 背景棋盘单元保持固定像素尺寸，透明图不被预先合成到主题颜色。
fn checkerboard(cell: f32, corners: Corners<Pixels>, cx: &Context<Shell>) -> AnyElement {
    let muted = cx.theme().muted;
    let background = cx.theme().secondary;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let corners = corners.clamp_radii_for_quad_size(bounds.size);
            window.paint_quad(fill(bounds, background).corner_radii(corners));
            let width: f32 = bounds.size.width.into();
            let height: f32 = bounds.size.height.into();
            for row in 0..(height / cell).ceil() as usize {
                for column in 0..(width / cell).ceil() as usize {
                    if (row + column) % 2 == 0 {
                        let x = column as f32 * cell;
                        let y = row as f32 * cell;
                        let cell_bounds = Bounds::new(
                            bounds.origin + point(px(x), px(y)),
                            size(px(cell.min(width - x)), px(cell.min(height - y))),
                        );
                        // 单元格只限制颜色区域，圆心始终属于完整视口。
                        window.with_content_mask(
                            Some(ContentMask { bounds: cell_bounds }),
                            |window| {
                                window.paint_quad(fill(bounds, muted).corner_radii(corners));
                            },
                        );
                    }
                }
            }
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}
