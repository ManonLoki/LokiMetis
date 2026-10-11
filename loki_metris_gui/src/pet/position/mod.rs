//! 桌宠几何写入只拥有最后用户意图与在途回执；权威保存值仍由 MonitorSettings 持有。

use super::Controller;
use gpui_kit::{Bounds, Context, Pixels};
use loki_metris_core::PetOverlayPosition;
use std::time::{Duration, Instant};

/// 不同物理像素位置静止满200ms才可提交；同像素通知不会延期。
const POSITION_DEBOUNCE: Duration = Duration::from_millis(200);

/// 一个尚未被成功回执消费的位置意图。
#[derive(Clone, Copy)]
struct PositionIntent {
    position: PetOverlayPosition,
    due_at: Instant,
}

/// 单写者队列保留latest desired及已接受的inflight；返回旧saved值也不能吞掉在途写后的恢复。
#[derive(Default)]
pub(super) struct PositionQueue {
    desired: Option<PositionIntent>,
    in_flight: Option<PetOverlayPosition>,
    /// 当前driver成功写入的操作回执；保护稍后到达的旧展示快照，不额外持久化或推导领域值。
    last_successful_write: Option<PetOverlayPosition>,
}
impl PositionQueue {
    /// 位置先量化为物理像素；未接受写入时回到saved可取消，否则保留最终恢复意图。
    fn observe(
        &mut self,
        saved: Option<PetOverlayPosition>,
        position: PetOverlayPosition,
        now: Instant,
    ) {
        if self
            .desired
            .is_some_and(|intent| intent.position == position)
        {
            return;
        }
        let acknowledged = self.last_successful_write.or(saved);
        if self.in_flight.is_none() && acknowledged == Some(position) {
            self.desired = None;
        } else {
            self.desired = Some(PositionIntent {
                position,
                due_at: now + POSITION_DEBOUNCE,
            });
        }
    }

    /// 无在途回执时返回距离最新意图可提交的时间；空队列结束driver。
    fn delay(&self, now: Instant) -> Option<Duration> {
        if self.in_flight.is_some() {
            return None;
        }
        self.desired
            .map(|intent| intent.due_at.saturating_duration_since(now))
    }

    /// 防抖到期才认领单个写入；已接受回执尚未结束时不能提交下一项。
    fn begin(&mut self, now: Instant) -> Option<PetOverlayPosition> {
        if self.in_flight.is_some() {
            return None;
        }
        let intent = self.desired?;
        if now < intent.due_at {
            return None;
        }
        self.in_flight = Some(intent.position);
        Some(intent.position)
    }

    /// 成功只消费仍相同的最终意图；旧失败不能丢掉较新位置，同目标失败留待事件重试或退出。
    fn complete(
        &mut self,
        position: PetOverlayPosition,
        committed: Option<PetOverlayPosition>,
    ) -> bool {
        debug_assert_eq!(self.in_flight, Some(position));
        self.in_flight = None;
        if let Some(committed) = committed {
            self.last_successful_write = Some(committed);
        }
        let succeeded = committed == Some(position);
        let same = self
            .desired
            .is_some_and(|intent| intent.position == position);
        if succeeded && same {
            self.desired = None;
        }
        self.desired.is_some() && (succeeded || !same)
    }

    /// 退出交出最后意图；服务先排空已接受工作，再提交该位置。
    pub(super) fn take_dirty(&mut self) -> Option<PetOverlayPosition> {
        self.desired.take().map(|intent| intent.position)
    }
}

impl Controller {
    /// 保留原生DPI舍入规则，真实移动/首布局可保存；重复bounds不会重启防抖或替换在途任务。
    pub(super) fn moved(&mut self, bounds: Bounds<Pixels>, scale: f32, cx: &mut Context<Self>) {
        if self.applying || self.display_geometry_unconfirmed || !self.geometry_restored || self.quitting {
            return;
        }
        let position = PetOverlayPosition {
            x: (f32::from(bounds.origin.x) * scale).round() as i32,
            y: ((f32::from(bounds.origin.y) + super::geometry::HEADER_HEIGHT as f32) * scale).round() as i32,
        };
        self.position.observe(
            self.settings.pet_overlay_position,
            position,
            cx.background_executor().now(),
        );
        if self.position_task.is_some() || self.position.desired.is_none() {
            return;
        }
        self.position_task = Some(cx.spawn(async move |entity, cx| {
            loop {
                let delay = entity
                    .update(cx, |this, cx| {
                        this.position.delay(cx.background_executor().now())
                    })
                    .ok()
                    .flatten();
                let Some(delay) = delay else {
                    let _ = entity.update(cx, |this, _| {
                        this.position_task = None;
                    });
                    return;
                };
                cx.background_executor().timer(delay).await;
                let work = entity
                    .update(cx, |this, cx| {
                        this.position
                            .begin(cx.background_executor().now())
                            .map(|position| (position, this.service.save_pet_position(position)))
                    })
                    .ok()
                    .flatten();
                let Some((position, work)) = work else {
                    continue;
                };
                let result = match work {
                    Ok(work) => {
                        crate::usage_service::receipt(
                            work,
                            cx.background_executor().clone(),
                            "error.monitor.settingsWriteFailed",
                        )
                        .await
                    }
                    Err(_) => Err(loki_metris_core::HookError::new(
                        "error.monitor.settingsWriteFailed",
                    )),
                };
                let keep_running = entity
                    .update(cx, |this, _| {
                        if let Ok(settings) = &result {
                            this.settings.pet_overlay_position = settings.pet_overlay_position;
                        }
                        let committed = result
                            .as_ref()
                            .ok()
                            .and_then(|settings| settings.pet_overlay_position);
                        let keep_running =
                            !this.quitting && this.position.complete(position, committed);
                        if !keep_running {
                            this.position_task = None;
                        }
                        keep_running
                    })
                    .unwrap_or(false);
                if !keep_running {
                    return;
                }
            }
        }));
    }
}

#[cfg(test)]
mod tests;
