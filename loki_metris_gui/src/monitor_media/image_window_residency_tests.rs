//! 同源图片的 CPU 缓存跨窗共享，GPU 图集必须按实际窗口需求独立退役。

use super::*;
use crate::monitor_media::{image_decode::LoadProbe, image_playback::MonitorImageView};
use gpui_kit::{
    AnyWindowHandle, AppContext, ObjectFit, TestAppContext, WindowVisibility, test::TestWindowExt,
};
use std::{io::Cursor, sync::Arc};

/// 一帧 PNG 使现有 all-frames 查询能严格证明该窗没有残留任何帧。
fn static_source() -> MonitorImageSource {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        4,
        4,
        image::Rgba([20, 180, 80, 255]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    MonitorImageSource {
        id: uuid::Uuid::new_v4().to_string(),
        bytes: Arc::from(bytes.into_inner()),
        format: loki_metris_core::ImageFormat::Png,
    }
}

/// 原生测试窗直接挂载生产播放器；不注入 decoded、frame 或缓存结果。
fn player(cx: &mut TestAppContext, source: MonitorImageSource) -> AnyWindowHandle {
    cx.add_window(move |window, cx| {
        let mut view = MonitorImageView::new(source, ObjectFit::Contain, window, cx);
        view.set_mounted(true, window, cx);
        view
    })
    .into()
}

/// 初次完成后台解码后，以完整正式绘制把真实像素上传到各窗图集。
fn warm(cx: &mut TestAppContext, handles: &[AnyWindowHandle]) {
    for _ in 0..4 {
        for &handle in handles {
            cx.update_window(handle, |_, window, app| window.render_frame(app))
                .unwrap();
        }
        cx.run_until_parked();
    }
}

/// 一窗隐藏应回收本窗全部纹理，保留另一窗 tile、共享 Arc 和原始解码次数。
#[gpui_kit::test]
fn hidden_window_releases_its_static_atlas_while_other_window_keeps_asset(cx: &mut TestAppContext) {
    let source = static_source();
    let probe = LoadProbe::new(&source.id);
    let main = player(cx, source.clone());
    let pet = player(cx, source.clone());
    warm(cx, &[main, pet]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    assert_eq!(decoded.image.frame_count(), 1);
    let tile = cx
        .update_window(pet, |_, window, _| {
            assert!(window.has_image_atlas_entry(&decoded.image));
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    cx.update_window(main, |_, window, _| {
        assert!(window.has_image_atlas_entry(&decoded.image));
    })
    .unwrap();
    cx.simulate_window_visibility_change(main, WindowVisibility::Hidden);
    cx.run_until_parked();
    cx.update_window(main, |_, window, _| {
        assert!(
            !window.has_image_atlas_entry(&decoded.image),
            "隐藏主窗不能继续保留另一窗口仍在使用图片的本窗纹理"
        );
    })
    .unwrap();
    assert!(cx.read(|app| app.has_asset::<MonitorImageDecoder>(&source)));
    let ready = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    assert!(Arc::ptr_eq(&decoded, &ready));
    assert_eq!(probe.started(), 1);
    cx.update_window(pet, |_, window, _| {
        assert!(window.has_image_atlas_entry(&decoded.image));
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile);
    })
    .unwrap();
    cx.simulate_window_visibility_change(main, WindowVisibility::Visible);
    for _ in 0..3 {
        cx.update_window(main, |_, window, app| window.draw(app).clear(app))
            .unwrap();
        cx.run_until_parked();
    }
    cx.update_window(main, |_, window, _| {
        assert!(window.has_image_atlas_entry(&decoded.image));
        assert!(!window.painted_polychrome_sprites().is_empty());
    })
    .unwrap();
    assert_eq!(probe.started(), 1, "恢复只重建本窗纹理，无须重新解码");
}

/// 同周期卸载又重新绘制时，迟到退役不能驱逐已重新获得需求的原物理图片。
#[gpui_kit::test]
fn same_cycle_reacquire_preserves_current_window_tile(cx: &mut TestAppContext) {
    let source = static_source();
    let main = player(cx, source.clone());
    let pet = player(cx, source.clone());
    warm(cx, &[main, pet]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    let tile = cx
        .update_window(main, |_, window, _| {
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    cx.update_window(main, |root, window, app| {
        let entity = root.downcast::<MonitorImageView>().unwrap();
        entity.update(app, |view, cx| {
            view.set_mounted(false, window, cx);
            view.set_mounted(true, window, cx);
        });
        window.draw(app).clear(app);
        window.draw(app).clear(app);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(main, |_, window, _| {
        assert!(window.has_image_atlas_entry(&decoded.image));
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile);
    })
    .unwrap();
}

/// 局部退役排队后最后全局位置退出，队列不能强持像素或复活已删除 Asset。
#[gpui_kit::test]
fn local_job_does_not_extend_pixels_after_last_global_owner(cx: &mut TestAppContext) {
    let source = static_source();
    let main = player(cx, source.clone());
    let pet = player(cx, source.clone());
    warm(cx, &[main, pet]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    let weak = Arc::downgrade(&decoded);
    let weak_image = Arc::downgrade(&decoded.image);
    drop(decoded);
    cx.update(|app| {
        for handle in [main, pet] {
            handle
                .update(app, |root, window, app| {
                    root.downcast::<MonitorImageView>()
                        .unwrap()
                        .update(app, |view, cx| {
                            view.set_mounted(false, window, cx);
                        });
                    window.replace_root(app, |_, _| gpui_kit::Empty);
                })
                .unwrap();
        }
        assert!(!app.has_asset::<MonitorImageDecoder>(&source));
        assert!(
            weak.upgrade().is_none(),
            "排队局部清理不应增加任何像素强引用"
        );
        assert!(
            weak_image.upgrade().is_none(),
            "图像帧也必须立即释放，不能只释放解码外壳"
        );
    });
    cx.run_until_parked();
    assert!(!cx.read(|app| app.has_asset::<MonitorImageDecoder>(&source)));
    assert!(weak.upgrade().is_none());
    assert!(weak_image.upgrade().is_none());
}

/// 回调前原窗已关闭时，只结束旧清理，不误删另一原生窗口仍在使用的图元。
#[gpui_kit::test]
fn closing_window_before_local_job_preserves_other_window(cx: &mut TestAppContext) {
    let source = static_source();
    let main = player(cx, source.clone());
    let pet = player(cx, source.clone());
    warm(cx, &[main, pet]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    let tile = cx
        .update_window(pet, |_, window, _| {
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    cx.update_window(main, |root, window, app| {
        root.downcast::<MonitorImageView>()
            .unwrap()
            .update(app, |view, cx| {
                view.set_mounted(false, window, cx);
            });
        window.remove_window();
    })
    .unwrap();
    cx.run_until_parked();
    assert!(cx.update_window(main, |_, _, _| ()).is_err());
    cx.update_window(pet, |_, window, _| {
        assert!(window.has_image_atlas_entry(&decoded.image));
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile);
    })
    .unwrap();
}

/// 释放根实体但保留原窗时，也要局部释放纹理，不能等待另一个窗口退出。
#[gpui_kit::test]
fn entity_release_cleans_only_its_live_window(cx: &mut TestAppContext) {
    let source = static_source();
    let main = player(cx, source.clone());
    let pet = player(cx, source.clone());
    warm(cx, &[main, pet]);
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    cx.update_window(main, |_, window, app| {
        window.replace_root(app, |_, _| gpui_kit::Empty);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(main, |_, window, _| {
        assert!(!window.has_image_atlas_entry(&decoded.image))
    })
    .unwrap();
    cx.update_window(pet, |_, window, _| {
        assert!(window.has_image_atlas_entry(&decoded.image))
    })
    .unwrap();
    assert!(cx.read(|app| app.has_asset::<MonitorImageDecoder>(&source)));
}

/// 最新回执不能覆盖仍在绘制的物理身份；不同平台沿各自的纹理生命周期。
#[gpui_kit::test]
fn cleanup_respects_current_paint_even_after_new_decode_receipt(cx: &mut TestAppContext) {
    let source = static_source();
    let next_source = static_source();
    let old_window = player(cx, source.clone());
    let new_window = player(cx, next_source.clone());
    warm(cx, &[old_window, new_window]);
    let old = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    let next = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&next_source)
            .unwrap()
            .unwrap()
    });
    assert_ne!(old.image.id, next.image.id);
    cx.update_window(old_window, |_, window, app| {
        assert!(window.has_image_atlas_entry(&old.image));
        let retired = WindowRetirement {
            source_id: source.id.clone(),
            window: window.window_handle().window_id(),
            image_id: old.image.id,
            decoded: Arc::downgrade(&old),
        };
        // 此处只模拟回执代数交错；像素、图集和两个解码结果仍来自正式 Asset 路径。
        remember(&source, &Ok(next.clone()), app);
        retired.cleanup(window, app);
        #[cfg(target_os = "macos")]
        assert!(
            window.has_image_atlas_entry(&old.image),
            "实际 paint claim 仍持有旧图，最新回执不能越过精确帧并集"
        );
        #[cfg(not(target_os = "macos"))]
        assert!(!window.has_image_atlas_entry(&old.image));
        assert!(
            app.has_asset::<MonitorImageDecoder>(&source),
            "局部退役不能删除 CPU Asset"
        );
    })
    .unwrap();
    cx.update_window(new_window, |_, window, _| {
        assert!(window.has_image_atlas_entry(&next.image));
    })
    .unwrap();
}
