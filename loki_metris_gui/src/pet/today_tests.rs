//! 顶栏使用真实 GPUI 布局、事件与 bounds 观察，验证更新不侵入宠物画布。

use super::{
    tests::{fixture, settle},
    *,
};
use gpui_kit::{ScrollDelta, TestAppContext, test::TestWindowExt};
use loki_metris_core::{Completeness, Confidence};

/// 固定标题32，图片格子仍64；标题上的鼠标操作不触发画布分页、缩放或设置。
#[gpui_kit::test]
fn today_header_preserves_canvas_and_isolates_mouse_actions(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let before = f.owner.read_with(cx, |owner, _| owner.settings.pet_window);
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        let header = window.find("pet-today-tokens").bounds();
        let body = window.find("pet-overlay").bounds();
        assert_eq!(header.size, size(px(128.), px(28.)));
        assert_eq!(body.origin.y - header.origin.y, px(32.));
        assert_eq!(body.size, size(px(128.), px(128.)));
        for id in 0_usize..4 {
            assert_eq!(
                window.find(("pet-position", id)).bounds().size,
                size(px(64.), px(64.))
            );
        }
        window.right_click("pet-today-tokens", cx);
        window.double_click("pet-today-tokens", cx);
        window.scroll(
            "pet-today-tokens",
            ScrollDelta::Pixels(point(px(0.), px(120.))),
            cx,
        );
    })
    .unwrap();
    settle(cx, &f.owner);
    assert_eq!(
        f.owner.read_with(cx, |owner, _| owner.settings.pet_window),
        before
    );
    assert!(
        f.owner
            .read_with(cx, |owner, _| owner.settings_window.is_none())
    );
    assert_eq!(f.service.pet_settings_revision(), 0);
}

/// 数值和质量更新绘制实际标签，原生外框、画布原点及每格大小保持稳定。
#[gpui_kit::test]
fn today_value_and_quality_changes_do_not_resize_window(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let count = cx.new(|_| 0_usize);
    let subscription = cx
        .update_window(f.overlay, |_, window, cx| {
            count.update(cx, |_, cx| {
                cx.observe_window_bounds(window, |n, _, _| *n += 1)
            })
        })
        .unwrap();
    let original = cx
        .update_window(f.overlay, |_, window, cx| {
            window.render_frame(cx);
            window.bounds()
        })
        .unwrap();
    for value in [
        TodayUsageDisplay::Value {
            total_tokens: 0,
            completeness: Completeness::Complete,
            confidence: Confidence::Exact,
        },
        TodayUsageDisplay::Value {
            total_tokens: u64::MAX,
            completeness: Completeness::Partial,
            confidence: Confidence::Derived,
        },
        TodayUsageDisplay::Unavailable,
        TodayUsageDisplay::Expired,
    ] {
        f.owner.update(cx, |owner, cx| {
            owner.today = value;
            cx.notify();
        });
        cx.run_until_parked();
        cx.update_window(f.overlay, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.bounds(), original);
            assert_eq!(
                window.find("pet-overlay").bounds().size,
                size(px(128.), px(128.))
            );
        })
        .unwrap();
    }
    assert_eq!(count.read_with(cx, |n, _| *n), 0);
    assert_eq!(f.service.pet_settings_revision(), 0);
    drop(subscription);
}

/// 同一整数格式用于宽窄视图；部分和推算必须出现在完整可访问文案中。
#[test]
fn today_labels_preserve_exact_large_integer_and_quality() {
    let value = TodayUsageDisplay::Value {
        total_tokens: u64::MAX,
        completeness: Completeness::Partial,
        confidence: Confidence::Derived,
    };
    let (short, full) = today::labels(value, "en-US", true);
    assert!(short.ends_with('*'));
    assert!(full.contains("18,446,744,073,709,551,615"));
    assert!(full.contains("Partial") && full.contains("Derived"));
    assert_ne!(
        today::labels(TodayUsageDisplay::NoRecords, "en-US", true).0,
        today::labels(
            TodayUsageDisplay::Value {
                total_tokens: 0,
                completeness: Completeness::Complete,
                confidence: Confidence::Exact
            },
            "en-US",
            true
        )
        .0
    );
}

/// 真实 service 投影在文件版本早退前更新；稳定 Pending 和关闭状态都不能重复 notify。
#[gpui_kit::test]
fn today_poll_notifies_only_on_display_transition_before_file_early_return(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, _| {
        let (a, b, c) = owner.snapshot_revisions();
        owner.observed = (a, b, c, u64::MAX);
    });
    let count = cx.new(|_| 0_usize);
    let observed_owner = f.owner.clone();
    let subscription = count.update(cx, |_, cx| {
        cx.observe(&observed_owner, |count, _, _| *count += 1)
    });
    for _ in 0..10 {
        f.owner.update(cx, |owner, cx| owner.refresh(cx));
        cx.run_until_parked();
    }
    assert_eq!(count.read_with(cx, |n, _| *n), 0);
    f.service.finish();
    for _ in 0..10 {
        f.owner.update(cx, |owner, cx| owner.refresh(cx));
        cx.run_until_parked();
    }
    assert_eq!(
        f.owner.read_with(cx, |owner, _| owner.today),
        TodayUsageDisplay::Unavailable
    );
    assert_eq!(count.read_with(cx, |n, _| *n), 1);
    drop(subscription);
}
