//! 本文件收纳：平台查询和本地开关分离；Denied 不会把本地 enabled 改成 false；OS 明确允许提交；OS 明确拒绝。

use super::{BucketRetry, MilestoneIssue, MilestoneState, Reservation};
use crate::{TodayUsageSnapshot, stable_id};
use serde::{Deserialize, Serialize};

/// 平台查询和本地开关分离；Denied 不会把本地 enabled 改成 false。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotificationPermission {
    /// OS 明确允许提交。
    Allowed,
    /// OS 明确拒绝。
    Denied,
    /// 需要用户主动授权，后台不能弹窗。
    NotDetermined,
    /// 裸二进制、未知或不可用宿主。
    #[default]
    Unavailable,
}
/// sender 回执仅表达 API 是否接受，无法证明用户实际看到横幅。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MilestoneOutcome {
    /// 平台 API 明确接受。
    Accepted,
    /// 平台 API 明确失败。
    Rejected,
    /// 超时或调用结果不明。
    Unknown,
}
/// 每档独立发送意图，只在预约持久化后交给平台。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MilestoneRequest {
    /// 稳定请求 ID，跨进程重试保持相同。
    pub id: String,
    /// 所属日界 key。
    pub day_key: String,
    /// 预约所用来源集合，变更后不得派发旧意图。
    pub source: String,
    /// 预约时间，宿主不能执行已经超时的内存意图。
    pub reserved_ms: i64,
    /// 新跨越的档位。
    pub bucket: u64,
    /// 提交时最新可信今日总量；同一跳跃的三条可以正文相同但 ID 不同。
    pub total_tokens: u64,
}
/// ID 保留完整十六进制档位，两档绝不共享 tag。
pub fn milestone_request_id(day_key: &str, bucket: u64) -> String {
    format!("{}-{bucket:016x}", stable_id("m1", day_key))
}
impl MilestoneState {
    /// 一次只预约一档，拒绝过期、关闭、权限不足、Unknown 与自动重试耗尽状态。
    pub fn reserve(
        &mut self,
        snapshot: &TodayUsageSnapshot,
        permission: NotificationPermission,
        now_ms: i64,
    ) -> Option<MilestoneRequest> {
        if !self.enabled
            || permission != NotificationPermission::Allowed
            || now_ms < self.next_send_ms
            || self.source.as_deref() != Some(snapshot.source.as_str())
            || self.active_key.as_deref() != Some(snapshot.day.key().as_str())
            || !snapshot.eligible(now_ms, &snapshot.day, &snapshot.source)
            || self.days.iter().any(|d| d.reservation.is_some())
        {
            return None;
        }
        let day = self.days.last_mut()?;
        if day.retries.iter().any(|r| r.failures >= 4) {
            self.issue = Some(MilestoneIssue::RetryExhausted);
            return None;
        }
        let retry = day
            .retries
            .iter()
            .enumerate()
            .min_by_key(|(_, r)| r.bucket)
            .map(|(i, r)| (i, r.clone()));
        let (bucket, failures) = if let Some((index, retry)) = retry {
            if retry.due_ms > now_ms {
                return None;
            }
            day.retries.remove(index);
            (retry.bucket, retry.failures)
        } else {
            let range = day.pending.first_mut()?;
            let bucket = range.first;
            range.first += 1;
            if range.first > range.last {
                day.pending.remove(0);
            }
            (bucket, 0)
        };
        day.reservation = Some(Reservation {
            bucket,
            started_ms: now_ms,
            failures,
            unknown: false,
        });
        let day_key = day.day.key();
        Some(MilestoneRequest {
            id: milestone_request_id(&day_key, bucket),
            day_key,
            bucket,
            total_tokens: snapshot.total_tokens,
            source: snapshot.source.clone(),
            reserved_ms: now_ms,
        })
    }
    /// 宿主证明尚未调用OS时归还预约；不消耗发送失败次数，旧策略事件明确抑制。
    pub fn defer_unsubmitted(&mut self, id: &str, now_ms: i64) {
        let Some(day) = self.days.iter_mut().find(|d| {
            d.reservation
                .as_ref()
                .is_some_and(|r| milestone_request_id(&d.day.key(), r.bucket) == id)
        }) else {
            return;
        };
        let reservation = day.reservation.take().unwrap();
        if reservation.unknown {
            day.unknown_count = day.unknown_count.saturating_sub(1);
        }
        if self.enabled && day.day.contains(now_ms) && reservation.bucket > day.retry_floor {
            day.retries.push(BucketRetry {
                bucket: reservation.bucket,
                failures: reservation.failures,
                due_ms: now_ms,
            });
        } else {
            day.suppressed = day.suppressed.saturating_add(1);
        }
    }
    /// 用户显式重试接受可能重复的 Unknown 风险；关闭时不重新入队。
    pub fn retry(&mut self, now_ms: i64) {
        if !self.enabled {
            return;
        }
        let Some(day) = self.days.last_mut() else {
            return;
        };
        for retry in &mut day.retries {
            retry.failures = 0;
            retry.due_ms = now_ms;
        }
        if day.reservation.as_ref().is_some_and(|r| r.unknown) {
            let reservation = day.reservation.take().unwrap();
            if reservation.bucket > day.retry_floor {
                day.retries.push(BucketRetry {
                    bucket: reservation.bucket,
                    failures: 0,
                    due_ms: now_ms,
                });
            } else {
                day.suppressed = day.suppressed.saturating_add(1);
            }
            day.unknown_count = day.unknown_count.saturating_sub(1);
        }
        self.issue = None;
    }
    /// Windows 晚到确定失败只加入该档稀疏重试；重复回调与旧日回调不重发其它档。
    pub fn late_failure(&mut self, id: &str, now_ms: i64) {
        let Some(day) = self.days.last_mut() else {
            return;
        };
        if !self.enabled || !day.day.contains(now_ms) {
            return;
        }
        if day
            .reservation
            .as_ref()
            .is_some_and(|r| milestone_request_id(&day.day.key(), r.bucket) == id)
        {
            self.complete(id, MilestoneOutcome::Rejected, now_ms);
            return;
        }
        let Some(bucket) = id
            .rsplit('-')
            .next()
            .and_then(|v| u64::from_str_radix(v, 16).ok())
        else {
            return;
        };
        if milestone_request_id(&day.day.key(), bucket) != id
            || bucket <= day.retry_floor
            || bucket > day.submitted_through
            || day.retries.iter().any(|r| r.bucket == bucket)
            || day.retries.len() >= 256
        {
            return;
        }
        day.retries.push(BucketRetry {
            bucket,
            failures: 1,
            due_ms: now_ms.saturating_add(30_000),
        });
        self.issue = Some(MilestoneIssue::Rejected);
    }
}
/// 确定失败保留同 ID 并采用30/120/600秒等待；未知结果绝不自动重复。
pub(super) fn complete(
    state: &mut MilestoneState,
    id: &str,
    outcome: MilestoneOutcome,
    now_ms: i64,
) {
    let Some(day) = state.days.iter_mut().find(|d| {
        d.reservation
            .as_ref()
            .is_some_and(|r| milestone_request_id(&d.day.key(), r.bucket) == id)
    }) else {
        return;
    };
    let reservation = day.reservation.as_mut().unwrap();
    if outcome == MilestoneOutcome::Unknown {
        if !reservation.unknown {
            reservation.unknown = true;
            day.unknown_count = day.unknown_count.saturating_add(1);
        }
        state.issue = Some(MilestoneIssue::Unknown);
        return;
    }
    let reservation = day.reservation.take().unwrap();
    if reservation.unknown {
        day.unknown_count = day.unknown_count.saturating_sub(1);
    }
    if outcome == MilestoneOutcome::Accepted {
        day.submitted = day.submitted.saturating_add(1);
        day.submitted_through = day.submitted_through.max(reservation.bucket);
        state.next_send_ms = now_ms.saturating_add(10_000);
        state.issue = None;
    } else if state.enabled && day.day.contains(now_ms) {
        let failures = reservation.failures.saturating_add(1).min(4);
        let delay = match failures {
            1 => 30_000,
            2 => 120_000,
            _ => 600_000,
        };
        day.retries.push(BucketRetry {
            bucket: reservation.bucket,
            failures,
            due_ms: now_ms.saturating_add(delay),
        });
        state.issue = Some(if failures == 4 {
            MilestoneIssue::RetryExhausted
        } else {
            MilestoneIssue::Rejected
        });
    } else {
        day.suppressed = day.suppressed.saturating_add(1);
    }
}
