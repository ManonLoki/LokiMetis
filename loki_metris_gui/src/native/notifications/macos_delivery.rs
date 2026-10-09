//! 应用唯一 UserNotifications delegate；不调用 GPUI 懒初始化 sender，也不覆盖活 owner。
use block2::{DynBlock, RcBlock};
use loki_metris_core::usage_milestones::MilestoneOutcome;
use objc2::{
    AnyThread, DefinedClass, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::*;
use std::sync::{Arc, Mutex, mpsc::SyncSender};
use tokio::sync::oneshot;

/// delegate 的跨线程输入只包含有界点击信号，绝不借用 UI。
struct DelegateIvars {
    clicked: SyncSender<super::driver::Event>,
}
define_class!(
    // 安全：NSObject 无额外子类不变量；delegate 不实现 Drop，ivars 由 objc2 管理。
    #[unsafe(super(NSObject))]
    #[name = "LokiMetrisMilestoneNotificationDelegate"]
    #[ivars = DelegateIvars]
    /// 应用唯一通知中心代理。
    struct NotificationDelegate;
    unsafe impl NSObjectProtocol for NotificationDelegate {}
    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        /// 点击只送一个恢复信号，真正窗口激活回到 GPUI 主线程。
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn response(
            &self,
            _: &UNUserNotificationCenter,
            _: &UNNotificationResponse,
            done: &DynBlock<dyn Fn()>,
        ) {
            let _ = self.ivars().clicked.try_send(super::driver::Event::Clicked);
            done.call(());
        }
        /// 前台显式选择静音 banner/list，API接受与可见效果仍独立验收。
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(
            &self,
            _: &UNUserNotificationCenter,
            _: &UNNotification,
            done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            done.call((
                UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,
            ));
        }
    }
);
/// 前台持有弱 delegate 属性的真实生命周期，退出只移除自身所有权。
pub(super) struct Host {
    delegate: Retained<NotificationDelegate>,
}
impl Host {
    /// 裸binary或活delegate冲突时失败关闭，启动不申请权限。
    pub fn new(clicked: SyncSender<super::driver::Event>) -> Option<Self> {
        if !super::macos::bundled() {
            return None;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        if center.delegate().is_some() {
            return None;
        }
        let allocated = NotificationDelegate::alloc().set_ivars(DelegateIvars { clicked });
        // 安全：NSObject.init 是指定初始化器，ivars 已完整建立。
        let delegate: Retained<NotificationDelegate> = unsafe { msg_send![super(allocated), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        Some(Self { delegate })
    }
}
impl Drop for Host {
    /// 不移除其它后来取得的owner，避免退出覆盖外部delegate。
    fn drop(&mut self) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        if center.delegate().is_some_and(|d| {
            std::ptr::eq(
                &*d as *const _ as *const (),
                &*self.delegate as *const _ as *const (),
            )
        }) {
            center.setDelegate(None);
        }
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
