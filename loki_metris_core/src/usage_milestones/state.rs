//! 本文件收纳：持久化失败分类不含系统消息、路径或业务载荷；首次状态或损坏历史只能明确建立当前值基线；已确认的本地关闭取消未提交项。

use super::{MilestoneOutcome, TOKEN_MILESTONE};
use crate::{TodayUsageSnapshot, UsageDay};
use serde::{Deserialize, Serialize};

/// 持久化失败分类不含系统消息、路径或业务载荷。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MilestoneIssue {
    /// 首次状态或损坏历史只能明确建立当前值基线。
    Baseline,
    /// 已确认的本地关闭取消未提交项。
    Disabled,
    /// 新来源集合取消旧来源事件。
    SourceChanged,
    /// 时间或时区切换使旧事件过期。
    Expired,
    /// OS 确定拒绝，可按同 ID 有界重试。
    Rejected,
    /// OS 和本地文件无原子事务，无法确定是否已经提交。
    Unknown,
    /// 自动重试耗尽，等待用户明确动作。
    RetryExhausted,
    /// 文件状态或容量非法，驱动应停止业务发送。
    Storage,
}
/// 压缩连续档位，不因大幅跳跃展开对象或丢档。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketRange {
    /// 包含的首档。
    pub first: u64,
    /// 包含的末档。
    pub last: u64,
}
/// 稀疏晚到失败只重试自身，不倒退其它已提交档位。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketRetry {
    /// 同日唯一档位。
    pub bucket: u64,
    /// 已知确定失败次数，初次提交外最多三次自动重试。
    pub failures: u8,
    /// Unix 毫秒重试截止。
    pub due_ms: i64,
}
/// 已先持久化的发送预约；崩溃后默认 Unknown，不能盲目重复。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reservation {
    /// 日界内档位。
    pub bucket: u64,
    /// 本次预约形成时间。
    pub started_ms: i64,
    /// 已发生的确定失败次数。
    pub failures: u8,
    /// 进程恢复或回执不明时置位。
    pub unknown: bool,
}
/// 每个日界保留高水位与结果计数；基线不计为发送成功。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneDay {
    /// 原民用日界。
    pub day: UsageDay,
    /// 新策略仅为0/1，表示当日是否已入队或建基线；回落不降低。
    pub high: u64,
    /// 因首次安装或重开跳过的已有档位数。
    pub baseline: u64,
    /// 未提交的压缩范围。
    pub pending: Vec<BucketRange>,
    /// OS 接受次数，仅代表 API 回执。
    pub submitted: u64,
    /// 来源改变、关闭或基线之后不再重试的历史档位上界。
    #[serde(default)]
    pub retry_floor: u64,
    /// 最近提交档位，仅辅助判别晚到失败。
    pub submitted_through: u64,
    /// 主动关闭或来源改变取消的未提交数量。
    pub suppressed: u64,
    /// 旧日未提交而过期数量。
    pub expired: u64,
    /// 结果不明数量。
    pub unknown_count: u64,
    /// 至多一个预约。
    pub reservation: Option<Reservation>,
    /// 确定失败的稀疏重试，容量受文件策略约束。
    pub retries: Vec<BucketRetry>,
}
impl MilestoneDay {
    /// 仅创建纯数据日账本；是否建已有量基线由状态机决定。
    pub(super) fn new(day: UsageDay) -> Self {
        Self {
            day,
            high: 0,
            baseline: 0,
            pending: vec![],
            submitted: 0,
            submitted_through: 0,
            retry_floor: 0,
            suppressed: 0,
            expired: 0,
            unknown_count: 0,
            reservation: None,
            retries: vec![],
        }
    }
    /// 宿主观察到日界改变时先过期旧队列，不等待新的可信聚合。
    pub fn cancel_for_expiry(&mut self) {
        self.cancel(true);
    }
    /// 待处理计数用整数范围求和，包括确定失败重试，不含单独披露的 Unknown。
    pub fn pending_count(&self) -> u64 {
        self.pending
            .iter()
            .map(|r| r.last - r.first + 1)
            .sum::<u64>()
            + self.retries.len() as u64
            + u64::from(self.reservation.as_ref().is_some_and(|r| !r.unknown))
    }
    /// 日界失效或关闭时清掉未提交队列，保留已预约的不确定性记录。
    pub(super) fn cancel(&mut self, expired: bool) {
        // 已预约项可能已进入OS，不能同时声称“已抑制”；纯未提交范围才计入取消。
        let count = self
            .pending
            .iter()
            .map(|r| r.last - r.first + 1)
            .sum::<u64>()
            + self.retries.len() as u64;
        if !expired {
            self.retry_floor = self.high;
        }
        if expired {
            self.expired = self.expired.saturating_add(count);
        } else {
            self.suppressed = self.suppressed.saturating_add(count);
        }
        self.pending.clear();
        self.retries.clear();
        if let Some(r) = self.reservation.as_mut()
            && !r.unknown {
                r.unknown = true;
                self.unknown_count = self.unknown_count.saturating_add(1);
            }
        if expired {
            self.reservation = None;
        }
    }
}
/// schema/策略版本与最多 32 个日界构成应用唯一权威去重账本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneState {
    /// 独立文件 schema，未知版本失败关闭。
    pub schema_version: u8,
    /// 版本 2 为每日首次突破 1B；版本 1 的 100M 账本只允许显式迁移。
    pub policy_version: u8,
    /// 最后已确认的本地开关。
    pub enabled: bool,
    /// 已建立过可信基线。
    pub initialized: bool,
    /// 下一个可信快照只建基线。
    pub needs_baseline: bool,
    /// 最近活动日 key。
    pub active_key: Option<String>,
    /// 最近时区规则，普通跨日和时区切换不同。
    pub zone: Option<String>,
    /// 最近来源集合摘要。
    pub source: Option<String>,
    /// 最近 32 个民用日界，活动日在末端。
    pub days: Vec<MilestoneDay>,
    /// 下一条成功提交最早时间，防止多档突发。
    pub next_send_ms: i64,
    /// 最近可披露原因。
    pub issue: Option<MilestoneIssue>,
}
impl Default for MilestoneState {
    /// 缺失文件从新策略开始，只考虑首次可信的当日总量，不假造历史提交。
    fn default() -> Self {
        Self {
            schema_version: 1,
            policy_version: 2,
            enabled: false,
            initialized: false,
            needs_baseline: true,
            active_key: None,
            zone: None,
            source: None,
            days: vec![],
            next_send_ms: 0,
            issue: None,
        }
    }
}
impl MilestoneState {
    /// 解码后检查范围、容量与单预约不变量，非法历史不得据此发送。
    pub fn valid(&self) -> bool {
        self.valid_policy(2)
    }
    /// 验证指定策略的历史，不把旧 100M 档位直接解释为新策略的日事件。
    fn valid_policy(&self, policy: u8) -> bool {
        self.schema_version == 1
            && self.policy_version == policy
            && self.days.len() <= 32
            && self.active_key == self.days.last().map(|d| d.day.key())
            && self.days.iter().filter(|d| d.reservation.is_some()).count() <= 1
            && self.days.iter().enumerate().all(|(i, d)| {
                d.day.start_ms < d.day.end_ms
                    && d.high <= if policy == 1 { u64::MAX / 100_000_000 } else { 1 }
                    && d.baseline <= d.high
                    && d.submitted_through <= d.high
                    && d.retry_floor <= d.high
                    && d.pending.len() <= 256
                    && d.retries.len() <= 256
                    && !self.days[..i].iter().any(|p| p.day.key() == d.day.key())
                    && d.pending.iter().enumerate().all(|(i, r)| {
                        r.first > 0
                            && r.first <= r.last
                            && r.last <= d.high
                            && (i == 0 || d.pending[i - 1].last < r.first)
                    })
                    && d.retries.iter().enumerate().all(|(i, r)| {
                        r.bucket > 0
                            && r.bucket <= d.high
                            && r.failures <= 4
                            && !d.retries[..i].iter().any(|p| p.bucket == r.bucket)
                            && !d
                                .pending
                                .iter()
                                .any(|p| p.first <= r.bucket && r.bucket <= p.last)
                    })
                    && d.reservation.as_ref().is_none_or(|r| {
                        r.bucket > 0
                            && r.bucket <= d.high
                            && r.failures <= 4
                            && !d
                                .pending
                                .iter()
                                .any(|p| p.first <= r.bucket && r.bucket <= p.last)
                            && !d.retries.iter().any(|p| p.bucket == r.bucket)
                    })
            })
    }
    /// 旧策略仅迁移可信去重事实，取消全部旧预约与重试；可证明已超过 1B 的日界保守建基线。
    pub fn upgrade_policy(&mut self) -> bool {
        if self.policy_version == 2 { return self.valid(); }
        if !self.valid_policy(1) { return false; }
        for day in &mut self.days {
            let high = u64::from(day.high >= 11);
            *day = MilestoneDay::new(day.day.clone());
            day.high = high;
            day.baseline = high;
            day.retry_floor = high;
        }
        self.policy_version = 2;
        self.needs_baseline = true;
        self.next_send_ms = 0;
        self.issue = Some(MilestoneIssue::Baseline);
        self.valid()
    }
    /// 只在偏好真实回执后调用；重开不追补关闭期间的消费。
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.needs_baseline = true;
        if !enabled {
            for day in &mut self.days {
                day.cancel(false);
            }
            self.issue = Some(MilestoneIssue::Disabled);
        }
    }
    /// 同日高水位只有 0/1；首次可信观测突破即入队，来源重建和重开仍抑制已有消费。
    pub fn observe(&mut self, snapshot: &TodayUsageSnapshot, now_ms: i64) {
        if !self.enabled || !snapshot.eligible(now_ms, &snapshot.day, &snapshot.source) {
            return;
        }
        let key = snapshot.day.key();
        let source_changed = self.source.as_ref().is_some_and(|s| s != &snapshot.source);
        let zone_changed = self.zone.as_ref().is_some_and(|z| z != &snapshot.day.zone);
        let switched_day = self.active_key.as_ref().is_some_and(|k| k != &key);
        if source_changed {
            for d in &mut self.days {
                d.cancel(false);
                d.reservation = None;
            }
            self.issue = Some(MilestoneIssue::SourceChanged);
        }
        if switched_day {
            for d in &mut self.days {
                d.cancel(true);
            }
            self.issue = Some(MilestoneIssue::Expired);
        }
        let existing = self.days.iter().position(|d| d.day.key() == key);
        let rebuilding = self.needs_baseline
            || source_changed
            || (zone_changed && existing.is_none())
            || (!zone_changed
                && existing.is_none()
                && self
                    .days
                    .last()
                    .is_some_and(|d| snapshot.day.start_ms < d.day.start_ms));
        let index = if let Some(index) = existing {
            let day = self.days.remove(index);
            self.days.push(day);
            self.days.len() - 1
        } else {
            if self.days.len() == 32 {
                self.days.remove(0);
            }
            self.days.push(MilestoneDay::new(snapshot.day.clone()));
            self.days.len() - 1
        };
        let day = &mut self.days[index];
        let bucket = u64::from(snapshot.total_tokens > TOKEN_MILESTONE);
        if rebuilding && self.initialized {
            day.baseline += bucket.saturating_sub(day.high);
            day.high = day.high.max(bucket);
            day.retry_floor = day.high;
            self.issue = Some(
                if source_changed || self.issue == Some(MilestoneIssue::SourceChanged) {
                    MilestoneIssue::SourceChanged
                } else {
                    MilestoneIssue::Baseline
                },
            );
        } else if bucket > day.high {
            if let Some(last) = day.pending.last_mut().filter(|r| r.last == day.high) {
                last.last = bucket;
            } else {
                day.pending.push(BucketRange {
                    first: day.high + 1,
                    last: bucket,
                });
            }
            day.high = bucket;
        }
        self.initialized = true;
        self.needs_baseline = false;
        self.active_key = Some(key);
        self.zone = Some(snapshot.day.zone.clone());
        self.source = Some(snapshot.source.clone());
    }
    /// 索引原地重建不能被看作新消费；持久基线意图跨重启保留，普通扫描不调用。
    pub fn source_reset(&mut self) {
        for day in &mut self.days {
            day.cancel(false);
            day.reservation = None;
        }
        self.needs_baseline = true;
        self.issue = Some(MilestoneIssue::SourceChanged);
    }
    /// 当前活动日的只读结果供设置与菜单投影。
    pub fn active(&self) -> Option<&MilestoneDay> {
        self.days
            .iter()
            .find(|d| Some(&d.day.key()) == self.active_key.as_ref())
    }
    /// 崩溃恢复把已保存预约转为 Unknown；需要平台核对或人类明确重试。
    pub fn recover(&mut self) {
        for d in &mut self.days {
            if let Some(r) = d.reservation.as_mut()
                && !r.unknown {
                    r.unknown = true;
                    d.unknown_count = d.unknown_count.saturating_add(1);
                    self.issue = Some(MilestoneIssue::Unknown);
                }
        }
    }
    /// 结果落盘前不能对外声明成功；驱动以事务克隆调用此方法。
    pub fn complete(&mut self, id: &str, outcome: MilestoneOutcome, now_ms: i64) {
        super::delivery::complete(self, id, outcome, now_ms);
    }
}
