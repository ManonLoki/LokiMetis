//! 应用级单 worker：真实今日摘要、core 状态机和文件驱动串行；UI 只收发送意图。

use super::{ServiceContext, UsageService, today};
use crate::backend::{local_index::current_epoch_ms, notification_state::Ledger};
use loki_metris_core::{
    ScanCancellation, TodayUsageDisplay, TodayUsageSnapshot, UsageDay, project_today_usage_display,
    usage_milestones::*,
};
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
    /// 宿主已发布第一次开关与权限；之前 worker 不调度通知。
    pub initialized: bool,
    /// 已确认的本地通知开关。
    pub enabled: bool,
    /// 桌宠浮窗正在展示当日摘要；只影响摘要读取频率。
    pub visible: bool,
    /// 已确认的系统通知权限。
    pub permission: NotificationPermission,
    /// 用户主动重试次数，单调递增；worker 看到变化即执行一次重试。
    pub retry: u64,
    /// 应用正在退出，worker 停止调度新通知。
    pub closing: bool,
}
/// 摘要读取事实独立于通知账本错误，时区读取只发生在后台 worker。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TodayReadStatus {
    /// 最近一次读取所属的民用日。
    day: Option<UsageDay>,
    /// 最近一次读取时的通知来源代次；与当前代次不同时需要重新读取。
    source_epoch: u64,
    /// 最近一次读取失败。
    failed: bool,
    /// worker 已关闭，不再读取。
    closed: bool,
}
/// 仅内存的设置投影，不把 API 接受写成实际系统送达。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Progress {
    /// 当日已越过、尚待提交的里程碑档位数。
    pub pending: u64,
    /// 保留天数内系统接受通知的累计次数；只代表 API 回执，不代表已送达。
    pub submitted: u64,
    /// 当日因首次安装或重新开启而直接跳过的既有档位数。
    pub baseline: u64,
    /// 保留天数内因主动关闭或来源变化而取消的未提交数量。
    pub suppressed: u64,
    /// 保留天数内因跨日未提交而过期的数量。
    pub expired: u64,
    /// 保留天数内回执不明的数量。
    pub unknown: u64,
    /// 可供平台历史核对的唯一当前 Unknown；无匹配仍不重发。
    pub unknown_id: Option<String>,
    /// 需要在设置页提示的里程碑问题。
    pub issue: Option<MilestoneIssue>,
    /// 账本读写失败的类别。
    pub error: Option<&'static str>,
    /// 最近一次成功读取的当日用量快照。
    pub snapshot: Option<TodayUsageSnapshot>,
    /// 当日摘要读取状态，只供 worker 内部判断。
    today: TodayReadStatus,
    /// 进度修订号；任一字段变化时递增，前台只比较修订号。
    pub revision: u64,
}
/// sender 回执与稀疏失败经过一个有界通道回到持久 owner。
pub(crate) enum Receipt {
    /// 一次提交的系统回执。
    Outcome(String, MilestoneOutcome),
    /// 提交后才到达的失败回调。
    LateFailure(String),
    /// 预约因条件不满足而没有提交，延后处理。
    Unsubmitted(String),
}
/// 服务克隆只共享控制和只读投影，所有存储 I/O 在唯一后台 worker。
pub(super) struct Monitor {
    /// 宿主发布开关、权限与重试意图的最新值通道。
    control: watch::Sender<Control>,
    /// worker 发布的只读进度投影。
    progress: watch::Receiver<Progress>,
    /// worker 交出的待发送通知（附预约代次），容量 1。
    requests: Mutex<mpsc::Receiver<(u64, MilestoneRequest)>>,
    /// 系统回执回到 worker 的有界通道。
    pub receipts: mpsc::Sender<Receipt>,
    /// 回执通道已满导致回执丢失时置位，worker 据此暂停而不当作成功。
    overflow: Arc<AtomicBool>,
    /// 后台账本 worker 句柄；退出时取出并等待。
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
        if let Some(task) = task
            && let Err(error) = task.await {
                tracing::error!(
                    event = "notification_worker_join_failed",
                    panicked = error.is_panic()
                );
            }
    }
}
impl UsageService {
    /// 设置只读最后真实业务投影，渲染不查询磁盘或 OS。
    pub(crate) fn notification_progress(&self) -> Progress {
        self.notifications.progress.borrow().clone()
    }
    /// 单次借用只投影 Copy 状态，不复制来源字符串、不查询时区或磁盘。
    pub(crate) fn today_display(&self) -> TodayUsageDisplay {
        if self.notifications.control.borrow().closing
            || self.notifications.progress.has_changed().is_err()
        {
            return TodayUsageDisplay::Unavailable;
        }
        let progress = self.notifications.progress.borrow();
        if progress.today.closed {
            return TodayUsageDisplay::Unavailable;
        }
        project_today_usage_display(
            progress.snapshot.as_ref(),
            current_epoch_ms(),
            progress.today.day.as_ref(),
            progress.today.source_epoch
                == self
                    .context
                    .notification_source_epoch
                    .load(Ordering::Acquire),
            progress.today.failed,
        )
    }
    /// 保存已确认后改变应用本地意图；不会因权限拒绝偷偷关闭。
    pub(crate) fn configure_notifications(
        &self,
        enabled: bool,
        permission: NotificationPermission,
    ) {
        self.context.activity_notifications.configure(enabled && permission == NotificationPermission::Allowed);
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
    /// 后台事务只持有纯值邮箱，不依赖 GPUI Entity 或原生通知对象。
    pub(crate) fn activity_notifications(&self) -> Arc<crate::notification_events::ActivityQueue> {
        self.context.activity_notifications.clone()
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
        let readable = {
            let progress = self.notifications.progress.borrow();
            !progress.today.failed
                && !progress.today.closed
                && !self.notifications.control.borrow().closing
                && self.notifications.progress.has_changed().is_ok()
        };
        if readable
            && epoch
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

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - notifications_worker.rs：账本持久化、发送意图发布与通知 worker 主循环，以及本模块单元测试
include!("notifications_worker.rs");
