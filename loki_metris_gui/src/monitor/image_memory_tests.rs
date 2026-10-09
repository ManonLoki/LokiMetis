//! 独立真实 App 的图片解码不能共享测试 scheduler 的等待者或像素占位预算。

use super::super::{
    image_decode::{
        DecodedMonitorImage, MonitorImageDecoder, MonitorImageSource, hold_decoders_for_test,
        hold_pixel_budget_for_test,
    },
    image_playback::MonitorImageView,
    image_residency,
};
use gpui_kit::{AnyWindowHandle, AppContext, ObjectFit, TestAppContext, test::TestWindowExt};
use std::{sync::Arc, time::Duration};

/// 每次使用独立 ID 和真实两帧文件，缓存、许可与完成通知均沿生产 Asset 路径。
fn source() -> MonitorImageSource {
    MonitorImageSource {
        id: uuid::Uuid::new_v4().to_string(),
        bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-loop.gif").as_slice()),
        format: loki_metris_core::ImageFormat::Gif,
    }
}

/// 创建真实播放器根；挂载、画布需求和 decoded 回执不使用替代结果。
fn player(cx: &mut TestAppContext, source: MonitorImageSource) -> AnyWindowHandle {
    cx.add_window(move |window, cx| {
        let mut view = MonitorImageView::new(source, ObjectFit::Contain, window, cx);
        view.set_mounted(true, window, cx);
        view
    })
    .into()
}

/// 完成实际 prepaint、后台解码与下一帧投影；不等待时钟或注入缓存结果。
fn draw(cx: &mut TestAppContext, window: AnyWindowHandle) {
    for _ in 0..3 {
        cx.update_window(window, |_, window, app| window.render_frame(app))
            .unwrap();
        cx.run_until_parked();
    }
}

/// 读回真正 Asset 结果及图元，防止只证明另一个 App 没有抛出错误。
fn painted(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    source: &MonitorImageSource,
) -> Arc<DecodedMonitorImage> {
    let decoded = cx
        .update(|app| app.fetch_asset::<MonitorImageDecoder>(source))
        .expect("另一个 App 的真实 Asset 必须完成，不能被外部许可占位阻塞")
        .expect("另一个 App 的真实图片必须解码成功，不能消费外部像素占位预算");
    assert_eq!(decoded.image.frame_count(), 2);
    // 图集查询要求所有帧已上传；实际推进两次边界后回到初帧，不绕过播放任务。
    for _ in 0..2 {
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        cx.update_window(window, |_, window, app| window.render_frame(app))
            .unwrap();
        cx.run_until_parked();
    }
    cx.update_window(window, |_, window, _| {
        assert!(!window.painted_polychrome_sprites().is_empty());
        assert!(window.has_image_atlas_entry(&decoded.image));
    })
    .unwrap();
    decoded
}

/// A 的两个真实许可均被占满时，B 仍解码绘制；A 退出也不能取消或驱逐 B。
#[gpui_kit::test]
fn full_decoder_permits_are_isolated_between_actual_apps(cx: &mut TestAppContext) {
    let mut blocked = cx.new_app();
    let permits = blocked.update(hold_decoders_for_test);
    let blocked_source = source();
    let blocked_window = player(&mut blocked, blocked_source.clone());
    draw(&mut blocked, blocked_window);
    assert!(
        blocked
            .update(|app| app.fetch_asset::<MonitorImageDecoder>(&blocked_source))
            .is_none()
    );

    let other_source = source();
    let other_window = player(cx, other_source.clone());
    draw(cx, other_window);
    let decoded = painted(cx, other_window, &other_source);
    assert!(
        blocked
            .update(|app| app.fetch_asset::<MonitorImageDecoder>(&blocked_source))
            .is_none()
    );
    drop(blocked);
    drop(permits);
    cx.run_until_parked();
    draw(cx, other_window);
    assert!(Arc::ptr_eq(
        &decoded,
        &painted(cx, other_window, &other_source)
    ));
}

/// A 的整额预算使 A 正式失败且可重试，B 同时成功并保持原 Arc 和 atlas。
#[gpui_kit::test]
fn full_pixel_budget_is_isolated_between_actual_apps(cx: &mut TestAppContext) {
    let mut exhausted = cx.new_app();
    let budget = exhausted.update(hold_pixel_budget_for_test);
    let exhausted_source = source();
    let exhausted_window = player(&mut exhausted, exhausted_source.clone());
    draw(&mut exhausted, exhausted_window);
    assert!(
        exhausted
            .update(|app| app.fetch_asset::<MonitorImageDecoder>(&exhausted_source))
            .unwrap()
            .is_err()
    );

    let other_source = source();
    let other_window = player(cx, other_source.clone());
    draw(cx, other_window);
    let decoded = painted(cx, other_window, &other_source);
    let tile = cx
        .update_window(other_window, |_, window, _| {
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    drop(budget);
    exhausted.update(|app| image_residency::retry_failed(&exhausted_source, app));
    draw(&mut exhausted, exhausted_window);
    let _recovered = painted(&mut exhausted, exhausted_window, &exhausted_source);
    draw(cx, other_window);
    assert!(Arc::ptr_eq(
        &decoded,
        &painted(cx, other_window, &other_source)
    ));
    cx.update_window(other_window, |_, window, _| {
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile);
    })
    .unwrap();
}
