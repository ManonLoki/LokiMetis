//! 待提交/在途/回执交错只测试真实PositionQueue状态机，不使用磁盘权威副本。

use super::*;
use gpui_kit::TestAppContext;
const A: PetOverlayPosition = PetOverlayPosition { x: 10, y: 20 };
const B: PetOverlayPosition = PetOverlayPosition { x: 30, y: 40 };

/// 尚未接受B时返回已保存A无需写；相同物理坐标通知不能推迟原截止时间。
#[gpui_kit::test]
fn pending_return_to_saved_cancels_and_duplicate_motion_keeps_deadline(cx: &mut TestAppContext) {
    let now = cx.update(|cx| cx.background_executor().now());
    let mut queue = PositionQueue::default();
    queue.observe(Some(A), B, now);
    queue.observe(Some(A), B, now + Duration::from_millis(150));
    assert_eq!(queue.begin(now + POSITION_DEBOUNCE), Some(B));
    assert!(!queue.complete(B, Some(B)));
    queue.observe(Some(B), A, now);
    queue.observe(Some(B), B, now + Duration::from_millis(100));
    assert!(queue.delay(now + POSITION_DEBOUNCE).is_none());
    assert!(queue.take_dirty().is_none());
}

/// B已接受后返回旧saved A必须排队恢复；同一driver在B回执前不能提交A。
#[gpui_kit::test]
fn in_flight_b_then_return_to_saved_a_preserves_required_restoration(cx: &mut TestAppContext) {
    let now = cx.update(|cx| cx.background_executor().now());
    let mut queue = PositionQueue::default();
    queue.observe(Some(A), B, now);
    assert_eq!(queue.begin(now + POSITION_DEBOUNCE), Some(B));
    queue.observe(Some(A), A, now + Duration::from_millis(210));
    assert!(queue.begin(now + Duration::from_secs(1)).is_none());
    assert!(queue.complete(B, Some(B)));
    assert_eq!(queue.begin(now + Duration::from_secs(1)), Some(A));
    assert!(!queue.complete(A, Some(A)));
    assert!(queue.take_dirty().is_none());
}

/// 较新B失败后仍保留退出意图；旧B失败不阻止较新A继续提交。
#[gpui_kit::test]
fn failures_preserve_latest_intent_and_do_not_swallow_newer_position(cx: &mut TestAppContext) {
    let now = cx.update(|cx| cx.background_executor().now());
    let mut queue = PositionQueue::default();
    queue.observe(Some(A), B, now);
    assert_eq!(queue.begin(now + POSITION_DEBOUNCE), Some(B));
    assert!(!queue.complete(B, None));
    assert_eq!(queue.take_dirty(), Some(B));
    queue.observe(Some(A), B, now);
    assert_eq!(queue.begin(now + POSITION_DEBOUNCE), Some(B));
    queue.observe(Some(A), A, now + POSITION_DEBOUNCE);
    assert!(queue.complete(B, None));
    assert_eq!(queue.begin(now + Duration::from_secs(1)), Some(A));
    assert_eq!(queue.take_dirty(), Some(A));
}

/// B成功之后展示快照仍为A，也不能吞掉用户随后返回A的真实恢复写入。
#[gpui_kit::test]
fn late_old_snapshot_after_success_cannot_hide_return_to_a(cx: &mut TestAppContext) {
    let now = cx.update(|cx| cx.background_executor().now());
    let mut queue = PositionQueue::default();
    queue.observe(Some(A), B, now);
    assert_eq!(queue.begin(now + POSITION_DEBOUNCE), Some(B));
    assert!(!queue.complete(B, Some(B)));
    queue.observe(Some(A), A, now + Duration::from_secs(1));
    assert_eq!(queue.begin(now + Duration::from_secs(2)), Some(A));
    assert!(!queue.complete(A, Some(A)));
    // 此后晚到的B快照也不能制造已成功保存A的重复意图。
    queue.observe(Some(B), A, now + Duration::from_secs(3));
    assert!(queue.delay(now + Duration::from_secs(4)).is_none());
}

/// 新位置C通过同一driver获得fresh成功回执后，原先的B ack必须被替换。
#[gpui_kit::test]
fn fresh_c_receipt_replaces_old_b_ack_before_return_to_b(cx: &mut TestAppContext) {
    let now = cx.update(|cx| cx.background_executor().now());
    let c = PetOverlayPosition { x: -90, y: 77 };
    let mut queue = PositionQueue::default();
    queue.observe(Some(A), B, now);
    assert_eq!(queue.begin(now + POSITION_DEBOUNCE), Some(B));
    assert!(!queue.complete(B, Some(B)));
    queue.observe(Some(A), c, now + Duration::from_secs(1));
    assert_eq!(queue.begin(now + Duration::from_secs(2)), Some(c));
    assert!(!queue.complete(c, Some(c)));
    queue.observe(Some(c), B, now + Duration::from_secs(3));
    assert_eq!(queue.begin(now + Duration::from_secs(4)), Some(B));
    assert!(!queue.complete(B, Some(B)));
}

/// 尚无保存值时首个真实位置必须提交，首次回执后重复bounds才可去重。
#[gpui_kit::test]
fn initial_none_persists_first_position_before_deduplicating(cx: &mut TestAppContext) {
    let now = cx.update(|cx| cx.background_executor().now());
    let mut queue = PositionQueue::default();
    queue.observe(None, A, now);
    assert_eq!(queue.begin(now + POSITION_DEBOUNCE), Some(A));
    assert!(!queue.complete(A, Some(A)));
    queue.observe(None, A, now + Duration::from_secs(1));
    assert!(queue.delay(now + Duration::from_secs(2)).is_none());
}
