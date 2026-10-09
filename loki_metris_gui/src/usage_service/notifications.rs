//! 应用级单 worker：真实今日摘要、core 状态机和文件驱动串行；UI 只收发送意图。

use super::{ServiceContext, UsageService, today};
use crate::backend::{local_index::current_epoch_ms, notification_state::Ledger};
use loki_metris_core::{ScanCancellation, TodayUsageSnapshot, UsageDay, usage_milestones::*};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

/// 宿主只发布已确认开关与权限；最新控制槽有界合并，重试使用单调代数。
#[derive(Clone, Debug, Default)]
pub(crate) struct Control {
    pub initialized: bool,
    pub enabled: bool,
    pub visible: bool,
    pub permission: NotificationPermission,
    pub retry: u64,
    pub closing: bool,
}
/// 仅内存的设置投影，不把 API 接受写成实际系统送达。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Progress {
    pub pending: u64,
    pub submitted: u64,
    pub baseline: u64,
    pub suppressed: u64,
    pub expired: u64,
    pub unknown: u64,
    /// 可供平台历史核对的唯一当前 Unknown；无匹配仍不重发。
    pub unknown_id: Option<String>,
    pub issue: Option<MilestoneIssue>,
    pub error: Option<&'static str>,
    pub snapshot: Option<TodayUsageSnapshot>,
    pub revision: u64,
}
/// sender 回执与稀疏失败经过一个有界通道回到持久 owner。
pub(crate) enum Receipt {
    Outcome(String, MilestoneOutcome),
    LateFailure(String),
    Unsubmitted(String),
}
/// 服务克隆只共享控制和只读投影，所有存储 I/O 在唯一后台 worker。
pub(super) struct Monitor {
    control: watch::Sender<Control>,
    progress: watch::Receiver<Progress>,
    requests: Mutex<mpsc::Receiver<(u64, MilestoneRequest)>>,
    pub receipts: mpsc::Sender<Receipt>,
    overflow: Arc<AtomicBool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl Monitor {
    /// 在业务 runtime 建立单个摘要/账本 worker，初始化不申请权限或发送。
    pub(super) fn start(runtime: &tokio::runtime::Runtime, context: Arc<ServiceContext>) -> Self {
        let (control, rx) = watch::channel(Control::default());
        let (progress_tx, progress) = watch::channel(Progress::default());
        let (request_tx, requests) = mpsc::channel(1);
        let (receipts, receipt_rx) = mpsc::channel(128);
        let overflow = Arc::new(AtomicBool::new(false));
        let task = runtime.spawn(run(
            context,
            rx,
            progress_tx,
            request_tx,
            receipt_rx,
            overflow.clone(),
        ));
        Self {
            control,
            progress,
            requests: Mutex::new(requests),
            receipts,
            overflow,
            task: Mutex::new(Some(task)),
        }
    }
    /// 系统回调队列超限明确暂停，不能把丢失的结果当成功。
    pub(super) fn reply(&self, receipt: Receipt) {
        if self.receipts.try_send(receipt).is_err() {
            self.overflow.store(true, Ordering::Release);
        }
    }
    /// 不可取消退出只等已接受文件操作；不等待 OS 回调或排空通知队列。
    pub(super) async fn finish(&self) {
        self.control.send_modify(|c| c.closing = true);
        let task = self
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}
impl UsageService {
    /// 设置只读最后真实业务投影，渲染不查询磁盘或 OS。
    pub(crate) fn notification_progress(&self) -> Progress {
        self.notifications.progress.borrow().clone()
    }
    /// 保存已确认后改变应用本地意图；不会因权限拒绝偷偷关闭。
    pub(crate) fn configure_notifications(
        &self,
        enabled: bool,
        permission: NotificationPermission,
    ) {
        self.notifications.control.send_if_modified(|c| {
            if c.initialized && c.enabled == enabled && c.permission == permission {
                return false;
            }
            c.initialized = true;
            c.enabled = enabled;
            c.permission = permission;
            true
        });
    }
    /// 浮窗会话需求只控制摘要读取，不增加顶栏或其它桌宠业务。
    pub(crate) fn today_visible(&self, visible: bool) {
        self.notifications.control.send_if_modified(|c| {
            if c.visible == visible {
                false
            } else {
                c.visible = visible;
                true
            }
        });
    }
    /// 主动重试语义由 core 决定，Unknown 风险在按钮文案明确显示。
    pub(crate) fn retry_notifications(&self) {
        self.notifications
            .control
            .send_modify(|c| c.retry = c.retry.wrapping_add(1));
    }
    /// 原生 UI 线程仅取一个已经成功持久预约的请求，不执行文件 I/O。
    pub(crate) fn take_notification(&self) -> Option<MilestoneRequest> {
        let (epoch, request) = self
            .notifications
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_recv()
            .ok()?;
        if epoch
            == self
                .context
                .notification_source_epoch
                .load(Ordering::Acquire)
        {
            Some(request)
        } else {
            self.notification_unsubmitted(request.id);
            None
        }
    }
    /// 平台尚未调用时归还预约，权限暂停不占用自动失败重试预算。
    pub(crate) fn notification_unsubmitted(&self, id: String) {
        self.notifications.reply(Receipt::Unsubmitted(id));
    }
    /// 完成回执只传稳定 ID 与枚举，不携带宿主对象或任意错误文本。
    pub(crate) fn notification_receipt(&self, id: String, outcome: MilestoneOutcome) {
        self.notifications.reply(Receipt::Outcome(id, outcome));
    }
    /// Windows 晚到失败只重新排该档，应用仍运行时可观察。
    pub(crate) fn notification_late_failure(&self, id: String) {
        self.notifications.reply(Receipt::LateFailure(id));
    }
    /// 平台稀疏回调丢失时显式暂停，禁止吞掉失败后继续宣称成功。
    pub(crate) fn notification_callback_overflow(&self) {
        self.notifications.overflow.store(true, Ordering::Release);
        self.refresh_today();
    }
    /// 唤醒、扫描或来源提交只合并为一个待刷新槽。
    pub(crate) fn refresh_today(&self) {
        self.context.today_refresh.notify_one();
    }
}

/// 文件事务在 blocking 岛执行，返回原 owner；失败时不提交候选状态。
async fn persist(mut ledger: Ledger, next: MilestoneState) -> (Ledger, Result<bool, &'static str>) {
    if ledger.state == next {
        return (ledger, Ok(false));
    }
    tokio::task::spawn_blocking(move || {
        let result = ledger.commit(next);
        (ledger, result)
    })
    .await
    .expect("通知文件驱动必须保留 owner")
}
/// 每次只发布语义变化，成功观测时间内部更新但不制造相同值的 UI 刷新风暴。
fn publish(
    tx: &watch::Sender<Progress>,
    ledger: &Ledger,
    snapshot: &Option<TodayUsageSnapshot>,
    error: Option<&'static str>,
) {
    let day = ledger.state.active();
    let mut next = Progress {
        pending: day.map_or(0, MilestoneDay::pending_count),
        submitted: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.submitted)),
        baseline: day.map_or(0, |d| d.baseline),
        suppressed: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.suppressed)),
        expired: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.expired)),
        unknown: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.unknown_count)),
        unknown_id: day.and_then(|d| {
            d.reservation
                .as_ref()
                .filter(|r| r.unknown)
                .map(|r| milestone_request_id(&d.day.key(), r.bucket))
        }),
        issue: ledger.state.issue,
        error: error.or(if ledger.read_error {
            Some("notification_history_rebuilt")
        } else {
            None
        }),
        snapshot: snapshot.clone(),
        revision: tx.borrow().revision,
    };
    let mut old = tx.borrow().clone();
    if let (Some(a), Some(b)) = (&mut old.snapshot, &mut next.snapshot) {
        a.observed_ms = b.observed_ms;
    }
    if old != next {
        next.revision = next.revision.wrapping_add(1);
    }
    // 观测时间仍更新内存；否则长期同值后的权限恢复会被前台永久判作过期。
    tx.send_replace(next);
}
/// 单在途查询与预约不展开大跨度；唤醒后每轮重算本地时区与日界。
async fn run(
    context: Arc<ServiceContext>,
    mut controls: watch::Receiver<Control>,
    progress: watch::Sender<Progress>,
    requests: mpsc::Sender<(u64, MilestoneRequest)>,
    mut receipts: mpsc::Receiver<Receipt>,
    overflow: Arc<AtomicBool>,
) {
    let path = context.directory.join("usage-notification-state.json");
    let mut ledger = tokio::task::spawn_blocking(move || Ledger::load(path))
        .await
        .expect("通知读取 owner");
    let mut recovered = ledger.state.clone();
    recovered.recover();
    let (loaded, result) = persist(ledger, recovered).await;
    ledger = loaded;
    let mut error = result.err();
    let mut snapshot: Option<TodayUsageSnapshot> = None;
    let mut next_refresh = 0_i64;
    let mut retry = 0;
    let mut epoch = context.notification_source_epoch.load(Ordering::Acquire);
    loop {
        let control = controls.borrow_and_update().clone();
        if !control.initialized && !control.closing {
            let _ = controls.changed().await;
            continue;
        }
        let now = current_epoch_ms();
        let mut next = ledger.state.clone();
        next.set_enabled(control.enabled);
        if control.retry != retry {
            retry = control.retry;
            next.retry(now);
            error = None;
        }
        let reset = context.notification_source_epoch.load(Ordering::Acquire);
        if epoch != reset {
            epoch = reset;
            next.source_reset();
            next_refresh = 0;
        }
        while let Ok(receipt) = receipts.try_recv() {
            match receipt {
                Receipt::Outcome(id, outcome) => next.complete(&id, outcome, now),
                Receipt::LateFailure(id) => next.late_failure(&id, now),
                Receipt::Unsubmitted(id) => next.defer_unsubmitted(&id, now),
            }
        }
        let day = UsageDay::capture(now, &jiff::tz::TimeZone::system());
        // 不论有无可信新数据，旧日队列都过期；绝不能用昨天快照发送“今天”。
        if next
            .days
            .last()
            .is_some_and(|d| day.as_ref() != Some(&d.day))
        {
            if snapshot
                .as_ref()
                .is_some_and(|s| day.as_ref() != Some(&s.day))
            {
                next_refresh = 0;
            }
            for old in &mut next.days {
                old.cancel_for_expiry();
            }
            next.issue = Some(MilestoneIssue::Expired);
        }
        if overflow.swap(false, Ordering::AcqRel) {
            error = Some("notification_callback_capacity");
        }
        if let Some(reservation) = next.days.last().and_then(|d| d.reservation.as_ref()) {
            if !reservation.unknown && now.saturating_sub(reservation.started_ms) >= 15_000 {
                let id =
                    milestone_request_id(&next.days.last().unwrap().day.key(), reservation.bucket);
                next.complete(&id, MilestoneOutcome::Unknown, now);
            }
        }
        let (saved, result) = persist(ledger, next).await;
        ledger = saved;
        if let Err(failure) = result {
            error = Some(failure);
        }
        if control.closing {
            publish(&progress, &ledger, &snapshot, error);
            break;
        }
        let demand = control.enabled || control.visible;
        if demand && now >= next_refresh {
            let zone = jiff::tz::TimeZone::system();
            let loaded = today::load(context.clone(), now, zone, ScanCancellation::default()).await;
            let after = current_epoch_ms();
            next_refresh = after.saturating_add(30_000);
            match loaded {
                Ok((_, value))
                    if UsageDay::capture(after, &jiff::tz::TimeZone::system()).as_ref()
                        == Some(&value.day) =>
                {
                    snapshot = Some(value);
                    if error == Some("notification_snapshot_unavailable") {
                        error = None;
                    }
                }
                _ => error = Some("notification_snapshot_unavailable"),
            }
        }
        // 关闭、来源变化在查询期间到来时先处理最新控制，不准预约旧意图。
        if controls.has_changed().unwrap_or(true) {
            continue;
        }
        let now = current_epoch_ms();
        let mut next = ledger.state.clone();
        let value = snapshot.as_ref().filter(|s| day.as_ref() == Some(&s.day));
        if let Some(value) = value {
            if error.is_none() {
                next.observe(value, now);
            }
        }
        // 先持久入队，再单独持久预约；前一步失败绝不能派发系统调用。
        let (saved, result) = persist(ledger, next).await;
        ledger = saved;
        if let Err(failure) = result {
            error = Some(failure);
        }
        let mut next = ledger.state.clone();
        let request = value
            .filter(|_| error.is_none())
            .and_then(|value| next.reserve(value, control.permission, now));
        let (saved, result) = persist(ledger, next).await;
        ledger = saved;
        match result {
            Ok(_) => {
                if let Some(request) = request {
                    if requests.try_send((epoch, request.clone())).is_err() {
                        let mut next = ledger.state.clone();
                        next.complete(&request.id, MilestoneOutcome::Unknown, now);
                        let (saved, result) = persist(ledger, next).await;
                        ledger = saved;
                        error = result.err();
                    }
                }
            }
            Err(failure) => error = Some(failure),
        }
        publish(&progress, &ledger, &snapshot, error);
        // 空闲最多30秒；活跃预约15秒到期、成功10秒节流，确定失败用core截止。
        let mut due = if demand {
            next_refresh
        } else {
            day.as_ref().map_or(now + 30_000, |d| d.end_ms)
        };
        if let Some(d) = ledger.state.active() {
            if let Some(r) = &d.reservation {
                if !r.unknown {
                    due = due.min(r.started_ms + 15_000);
                }
            } else if control.enabled
                && control.permission == NotificationPermission::Allowed
                && error.is_none()
                && !d.retries.iter().any(|r| r.failures >= 4)
            {
                if let Some(r) = d
                    .retries
                    .iter()
                    .filter(|r| r.failures < 4)
                    .min_by_key(|r| r.bucket)
                {
                    due = due.min(r.due_ms.max(now + 1_000));
                } else if !d.pending.is_empty() {
                    due = due.min(ledger.state.next_send_ms.max(now + 1_000));
                }
            }
        }
        tokio::select! {
            _=controls.changed()=>{next_refresh=0;},
            _=context.today_refresh.notified()=>{tokio::time::sleep(Duration::from_millis(250)).await;next_refresh=0;},
            receipt=receipts.recv()=>{if let Some(receipt)=receipt {
                let mut next=ledger.state.clone();let now=current_epoch_ms();
                match receipt {Receipt::Outcome(id,value)=>next.complete(&id,value,now),Receipt::LateFailure(id)=>next.late_failure(&id,now),Receipt::Unsubmitted(id)=>next.defer_unsubmitted(&id,now)}
                let (saved,result)=persist(ledger,next).await;ledger=saved;if let Err(failure)=result {error=Some(failure);}
            }},
            _=tokio::time::sleep(Duration::from_millis(due.saturating_sub(current_epoch_ms()).max(1_000) as u64))=>{},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metris_core::{Completeness, Confidence, TodayAvailability};
    /// 相同总量每次真实读取都更新新鲜度，但不能制造UI revision风暴。
    #[test]
    fn notification_projection_refreshes_age_without_ui_revision() {
        let root = tempfile::tempdir().unwrap();
        let ledger = Ledger::load(root.path().join("state.json"));
        let now = "2026-10-10T12:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        let mut value = TodayUsageSnapshot {
            day: UsageDay::capture(now, &jiff::tz::TimeZone::UTC).unwrap(),
            source: "all".into(),
            total_tokens: 350_000_000,
            observed_ms: now,
            availability: TodayAvailability::Available,
            completeness: Completeness::Complete,
            confidence: Confidence::Exact,
        };
        let (tx, rx) = watch::channel(Progress::default());
        publish(&tx, &ledger, &Some(value.clone()), None);
        let revision = rx.borrow().revision;
        value.observed_ms += 120_000;
        publish(&tx, &ledger, &Some(value.clone()), None);
        assert_eq!(rx.borrow().revision, revision);
        assert_eq!(
            rx.borrow().snapshot.as_ref().unwrap().observed_ms,
            value.observed_ms
        );
        assert!(rx.borrow().snapshot.as_ref().unwrap().eligible(
            value.observed_ms,
            &value.day,
            &value.source
        ));
    }
}
