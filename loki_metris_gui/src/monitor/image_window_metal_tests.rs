//! 原生 Metal 图集逐帧查询，证明工作集有界、局部退役和跨窗继续绘制。

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
    /// 渲染一个会统计 prepaint / paint 次数的缓存视图，用来证明缓存场景有没有被重复绘制。
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

/// 暖机只驱动正式调度器与真实绘制，跨过播放边界而不直接注入帧。
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

/// 两窗各保留当前绘制帧，退出一窗清空本窗且保留另一窗真实 tile，返回后正常重建。
#[test]
fn resident_metal_frames_retire_per_window_and_reupload_without_redecode() {
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
    assert_eq!(main_tiles.iter().filter(|tile| tile.is_some()).count(), 1);
    assert_eq!(pet_tiles.iter().filter(|tile| tile.is_some()).count(), 1);
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

/// 十二帧实际动画经历多个边界后，图集只保留该位置最后成功绘制的一帧。
#[test]
fn image_frame_working_set_keeps_only_current_painted_frame() {
    let (mut cx, atlases) = context();
    let source = MonitorImageSource {
        id: uuid::Uuid::new_v4().to_string(),
        bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-working-set.png").as_slice()),
        format: loki_metris_core::ImageFormat::Png,
    };
    let probe = LoadProbe::new(&source.id);
    let (handle, root) = surface(&mut cx, &source, 1);
    warm(&mut cx, &[handle]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    assert_eq!(decoded.image.frame_count(), 12);
    let atlas = atlases.borrow()[0].clone();
    let resident = tiles(&atlas, &decoded);
    cx.update_window(handle, |_, window, _| {
        for (frame_index, tile) in resident.iter().enumerate() {
            assert_eq!(
                window.has_image_frame_atlas_entry(RenderImageParams {
                    image_id: decoded.image.id,
                    frame_index,
                }),
                tile.is_some(),
                "测试查询必须同时适用真实 Metal 的存在与缺失帧"
            );
        }
    })
    .unwrap();
    assert_eq!(
        resident.iter().filter(|tile| tile.is_some()).count(),
        1,
        "经历多个真实播放边界后不能累积所有已上传帧"
    );
    for _ in 0..30 {
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            tiles(&atlas, &decoded)
                .iter()
                .filter(|tile| tile.is_some())
                .count(),
            1
        );
    }
    assert_eq!(probe.started(), 1);
    show(&mut cx, handle, &root, 0, false);
    assert!(tiles(&atlas, &decoded).iter().all(Option::is_none));
}

/// 同窗错相位置保留帧并集；图库继续持 CPU 结果也不能固定已卸载播放器的 GPU 页。
#[test]
fn phased_players_keep_union_then_release_frames_while_catalog_keeps_decode() {
    let (mut cx, atlases) = context();
    let source = MonitorImageSource {
        id: uuid::Uuid::new_v4().to_string(),
        bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-working-set.png").as_slice()),
        format: loki_metris_core::ImageFormat::Png,
    };
    let probe = LoadProbe::new(&source.id);
    let (handle, root) = surface(&mut cx, &source, 2);
    show(&mut cx, handle, &root, 1, false);
    warm(&mut cx, &[handle]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    let atlas = atlases.borrow()[0].clone();
    assert_eq!(
        tiles(&atlas, &decoded)
            .iter()
            .filter(|tile| tile.is_some())
            .count(),
        1
    );
    show(&mut cx, handle, &root, 1, true);
    for _ in 0..3 {
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .unwrap();
        cx.run_until_parked();
    }
    for _ in 0..20 {
        assert_eq!(
            tiles(&atlas, &decoded)
                .iter()
                .filter(|tile| tile.is_some())
                .count(),
            2,
            "两个错相实际位置必须各保留正在绘制的一帧"
        );
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .unwrap();
        cx.run_until_parked();
    }
    let entry = Rc::new(());
    cx.update_window(handle, |_, window, app| {
        let owner = root.update(app, |_, cx| ImageOwner::catalog(&entry, window, cx));
        acquire(&source, &owner, app);
    })
    .unwrap();
    show(&mut cx, handle, &root, 0, false);
    assert_eq!(
        tiles(&atlas, &decoded)
            .iter()
            .filter(|tile| tile.is_some())
            .count(),
        1
    );
    show(&mut cx, handle, &root, 1, false);
    assert!(tiles(&atlas, &decoded).iter().all(Option::is_none));
    let ready = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    assert!(Arc::ptr_eq(&ready, &decoded));
    assert_eq!(probe.started(), 1);
    drop(ready);
    cx.update_window(handle, |_, window, app| {
        release_catalog(&source.id, &entry, window, app)
    })
    .unwrap();
    assert!(!cx.update(|app| app.has_asset::<MonitorImageDecoder>(&source)));
    let weak = Arc::downgrade(&decoded);
    drop(decoded);
    assert!(weak.upgrade().is_none());
}

/// 独立 cached sibling 的真实 prepaint/paint 计数，不以 Render 计数替代缓存重放。
struct CachedProbe {
    player: Entity<MonitorImageView>,
    prepaint: Rc<std::cell::Cell<usize>>,
    paint: Rc<std::cell::Cell<usize>>,
}
impl Render for CachedProbe {
    /// 按 visible 决定是否把正在移动的兄弟视图放进缓存面，用来验证换帧时旧缓存场景仍然有效。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let prepaint = self.prepaint.clone();
        let paint = self.paint.clone();
        div()
            .relative()
            .size_full()
            .child(self.player.clone())
            .child(
                gpui_kit::canvas(
                    move |_, _, _| prepaint.set(prepaint.get() + 1),
                    move |_, (), _, _| paint.set(paint.get() + 1),
                )
                .absolute()
                .inset_0(),
            )
    }
}
struct CachedSurface {
    probe: Entity<CachedProbe>,
    moving: Entity<MonitorImageView>,
    visible: bool,
}
impl Render for CachedSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let sibling = if self.visible {
            self.moving.clone().into_any_element()
        } else {
            div().size_full().into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_row()
            .child(
                self.probe
                    .clone()
                    .cached(gpui_kit::StyleRefinement::default().w(px(50.)).h(px(100.))),
            )
            .child(div().w(px(50.)).h(px(100.)).child(sibling))
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - image_window_metal_tests_more.rs：正常换帧不能强制刷新无关缓存；终止动画的旧 Scene 必须持续使用有效 tile。
include!("image_window_metal_tests_more.rs");
