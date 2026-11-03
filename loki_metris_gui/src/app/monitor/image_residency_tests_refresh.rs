// 本文件是 image_residency_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：显式刷新只重试失败结果并通知另一实际 owner，预算释放后两窗都绘制同一正常结果。；健康共享结果在一窗显式刷新期间保持同 Arc 和另一窗的真实 tile。；已共享的 pending 任务在刷新期间保留；同源文件查询回执不能取消另一窗仍在等待的任务。

/// 显式刷新只重试失败结果并通知另一实际 owner，预算释放后两窗都绘制同一正常结果。
#[gpui_kit::test]
fn refresh_retries_real_budget_error_and_notifies_other_visible_owner(cx: &mut TestAppContext) {
    let budget = cx.update(hold_pixel_budget_for_test);
    let f = windows(cx);
    draw(cx, &[f.main, f.playback]);
    assert!(cx.update(|app| {
        app.fetch_asset::<MonitorImageDecoder>(&f.source)
            .unwrap()
            .is_err()
    }));
    let notices = Rc::new(Cell::new(0));
    let count = notices.clone();
    let _notice = cx.update(|app| app.observe(&f.image, move |_, _| count.set(count.get() + 1)));
    drop(budget);
    cx.update_window(f.main, |_, window, app| {
        window.click("monitor-images-refresh", app)
    })
    .unwrap();
    assert!(
        notices.get() > 0,
        "刷新失败 cache 必须通知另一实际 owner 重试"
    );
    settle_query(&f, cx);
    draw(cx, &[f.main, f.playback]);
    let decoded = ready(&f, cx);
    cx.update_window(f.main, |_, window, app| {
        assert!(Arc::ptr_eq(
            &decoded,
            &window
                .get_asset::<MonitorImageDecoder>(&f.source, app)
                .unwrap()
                .unwrap()
        ))
    })
    .unwrap();
    cx.update_window(f.playback, |_, window, _| {
        assert!(!window.painted_polychrome_sprites().is_empty())
    })
    .unwrap();
}

/// 单帧健康共享结果跨异步刷新保持同 Arc 和真实 tile，不把动画合法换帧当作资源驱逐。
#[gpui_kit::test]
fn refresh_preserves_healthy_other_window_arc_and_tile(cx: &mut TestAppContext) {
    let mut gif = Vec::new();
    image::codecs::gif::GifEncoder::new(&mut gif)
        .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(3, 3, image::Rgba([0, 0, 0, 255]))))
        .unwrap();
    let f = windows_with_gif(cx, &gif);
    draw(cx, &[f.main, f.playback]);
    let decoded = ready(&f, cx);
    assert_eq!(decoded.image.frame_count(), 1, "tile稳定性场景必须与动画帧身份独立");
    let tile = cx
        .update_window(f.playback, |_, window, _| {
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    // 超过原双帧夹具的200ms边界，仍严格核对同一静态帧的真实图集身份。
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.update_window(f.main, |_, window, app| {
        window.click("monitor-images-refresh", app)
    })
    .unwrap();
    assert!(cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    assert!(
        Arc::ptr_eq(&decoded, &ready(&f, cx)),
        "健康的另一 owner 不应因刷新重新解码"
    );
    settle_query(&f, cx);
    draw(cx, &[f.main, f.playback]);
    cx.update_window(f.playback, |_, window, _| {
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile)
    })
    .unwrap();
}

/// 已共享的 pending 任务在刷新期间保留；同源文件查询回执不能取消另一窗仍在等待的任务。
#[gpui_kit::test]
fn refresh_preserves_other_window_pending_asset(cx: &mut TestAppContext) {
    let permits = cx.update(hold_decoders_for_test);
    let f = windows(cx);
    draw(cx, &[f.main, f.playback]);
    assert_eq!(f.probe.started(), 1);
    cx.update_window(f.main, |_, window, app| {
        window.click("monitor-images-refresh", app)
    })
    .unwrap();
    let survived = cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source));
    let started = f.probe.started();
    drop(permits);
    cx.run_until_parked();
    assert!(survived, "刷新不能取消另一窗的 pending 任务");
    assert_eq!(started, 1, "pending 刷新不能取消并立即重建实际 Asset 任务");
    settle_query(&f, cx);
    draw(cx, &[f.main, f.playback]);
    let _decoded = ready(&f, cx);
    assert_eq!(f.probe.started(), 1);
}

/// 源文件读取失败后恢复仍经正式 query 回执换 entry，不能被旧错误投影遮蔽。
#[gpui_kit::test]
fn source_read_error_repair_uses_formal_query_receipt_and_actual_paint(cx: &mut TestAppContext) {
    let f = windows(cx);
    let directory = f.root.path().join("data/monitor-images");
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("index.json")).unwrap()).unwrap();
    let stored = directory.join(index[0]["storedName"].as_str().unwrap());
    std::fs::write(&stored, b"damaged fixture").unwrap();
    f.shell.update(cx, |this, cx| this.load_monitor_images(cx));
    settle_query(&f, cx);
    assert!(
        f.shell
            .read_with(cx, |this, _| this.monitor.catalog.images[0].source.is_err())
    );
    cx.update_window(f.main, |_, window, app| window.render_frame(app))
        .unwrap();
    std::fs::write(
        &stored,
        include_bytes!("../../../tests/fixtures/monitor-loop.gif"),
    )
    .unwrap();
    cx.update_window(f.main, |_, window, app| {
        window.click("monitor-images-refresh", app)
    })
    .unwrap();
    settle_query(&f, cx);
    assert!(
        f.shell
            .read_with(cx, |this, _| this.monitor.catalog.images[0].source.is_ok())
    );
    draw(cx, &[f.main, f.playback]);
    let _decoded = ready(&f, cx);
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
