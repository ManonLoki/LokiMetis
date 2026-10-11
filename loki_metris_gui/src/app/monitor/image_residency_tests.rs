//! 实际图库需求画布与独立播放器窗口共享同一 Asset，检查 CPU、GPU 和关闭时序。

use super::super::{
    Shell,
    state::{ImageEntry, Route},
};
use crate::monitor::{
    image_decode::{
        DecodedMonitorImage, LoadProbe, MonitorImageDecoder, MonitorImageSource,
        hold_decoders_for_test, hold_pixel_budget_for_test,
    },
    image_playback::MonitorImageView,
};
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Entity, IntoElement, ObjectFit, Render, ScrollDelta,
    TestAppContext, Window, WindowVisibility, div, point, prelude::*, px, test::TestWindowExt,
};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

/// 只有实际播放器参与绘制；隐藏投影不代理任何资源清理逻辑。
struct PlaybackWindow {
    image: Entity<MonitorImageView>,
    shown: bool,
}
impl Render for PlaybackWindow {
    /// 图元来源保持生产播放器，空投影只模拟真实位置卸载。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when(self.shown, |div| div.child(self.image.clone()))
            .into_any_element()
    }
}

/// 所有文件、服务与偏好由当前测试拥有；窗口关闭测试会额外保留播放器强引用。
struct Windows {
    root: tempfile::TempDir,
    service: Arc<crate::usage_service::UsageService>,
    main: AnyWindowHandle,
    shell: Entity<Shell>,
    playback: AnyWindowHandle,
    playback_root: Entity<PlaybackWindow>,
    image: Entity<MonitorImageView>,
    source: MonitorImageSource,
    probe: LoadProbe,
    _writer: crate::app::tests::FinishWriter,
}
impl Drop for Windows {
    /// 失败断言也必须回收本测试独占的后台服务。
    fn drop(&mut self) {
        self.service.finish();
    }
}

/// 正式导入回执供给真实 Shell 图库和另一原生窗口；不修改缓存、解码或资源私有状态。
fn windows(cx: &mut TestAppContext) -> Windows {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("shared.gif");
    std::fs::write(
        &path,
        include_bytes!("../../../tests/fixtures/monitor-loop.gif"),
    )
    .unwrap();
    let service =
        crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let imported = service
        .import_monitor_image(path)
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    let source = MonitorImageSource {
        id: imported.id.clone(),
        bytes: imported.snapshot.bytes[&imported.id].clone(),
        format: loki_metris_core::ImageFormat::Gif,
    };
    let probe = LoadProbe::new(&source.id);
    let (main, shell, writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Images;
        this.dashboard.service = Some(service.clone());
        this.monitor.catalog.accept_images(imported.snapshot);
        cx.notify();
    });
    let player_source = source.clone();
    let playback = cx.add_window(move |window, cx| {
        let image =
            cx.new(|cx| MonitorImageView::new(player_source, ObjectFit::Contain, window, cx));
        image.update(cx, |image, cx| image.set_mounted(true, window, cx));
        PlaybackWindow { image, shown: true }
    });
    let playback_root = playback.update(cx, |_, _, cx| cx.entity()).unwrap();
    let image = playback_root.read_with(cx, |this, _| this.image.clone());
    Windows {
        root,
        service,
        main,
        shell,
        playback: playback.into(),
        playback_root,
        image,
        source,
        probe,
        _writer: writer,
    }
}

/// 每帧都经过完整生产 render/prepaint/paint，资源后台工作由真实测试调度器推进。
fn draw(cx: &mut TestAppContext, handles: &[AnyWindowHandle]) {
    for _ in 0..4 {
        for &handle in handles {
            cx.update_window(handle, |_, window, app| window.render_frame(app))
                .unwrap();
        }
        cx.run_until_parked();
    }
}

/// 对已经完成的缓存取真实 Arc；调用前必须另外断言缓存仍存在，避免 peek 启动新解码。
fn ready(f: &Windows, cx: &mut TestAppContext) -> Arc<DecodedMonitorImage> {
    assert!(cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    cx.update_window(f.playback, |_, window, app| {
        window
            .get_asset::<MonitorImageDecoder>(&f.source, app)
            .expect("共享资源必须仍为 Ready")
            .unwrap()
    })
    .unwrap()
}

/// 实际位置卸载先通知生产播放器，再移除它的真实视图投影。
fn unmount_player(f: &Windows, cx: &mut TestAppContext) {
    cx.update_window(f.playback, |_, window, app| {
        f.image
            .update(app, |image, cx| image.set_mounted(false, window, cx));
        f.playback_root.update(app, |this, cx| {
            this.shown = false;
            cx.notify();
        });
        window.render_frame(app);
    })
    .unwrap();
}

/// 服务回执只在正式 query 完成后判定成功，超时保留失败而非绕过异步路径。
fn settle_query(f: &Windows, cx: &mut TestAppContext) {
    for _ in 0..500 {
        cx.run_until_parked();
        if f.shell
            .read_with(cx, |this, _| !this.monitor.catalog.images_loading)
        {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("正式图库 query 回执必须有界完成");
}

/// 一个窗口隐藏不能清另一窗口的同 Arc；最后 owner 离开须清两窗所有帧并释放像素。
#[gpui_kit::test]
fn shared_ready_survives_one_window_hidden_and_final_owner_cleans_all_atlases(
    cx: &mut TestAppContext,
) {
    let f = windows(cx);
    draw(cx, &[f.main, f.playback]);
    let decoded = ready(&f, cx);
    let weak = Arc::downgrade(&decoded);
    let tile = cx
        .update_window(f.playback, |_, window, _| {
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    cx.simulate_window_visibility_change(f.main, WindowVisibility::Hidden);
    assert!(
        cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)),
        "隐藏图库不能驱逐另一窗口正在使用的 Asset"
    );
    assert!(Arc::ptr_eq(&decoded, &ready(&f, cx)));
    draw(cx, &[f.playback]);
    cx.update_window(f.playback, |_, window, _| {
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile)
    })
    .unwrap();
    unmount_player(&f, cx);
    assert!(!cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    for handle in [f.main, f.playback] {
        cx.update_window(handle, |_, window, _| {
            assert!(
                !window.has_image_atlas_entry(&decoded.image),
                "最后 owner 必须回收每个仍存活窗口的图集"
            )
        })
        .unwrap();
    }
    drop(decoded);
    assert!(
        weak.upgrade().is_none(),
        "隐藏窗口和离屏 view 不能残留像素 owner"
    );
}

/// 真实等待许可的 pending Asset 已被两个窗口使用，隐藏一方不能取消另一方的解码任务。
#[gpui_kit::test]
fn shared_pending_survives_gallery_hide_and_remaining_window_actually_paints(
    cx: &mut TestAppContext,
) {
    let permits = cx.update(hold_decoders_for_test);
    let f = windows(cx);
    draw(cx, &[f.main, f.playback]);
    assert_eq!(f.probe.started(), 1);
    assert!(cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    cx.simulate_window_visibility_change(f.main, WindowVisibility::Hidden);
    let survived = cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source));
    drop(permits);
    cx.run_until_parked();
    assert!(survived, "另一窗口的 pending Asset 不能被图库取消");
    draw(cx, &[f.playback]);
    let decoded = ready(&f, cx);
    assert_eq!(f.probe.started(), 1, "另一 owner 必须完成原始 pending 任务");
    cx.update_window(f.playback, |_, window, _| {
        assert!(!window.painted_polychrome_sprites().is_empty())
    })
    .unwrap();
    let weak = Arc::downgrade(&decoded);
    unmount_player(&f, cx);
    assert!(!cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    drop(decoded);
    assert!(weak.upgrade().is_none());
}

/// 关闭窗口时外部仍强持播放器，关闭 owner 也必须退役，最后活跃窗口离开后弱像素失效。
#[gpui_kit::test]
fn closed_window_retires_owner_and_pixels_even_when_view_entity_is_retained(
    cx: &mut TestAppContext,
) {
    let f = windows(cx);
    draw(cx, &[f.main, f.playback]);
    let decoded = ready(&f, cx);
    let weak = Arc::downgrade(&decoded);
    cx.update_window(f.playback, |_, window, _| window.remove_window())
        .unwrap();
    assert!(cx.update_window(f.playback, |_, _, _| ()).is_err());
    assert!(
        cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)),
        "关闭一窗不能取消活跃图库"
    );
    cx.simulate_window_visibility_change(f.main, WindowVisibility::Hidden);
    assert!(!cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    drop(decoded);
    assert!(
        weak.upgrade().is_none(),
        "被外部保留的已关闭播放器必须释放 decoded"
    );
    assert!(!f.image.read_with(cx, |view, _| view.is_decoded()));
}

/// 新 entry 已由真实需求画布取得资源但尚未创建子播放器时，旧 entry 的迟到释放不得误删它。
#[gpui_kit::test]
fn retired_entry_release_cannot_remove_new_same_id_entry_owner(cx: &mut TestAppContext) {
    let f = windows(cx);
    cx.simulate_window_visibility_change(f.playback, WindowVisibility::Hidden);
    draw(cx, &[f.main]);
    let old = f.shell.update(cx, |this, cx| {
        let old = this.monitor.catalog.images.remove(0);
        this.monitor.catalog.images.push(ImageEntry {
            id: f.source.id.clone(),
            filename: "new.gif".into(),
            format: f.source.format,
            source: Ok(f.source.clone()),
            views: Default::default(),
            residency: Default::default(),
        });
        assert!(!Rc::ptr_eq(
            &old.residency,
            &this.monitor.catalog.images[0].residency
        ));
        cx.notify();
        old
    });
    cx.update_window(f.main, |_, window, app| {
        window.render_frame(app);
        super::release_image(&old, window, app);
        assert!(
            app.has_asset::<MonitorImageDecoder>(&f.source),
            "迟到旧 entry release 不能退役新 entry 的唯一需求 owner"
        );
        assert!(
            window
                .get_asset::<MonitorImageDecoder>(&f.source, app)
                .is_some()
        );
    })
    .unwrap();
    draw(cx, &[f.main]);
    cx.update_window(f.main, |_, window, _| {
        let viewport = window
            .find(format!("monitor-gallery-img-{}", f.source.id))
            .bounds()
            .scale(window.scale_factor());
        assert!(
            window
                .painted_polychrome_sprites()
                .iter()
                .any(|sprite| sprite.rounded_clip_bounds == viewport)
        );
    })
    .unwrap();
}

/// 真实滚动已排队旧图库离屏清理，另一窗口在同一更新内开始绘制后不能被旧 defer 驱逐。
#[gpui_kit::test]
fn queued_offscreen_cleanup_rechecks_new_other_window_owner(cx: &mut TestAppContext) {
    let f = windows(cx);
    unmount_player(&f, cx);
    f.shell.update(cx, |this, cx| {
        for index in 0..20 {
            this.monitor.catalog.images.push(ImageEntry {
                id: format!("unavailable-{index}"),
                filename: "unavailable.gif".into(),
                format: f.source.format,
                source: Err(loki_metris_core::HookError::new(
                    "error.monitor.imagesReadFailed",
                )),
                views: Default::default(),
                residency: Default::default(),
            });
        }
        cx.notify();
    });
    draw(cx, &[f.main]);
    let decoded = ready(&f, cx);
    cx.update_window(f.main, |_, window, app| {
        window.scroll(
            "page-content",
            ScrollDelta::Pixels(point(px(0.), px(-2200.))),
            app,
        );
        let first = window
            .find(format!("monitor-gallery-img-{}", f.source.id))
            .bounds();
        assert!(
            first.bottom() <= window.find("page-content").bounds().top(),
            "必须实际滚出首幅图片"
        );
        f.playback
            .update(app, |_, other, app| {
                f.image
                    .update(app, |image, cx| image.set_mounted(true, other, cx));
                f.playback_root.update(app, |this, cx| {
                    this.shown = true;
                    cx.notify();
                });
                other.render_frame(app);
                other.render_frame(app);
            })
            .unwrap();
    })
    .unwrap();
    assert!(
        cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)),
        "旧图库 defer 不能删除后来出现的另一窗口 owner"
    );
    assert!(Arc::ptr_eq(&decoded, &ready(&f, cx)));
    cx.update_window(f.playback, |_, window, _| {
        assert!(!window.painted_polychrome_sprites().is_empty())
    })
    .unwrap();
}

/// 真实根 view 被替换但窗口仍存活，纯 Entity release 必须回收最后 owner 的 CPU 与 GPU。
#[gpui_kit::test]
fn released_entity_cleans_last_asset_while_its_window_remains_open(cx: &mut TestAppContext) {
    let f = windows(cx);
    cx.simulate_window_visibility_change(f.main, WindowVisibility::Hidden);
    cx.simulate_window_visibility_change(f.playback, WindowVisibility::Hidden);
    let source = f.source.clone();
    let handle = cx.add_window(move |window, cx| {
        let mut view = MonitorImageView::new(source, ObjectFit::Contain, window, cx);
        view.set_mounted(true, window, cx);
        view
    });
    let old = handle
        .update(cx, |_, _, cx| cx.entity().downgrade())
        .unwrap();
    draw(cx, &[handle.into()]);
    let decoded = ready(&f, cx);
    let weak = Arc::downgrade(&decoded);
    cx.update_window(handle.into(), |_, window, app| {
        window.replace_root(app, |_, _| gpui_kit::Empty);
        window.render_frame(app);
    })
    .unwrap();
    draw(cx, &[handle.into()]);
    assert!(
        old.upgrade().is_none(),
        "旧根 Entity 必须已真实释放，不能只检查 replace_root 调用"
    );
    assert!(
        !cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)),
        "已释放 Entity 不应等窗口关闭才归还 Asset"
    );
    cx.update_window(handle.into(), |_, window, _| {
        assert!(!window.has_image_atlas_entry(&decoded.image))
    })
    .unwrap();
    drop(decoded);
    assert!(weak.upgrade().is_none());
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - image_residency_tests_more.rs：显式刷新只重试失败结果并通知另一实际 owner，预算释放后两窗都绘制同一正常结果。；健康共享结果在一窗显式刷新期间保持同 Arc 和另一窗的真实 tile。；已共享的 pending 任务在刷新期间保留；同源文件查询回执不能取消另一窗仍在等待的任务。
include!("image_residency_tests_more.rs");
