// 本文件是 image_window_metal_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：正常换帧不能强制刷新无关缓存；终止动画的旧 Scene 必须持续使用有效 tile。

/// 正常换帧不能强制刷新无关缓存；终止动画的旧 Scene 必须持续使用有效 tile。
#[test]
fn moving_sibling_preserves_cached_terminal_frame_and_lifecycle_refresh() {
    let (mut cx, atlases) = context();
    let source = MonitorImageSource {
        id: uuid::Uuid::new_v4().to_string(),
        bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-finite.png").as_slice()),
        format: loki_metris_core::ImageFormat::Png,
    };
    let prepaint = Rc::new(std::cell::Cell::new(0));
    let paint = Rc::new(std::cell::Cell::new(0));
    let captured = (prepaint.clone(), paint.clone());
    let player_source = source.clone();
    let handle = cx
        .open_window(size(px(100.), px(100.)), move |window, app| {
            app.new(|cx| {
                let first = cx.new(|cx| {
                    let mut player = MonitorImageView::new(
                        player_source.clone(),
                        ObjectFit::Contain,
                        window,
                        cx,
                    );
                    player.set_mounted(true, window, cx);
                    player
                });
                let probe = cx.new(|_| CachedProbe {
                    player: first,
                    prepaint,
                    paint,
                });
                let moving = cx
                    .new(|cx| MonitorImageView::new(player_source, ObjectFit::Contain, window, cx));
                CachedSurface {
                    probe,
                    moving,
                    visible: false,
                }
            })
        })
        .unwrap();
    let root = handle.update(&mut cx, |_, _, cx| cx.entity()).unwrap();
    let handle: AnyWindowHandle = handle.into();
    warm(&mut cx, &[handle]);
    cx.advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| window.draw(app).clear(app))
        .unwrap();
    let decoded = cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&source)
            .unwrap()
            .unwrap()
    });
    assert_eq!(decoded.plays, Some(2));
    let atlas = atlases.borrow()[0].clone();
    let terminal = tiles(&atlas, &decoded)[1].expect("第一位置必须自然停在白色末帧");
    let counts = (captured.0.get(), captured.1.get());
    assert!(counts.0 > 0 && counts.1 > 0);
    cx.update_window(handle, |_, window, app| {
        root.update(app, |root, cx| {
            root.moving
                .update(cx, |view, cx| view.set_mounted(true, window, cx));
            root.visible = true;
            cx.notify();
        });
        window.draw(app).clear(app);
    })
    .unwrap();
    cx.run_until_parked();
    for _ in 0..8 {
        cx.update_window(handle, |_, window, app| {
            window.draw(app).clear(app);
            assert!(
                window
                    .painted_polychrome_sprites()
                    .iter()
                    .any(|sprite| sprite.tile == terminal)
            );
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(tiles(&atlas, &decoded)[1], Some(terminal));
        assert_eq!(
            (captured.0.get(), captured.1.get()),
            counts,
            "移动位置的正常换帧不得使无关 cached sibling 重绘"
        );
        cx.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    cx.update_window(handle, |_, window, app| window.draw(app).clear(app))
        .unwrap();
    assert_eq!(
        tiles(&atlas, &decoded)
            .iter()
            .filter(|tile| tile.is_some())
            .count(),
        1,
        "两个位置最终共享末帧"
    );
    cx.update_window(handle, |_, window, app| {
        root.update(app, |root, cx| {
            root.moving
                .update(cx, |view, cx| view.set_mounted(false, window, cx));
            root.visible = false;
            cx.notify();
        });
        window.draw(app).clear(app);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| window.draw(app).clear(app))
        .unwrap();
    assert_eq!(
        tiles(&atlas, &decoded)[1],
        Some(terminal),
        "另一位置仍持末帧，不能删 key"
    );
    assert!(
        captured.0.get() > counts.0 && captured.1.get() > counts.1,
        "即使保留同帧，生命周期撤销也必须失效旧 Scene"
    );
    let pixels = cx.capture_screenshot(handle).unwrap();
    assert_eq!(
        pixels.get_pixel(pixels.width() / 4, pixels.height() / 2).0,
        [255; 4]
    );
}
