//! 有完成回执的直接提交驱动；delegate 租约保留 GPUI Kit 已安装的回调。
use block2::RcBlock;
use loki_metris_core::usage_milestones::MilestoneOutcome;
use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_user_notifications::*;
use std::sync::{Arc, Mutex, mpsc::SyncSender};
use tokio::sync::oneshot;

/// 前台持有中心与桥接租约，退出只恢复仍由自身拥有的槽位。
pub(super) struct Host {
    /// 当前进程的通知中心。
    center: Retained<UNUserNotificationCenter>,
    /// 代理槽位租约；释放时只在槽位仍属于本应用时恢复原代理。
    lease: super::macos_delegate::Lease,
}
impl Host {
    /// GPUI didFinishLaunching 返回前协作桥接已知框架代理；未知代理仍失败关闭。
    pub fn new(clicked: SyncSender<super::driver::Event>) -> Option<Self> {
        if objc2::MainThreadMarker::new().is_none() || !super::macos::bundled() {
            tracing::warn!(
                event = "notification_host_unavailable",
                reason = "thread_or_bundle"
            );
            return None;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let lease = match super::macos_delegate::Lease::install(&*center, clicked) {
            Ok(lease) => lease,
            Err(reason) => {
                tracing::warn!(event = "notification_host_unavailable", reason);
                return None;
            }
        };
        tracing::info!(event = "notification_host_installed");
        Some(Self { center, lease })
    }
}
impl Drop for Host {
    /// 不移除其它后来取得的owner，避免退出覆盖外部delegate。
    fn drop(&mut self) {
        self.lease.release(&*self.center);
    }
}
/// 完成块只报告安排请求成功与否，不输出NSError描述或借用宿主对象。
pub(super) fn send(id: &str, title: &str, body: &str) -> oneshot::Receiver<MilestoneOutcome> {
    let (tx, rx) = oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(tx)));
    if !super::macos::bundled() {
        reply(&sender, MilestoneOutcome::Rejected);
        return rx;
    }
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(id),
        &content,
        None,
    );
    let block = RcBlock::new(move |error: *mut NSError| {
        let outcome = if error.is_null() {
            MilestoneOutcome::Accepted
        } else {
            // 安全：NSError仅在系统完成块期间有效，立即复制无载荷域与码。
            let error = unsafe { &*error };
            tracing::warn!(event="notification_send_rejected",domain=%error.domain(),code=error.code());
            MilestoneOutcome::Rejected
        };
        reply(&sender, outcome);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&block));
    rx
}
/// 崩溃恢复只在待发或通知中心找到相同ID时确认接受，找不到仍保持Unknown。
pub(super) async fn accepted(id: String) -> bool {
    if !super::macos::bundled() {
        return false;
    }
    let (tx, rx) = oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let pending = id.clone();
    let block = RcBlock::new(
        move |values: std::ptr::NonNull<NSArray<UNNotificationRequest>>| {
            // 安全：数组及元素在系统回调内有效，跨线程只交付bool。
            let found = unsafe { values.as_ref() }
                .iter()
                .any(|r| r.identifier().to_string() == pending);
            reply(&tx, found);
        },
    );
    UNUserNotificationCenter::currentNotificationCenter()
        .getPendingNotificationRequestsWithCompletionHandler(&block);
    if rx.await.unwrap_or(false) {
        return true;
    }
    let (tx, rx) = oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let block = RcBlock::new(move |values: std::ptr::NonNull<NSArray<UNNotification>>| {
        // 安全：只在完成块内枚举系统拥有的对象，返回纯值。
        let found = unsafe { values.as_ref() }
            .iter()
            .any(|n| n.request().identifier().to_string() == id);
        reply(&tx, found);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .getDeliveredNotificationsWithCompletionHandler(&block);
    rx.await.unwrap_or(false)
}
/// 每个系统回调最多完成一次，迟到结果在接收方已关闭时安全丢弃。
fn reply<T>(sender: &Mutex<Option<oneshot::Sender<T>>>, value: T) {
    if let Some(tx) = sender
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        let _ = tx.send(value);
    }
}
