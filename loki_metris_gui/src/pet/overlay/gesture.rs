//! 平台已计数的第二击只在同一位置完成；位移与生命周期取消不可恢复。

use gpui_kit::{ClickEvent, MouseButton, MouseDownEvent, Pixels, Point};

/// 与GPUI 0.3.8拖动判定一致，阈值单位是逻辑像素，边界2px仍可点击。
pub(super) const CLICK_MOVE_THRESHOLD: f64 = 2.;

/// 候选绑定实际slot与第二击按下位置，不累计平台已有的clickCount。
struct Pending {
    slot: u8,
    origin: Point<Pixels>,
}

/// 仅拥有一次指针手势，动画、分页及领域偏好均不属于该状态。
#[derive(Default)]
pub(super) struct DoubleClick {
    pending: Option<Pending>,
}

impl DoubleClick {
    /// 新按下替换旧候选，只有平台明确的第二次左键按下可以等待完成。
    pub(super) fn begin(&mut self, slot: u8, event: &MouseDownEvent) {
        self.pending = (event.button == MouseButton::Left && event.click_count == 2)
            .then_some(Pending { slot, origin: event.position });
    }

    /// 超过阈值立即清空；返回起点也不能重新激活取消的双击。
    pub(super) fn motion(&mut self, position: Point<Pixels>) {
        if self.pending.as_ref().is_some_and(|pending| {
            (position - pending.origin).magnitude() > CLICK_MOVE_THRESHOLD
        }) {
            self.cancel();
        }
    }

    /// 控件、离窗、失焦、隐藏或替换事件只取消，不能产生主窗动作。
    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }

    /// 原生完成点击必须匹配按钮、计数、按下及目标，并且只消费一次。
    pub(super) fn complete(&mut self, slot: u8, event: &ClickEvent) -> bool {
        let Some(pending) = self.pending.take() else { return false };
        let ClickEvent::Mouse(event) = event else { return false };
        pending.slot == slot
            && pending.origin == event.down.position
            && event.down.button == MouseButton::Left
            && event.up.button == MouseButton::Left
            && event.down.click_count == 2
            && event.up.click_count == 2
            && (event.up.position - pending.origin).magnitude() <= CLICK_MOVE_THRESHOLD
    }
}
