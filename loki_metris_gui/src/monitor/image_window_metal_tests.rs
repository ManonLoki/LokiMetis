//! 原生 Metal 多帧图集逐帧查询，证明局部退役实际移除所有纹理且另一窗继续绘制。

use super::*;
use crate::monitor::{image_decode::LoadProbe, image_playback::MonitorImageView};
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Entity, HeadlessAppContext, IntoElement, NoopTextSystem,
    ObjectFit, PlatformAtlas, Render, RenderImageParams, Window, div, prelude::*, px, size,
    test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// 两个实际播放器共享原始字节；可见投影与挂载状态一起改变，沿生产清理路径。
struct Surface {
    images: Vec<Entity<MonitorImageView>>,
    visible: Vec<bool>,
}
impl Render for Surface {
    /// 每个真实播放器各有自己的 Entity owner，不用替代缓存或伪造图元。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_row().children(
            self.images
                .iter()
                .zip(&self.visible)
                .filter(|(_, visible)| **visible)
                .map(|(image, _)| image.clone()),
        )
    }
}

/// 原生测试平台仅旁路保存弱 atlas 句柄，不替换 renderer、不延长窗口 GPU 寿命。
fn context() -> (
    HeadlessAppContext,
    Rc<RefCell<Vec<Weak<dyn PlatformAtlas>>>>,
) {
    let atlases = Rc::new(RefCell::new(Vec::new()));
    let captured = atlases.clone();
    let cx = HeadlessAppContext::with_platform(Arc::new(NoopTextSystem), Arc::new(()), move || {
        let renderer = gpui_kit::platform::current_headless_renderer()?;
        let renderer = renderer.expect("本机必须实际创建 Metal renderer");
        captured
            .borrow_mut()
            .push(Arc::downgrade(&renderer.sprite_atlas()));
        Ok(Some(renderer))
    });
    (cx, atlases)
}

/// 每个根窗口可有多个独立 owner；使用真实两帧文件与正式 Asset 解码。
fn surface(
    cx: &mut HeadlessAppContext,
    source: &MonitorImageSource,
    count: usize,
) -> (AnyWindowHandle, Entity<Surface>) {
    let source = source.clone();
    let handle = cx
        .open_window(size(px(100.), px(100.)), move |window, app| {
            app.new(|cx| {
                let images = (0..count)
                    .map(|_| {
                        cx.new(|cx| {
                            let mut image = MonitorImageView::new(
                                source.clone(),
                                ObjectFit::Contain,
                                window,
                                cx,
                            );
                            image.set_mounted(true, window, cx);
                            image
                        })
                    })
                    .collect();
                Surface {
                    images,
                    visible: vec![true; count],
                }
            })
        })
        .unwrap();
    let root = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
    (handle.into(), root)
}

/// 既有 key 返回真实 tile；缺失 builder 返回 None，不分配、不上传或改变图集。
fn tiles(
    atlas: &Weak<dyn PlatformAtlas>,
    decoded: &DecodedMonitorImage,
) -> Vec<Option<gpui_kit::AtlasTile>> {
    let atlas = atlas.upgrade().expect("活窗口必须仍有原生图集");
    (0..decoded.image.frame_count())
        .map(|frame_index| {
            atlas
                .get_or_insert_with(
                    RenderImageParams {
                        image_id: decoded.image.id,
                        frame_index,
                    }
                    .into(),
                    &mut || Ok(None),
                )
                .unwrap()
        })
        .collect()
}

/// 暖机只驱动正式调度器与真实绘制，两个帧都必须经生产播放计时上传。
fn warm(cx: &mut HeadlessAppContext, handles: &[AnyWindowHandle]) {
    for _ in 0..4 {
        for &handle in handles {
            cx.update_window(handle, |_, window, app| window.render_frame(app))
                .unwrap();
        }
        cx.run_until_parked();
    }
    for _ in 0..2 {
        cx.advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        for &handle in handles {
            cx.update_window(handle, |_, window, app| window.render_frame(app))
                .unwrap();
        }
    }
}

/// 实际投影卸载最后本窗播放器；根实体继续存活，不能借关闭窗口掩盖泄漏。
fn show(
    cx: &mut HeadlessAppContext,
    handle: AnyWindowHandle,
    root: &Entity<Surface>,
    index: usize,
    visible: bool,
) {
    cx.update_window(handle, |_, window, app| {
        root.update(app, |root, cx| {
            root.images[index].update(cx, |image, cx| image.set_mounted(visible, window, cx));
            root.visible[index] = visible;
            cx.notify();
        });
        window.draw(app).clear(app);
    })
    .unwrap();
    cx.run_until_parked();
}

/// 两窗各已上传全部帧，退出一窗逐帧清空本窗且保留另一窗真实 tile，返回后正常重建。
#[test]
fn all_metal_frames_retire_per_window_and_reupload_without_redecode() {
    let (mut cx, atlases) = context();
    let source = MonitorImageSource {
        id: uuid::Uuid::new_v4().to_string(),
        bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-loop.gif").as_slice()),
        format: loki_metris_core::ImageFormat::Gif,
    };
    let probe = LoadProbe::new(&source.id);
    let (main, main_root) = surface(&mut cx, &source, 2);
    let (pet, pet_root) = surface(&mut cx, &source, 1);
    warm(&mut cx, &[main, pet]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    assert_eq!(decoded.image.frame_count(), 2);
    let main_atlas = atlases.borrow()[0].clone();
    let pet_atlas = atlases.borrow()[1].clone();
    let main_tiles = tiles(&main_atlas, &decoded);
    let pet_tiles = tiles(&pet_atlas, &decoded);
    assert!(main_tiles.iter().all(Option::is_some));
    assert!(pet_tiles.iter().all(Option::is_some));
    show(&mut cx, main, &main_root, 0, false);
    assert_eq!(
        tiles(&main_atlas, &decoded),
        main_tiles,
        "同窗仍有第二位置，不能提前退役"
    );
    show(&mut cx, main, &main_root, 1, false);
    assert!(tiles(&main_atlas, &decoded).iter().all(Option::is_none));
    assert_eq!(tiles(&pet_atlas, &decoded), pet_tiles);
    let ready = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    assert!(Arc::ptr_eq(&ready, &decoded));
    drop(ready);
    assert_eq!(probe.started(), 1);
    show(&mut cx, main, &main_root, 0, true);
    for _ in 0..3 {
        cx.update_window(main, |_, window, app| window.draw(app).clear(app))
            .unwrap();
        cx.run_until_parked();
    }
    assert!(tiles(&main_atlas, &decoded).iter().any(Option::is_some));
    assert_eq!(probe.started(), 1);
    // 黑白 fixture 恢复时先到黑色首帧；推进真实时钟后白帧才可区分清屏底色。
    cx.advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update_window(main, |_, window, app| window.draw(app).clear(app))
        .unwrap();
    let pixels = cx
        .capture_screenshot(main)
        .expect("恢复后的真实 Metal 图元必须可读回");
    assert_eq!(
        pixels.get_pixel(pixels.width() / 2, pixels.height() / 2).0,
        [255; 4]
    );
    show(&mut cx, main, &main_root, 0, false);
    show(&mut cx, pet, &pet_root, 0, false);
    assert!(tiles(&main_atlas, &decoded).iter().all(Option::is_none));
    assert!(tiles(&pet_atlas, &decoded).iter().all(Option::is_none));
    assert!(!cx.update(|app| app.has_asset::<MonitorImageDecoder>(&source)));
    let weak = Arc::downgrade(&decoded);
    let weak_image = Arc::downgrade(&decoded.image);
    drop(decoded);
    assert!(weak.upgrade().is_none());
    assert!(weak_image.upgrade().is_none());
}
