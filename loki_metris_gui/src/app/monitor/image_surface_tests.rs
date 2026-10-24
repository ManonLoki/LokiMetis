//! 在真实管理页读取已完成的图片图元和棋盘几何，验证各挂载位置的内侧圆角。

use super::super::{
    Shell,
    state::{ImageEntry, Route},
};
use crate::monitor_media::image_decode::MonitorImageSource;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Corners, Entity, Pixels, TestAppContext, Window,
    component::ActiveTheme, px, test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, AiTool, public_monitor_ai_tools};
use std::sync::Arc;

/// 实际 GIF 同时进入图库和默认档案，所有偏好与资源只属于测试窗口。
fn setup(
    cx: &mut TestAppContext,
    management: bool,
) -> (
    AnyWindowHandle,
    Entity<Shell>,
    crate::app::tests::FinishWriter,
) {
    let (handle, shell, writer) = crate::app::tests::open_shell(cx, 1440., 1400.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = if management {
            Route::Management
        } else {
            Route::Images
        };
        this.monitor.catalog.accept_profiles(
            public_monitor_ai_tools()
                .map(AiProfileDraft::default_for)
                .collect(),
        );
        this.monitor
            .catalog
            .editor_mut(AiTool::Codex)
            .unwrap()
            .draft
            .hooks[0]
            .image = "rounded-fixture".into();
        let source = MonitorImageSource {
            id: "rounded-fixture".into(),
            bytes: Arc::from(include_bytes!("../../../tests/fixtures/monitor-loop.gif").as_slice()),
            format: loki_metris_core::ImageFormat::Gif,
        };
        this.monitor.catalog.images = vec![ImageEntry {
            id: source.id.clone(),
            filename: "rounded.gif".into(),
            format: source.format,
            source: Ok(source),
            views: Default::default(),
            residency: Default::default(),
        }];
        this.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    settle(cx, handle);
    (handle, shell, writer)
}

/// 先由真实需求画布发起解码，再让播放器完成帧绘制。
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    for _ in 0..4 {
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .unwrap();
        cx.run_until_parked();
    }
}

/// 图片和每个棋盘格共享原视口圆心；矩形 mask 仅决定单元格而不重建圆角。
fn assert_preview(
    window: &Window,
    app: &App,
    id: &str,
    corners: Corners<Pixels>,
    cell: f32,
) -> gpui_kit::AtlasTile {
    let viewport = window
        .find(id.to_owned())
        .bounds()
        .scale(window.scale_factor());
    let sprites = window.painted_polychrome_sprites();
    let image = sprites
        .iter()
        .find(|sprite| sprite.rounded_clip_bounds == viewport)
        .expect("真实图片图元必须保留原视口而非拟合图或 guard 框");
    assert_eq!(image.corner_radii, corners.scale(window.scale_factor()));
    let quads = window.painted_quads();
    let base = quads
        .iter()
        .find(|quad| {
            quad.bounds == viewport && quad.background.as_solid() == Some(app.theme().secondary)
        })
        .expect("真实棋盘底色必须覆盖完整图片视口");
    assert_eq!(base.corner_radii, corners.scale(window.scale_factor()));
    let first_cell = quads
        .iter()
        .find(|quad| {
            quad.bounds == viewport
                && quad.background.as_solid() == Some(app.theme().muted)
                && quad.content_mask.bounds.origin == viewport.origin
        })
        .expect("棋盘首格必须以原视口形状绘制并保留单元格 mask");
    assert_eq!(
        first_cell.corner_radii,
        corners.scale(window.scale_factor())
    );
    assert_eq!(
        first_cell.content_mask.bounds.size.width,
        px(cell).scale(window.scale_factor())
    );
    assert_eq!(
        first_cell.content_mask.bounds.size.height,
        px(cell).scale(window.scale_factor())
    );
    image.tile
}

/// 四比三图库只裁顶部十九像素内角，底部直线连接卡片信息。
#[gpui_kit::test]
fn gallery_image_and_checker_keep_card_inner_top_corners(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = setup(cx, false);
    cx.update_window(handle, |_, window, app| {
        assert_preview(
            window,
            app,
            "monitor-gallery-img-rounded-fixture",
            Corners {
                top_left: px(19.),
                top_right: px(19.),
                bottom_left: px(0.),
                bottom_right: px(0.),
            },
            18.,
        );
    })
    .unwrap();
}

/// 一像素触发边框和两像素选项边框分别对应十一、十像素内角，图片帧共用图集。
#[gpui_kit::test]
fn picker_image_and_checker_keep_border_insets_and_shared_tile(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = setup(cx, true);
    cx.update_window(handle, |_, window, app| {
        window.click("monitor-picker-trigger-idle", app);
    })
    .unwrap();
    settle(cx, handle);
    cx.update_window(handle, |_, window, app| {
        let current = assert_preview(
            window,
            app,
            "monitor-picker-current-idle-rounded-fixture",
            Corners::all(px(11.)),
            16.,
        );
        let option = assert_preview(
            window,
            app,
            "monitor-picker-option-img-idle-rounded-fixture",
            Corners::all(px(10.)),
            12.,
        );
        assert_eq!(
            current, option,
            "同一 GIF 帧不能因不同视口圆角而复制 GPU tile"
        );
        assert_eq!(
            window
                .find("monitor-picker-trigger-idle")
                .bounds()
                .size
                .height,
            px(132.)
        );
    })
    .unwrap();
}
