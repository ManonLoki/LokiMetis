//! 光轨几何、暂停恢复及减少动态效果的确定性回归，不启动真实宿主服务。

use super::*;
use gpui_kit::size;
use std::time::Duration;

/// 各种宽高和圆角下的中心线连续闭合，光晕包络不越过卡片边界。
#[test]
fn rounded_tracks_close_and_keep_the_glow_inside_every_card_size() {
    for (width, height) in [(400., 176.), (96., 112.), (320., 40.), (8., 8.)] {
        for radius in [0., 12., 20.] {
            let bounds = Bounds::new(point(px(30.), px(50.)), size(px(width), px(height)));
            let rail = RoundedRail::new(bounds, px(radius)).unwrap();
            let first = rail.point(0.);
            let last = rail.point(rail.length);
            assert!((f32::from(first.x - last.x)).abs() < 0.001);
            assert!((f32::from(first.y - last.y)).abs() < 0.001);
            let mut previous = rail.point(-rail.length / 1024.);
            for step in 0..=1024 {
                let position = rail.point(rail.length * step as f32 / 1024.);
                let x = f32::from(position.x - bounds.origin.x);
                let y = f32::from(position.y - bounds.origin.y);
                assert!(x >= GLOW_WIDTH / 2. && x <= width - GLOW_WIDTH / 2.);
                assert!(y >= GLOW_WIDTH / 2. && y <= height - GLOW_WIDTH / 2.);
                let delta = position - previous;
                let distance = f32::from(delta.x).hypot(f32::from(delta.y));
                assert!(distance <= rail.length / 1024. + 0.002, "光轨跨越转角或接缝不能跳线");
                previous = position;
            }
        }
    }
}

/// 没有可绘制面积时拒绝路径，避免极窄卡片产生负尺寸或NaN。
#[test]
fn empty_and_non_finite_card_bounds_skip_the_track() {
    for (width, height) in [(0., 120.), (120., 0.), (5., 5.), (f32::NAN, 100.), (100., f32::INFINITY)] {
        assert!(RoundedRail::new(Bounds::new(point(px(0.), px(0.)), size(px(width), px(height))), px(20.)).is_none());
    }
}

/// 主要卡片八秒循环，暂停后只累计新活动帧间隔，隐藏时间不造成瞬移。
#[test]
fn primary_orbit_pauses_without_catching_up_background_time() {
    let now = Instant::now();
    let mut motion = CardDecoration::default();
    assert!(motion.tick(CardPriority::Primary, false, true, now));
    motion.tick(CardPriority::Primary, false, true, now + Duration::from_secs(2));
    assert!((motion.phase - 0.25).abs() < 1e-6);
    motion.pause();
    motion.tick(CardPriority::Primary, false, true, now + Duration::from_secs(120));
    assert!((motion.phase - 0.25).abs() < 1e-6);
    motion.tick(CardPriority::Primary, false, true, now + Duration::from_secs(122));
    assert!((motion.phase - 0.5).abs() < 1e-6);
    motion.tick(CardPriority::Primary, false, true, now + Duration::from_secs(126));
    assert!(motion.phase.abs() < 1e-6);
}

/// 辅助卡不播放；次卡仅悬停播放；减少动态、失焦与离屏统一禁止申请帧。
#[test]
fn hierarchy_and_motion_gate_prevent_unnecessary_animation() {
    let now = Instant::now();
    let mut motion = CardDecoration::default();
    for priority in [CardPriority::Primary, CardPriority::Secondary, CardPriority::Quiet] {
        for hovered in [false, true] {
            assert!(!motion.tick(priority, hovered, false, now));
            assert!(motion.last_tick.is_none());
            let expected = priority == CardPriority::Primary || priority == CardPriority::Secondary && hovered;
            assert_eq!(motion.tick(priority, hovered, true, now), expected);
            motion.pause();
        }
    }
}

/// 快速退出再进入次卡时沿用原光轨位置，已暂停动画不会留下活动时钟。
#[test]
fn secondary_hover_reentry_does_not_jump_or_keep_a_clock_alive() {
    let now = Instant::now();
    let mut motion = CardDecoration::default();
    motion.tick(CardPriority::Secondary, true, true, now);
    motion.tick(CardPriority::Secondary, true, true, now + Duration::from_secs(1));
    let phase = motion.phase;
    assert!(!motion.tick(CardPriority::Secondary, false, true, now + Duration::from_secs(2)));
    assert!(motion.last_tick.is_none());
    assert!(motion.tick(CardPriority::Secondary, true, true, now + Duration::from_secs(30)));
    assert_eq!(motion.phase, phase);
}
