//! 仅桥接本版本 GPUI 的已知代理；Kit 的非里程碑回调、可选方法和原槽位均保留。
use block2::DynBlock;
use objc2::{AnyThread, DefinedClass, define_class, msg_send, rc::Retained, runtime::ProtocolObject, sel};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_user_notifications::*;
use std::sync::mpsc::SyncSender;

/// 原生中心的弱槽位接口，隔离回归使用内存槽位而不触及系统通知。
pub(super) trait Center {
    /// 读取时取得强引用，防止协作代理在桥接期间释放。
    fn get(&self) -> Option<Retained<ProtocolObject<dyn UNUserNotificationCenterDelegate>>>;
    /// 安装或恢复同一个协议对象。
    fn set(&self, delegate: Option<&ProtocolObject<dyn UNUserNotificationCenterDelegate>>);
}
impl Center for UNUserNotificationCenter {
    /// 转发真实 weak 属性读取。
    fn get(&self) -> Option<Retained<ProtocolObject<dyn UNUserNotificationCenterDelegate>>> {
        self.delegate()
    }
    /// 转发真实 weak 属性写入。
    fn set(&self, delegate: Option<&ProtocolObject<dyn UNUserNotificationCenterDelegate>>) {
        self.setDelegate(delegate);
    }
}
/// 回调只发送有界纯值；既有 GPUI 代理被强持有并继续处理自己的通知。
struct DelegateIvars {
    clicked: SyncSender<super::driver::Event>,
    previous: Option<Retained<ProtocolObject<dyn UNUserNotificationCenterDelegate>>>,
}
impl DelegateIvars {
    /// 当前协议仅匹配 core 生成的 m1 命名空间及完整十六进制 ID，不能劫持 Kit tag。
    fn response(&self, id: &str, done: impl FnOnce(), foreign: impl FnOnce()) {
        if owns(id) {
            let _ = self.clicked.try_send(super::driver::Event::Clicked);
            done();
        } else {
            foreign();
        }
    }
}
/// 原生路由只识别本应用既有稳定 tag 格式，不参与业务去重。
fn owns(id: &str) -> bool {
    let Some((day, bucket)) = id.strip_prefix("m1-").and_then(|v| v.split_once('-')) else {
        return false;
    };
    day.len() == 16 && bucket.len() == 16
        && day.bytes().chain(bucket.bytes()).all(|b| b.is_ascii_hexdigit())
}
/// objc2 0.6 的自动类名包含模块和包版本；只协作当前固定归档中审阅过的实现。
fn supported_class_name(name: &std::ffi::CStr) -> bool {
    name == c"gpui_macos::system_notifications::NotificationResponseDelegate0.3.8"
}
define_class!(
    // 安全：NSObject 无额外不变量；仅并发访问有界线程安全 sender 与已知 GPUI 线程安全代理。
    #[unsafe(super(NSObject))]
    #[name = "LokiMetrisMilestoneNotificationDelegate"]
    #[ivars = DelegateIvars]
    /// 里程碑的原生桥接代理，不替换 Kit 注册的 App 回调。
    struct NotificationDelegate;
    // SAFETY: NotificationDelegate 由 define_class! 声明为 NSObject 子类，满足 NSObjectProtocol 的要求。
    unsafe impl NSObjectProtocol for NotificationDelegate {}
    // SAFETY: 下面实现的方法签名与 UNUserNotificationCenterDelegate 对应选择子的 Objective-C 类型一致。
    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        /// 本应用点击返回主线程，其它通知原样交给 GPUI，完成块只调用一次。
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn response(&self, center: &UNUserNotificationCenter, response: &UNNotificationResponse, done: &DynBlock<dyn Fn()>) {
            self.ivars().response(&response.notification().request().identifier().to_string(), || done.call(()), || {
                if let Some(previous) = self.ivars().previous.as_ref().filter(|p| p.respondsToSelector(sel!(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))) {
                    previous.userNotificationCenter_didReceiveNotificationResponse_withCompletionHandler(center, response, done);
                } else {
                    done.call(());
                }
            });
        }
        /// 里程碑前台静音展示，非里程碑保留框架的展示决定。
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(&self, center: &UNUserNotificationCenter, notification: &UNNotification, done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
            if owns(&notification.request().identifier().to_string()) {
                done.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
            } else if let Some(previous) = self.ivars().previous.as_ref().filter(|p| p.respondsToSelector(sel!(userNotificationCenter:willPresentNotification:withCompletionHandler:))) {
                previous.userNotificationCenter_willPresentNotification_withCompletionHandler(center, notification, done);
            } else {
                done.call((UNNotificationPresentationOptions::empty(),));
            }
        }
        /// 保留协议的可选系统设置回调，不将其误作通知点击。
        #[unsafe(method(userNotificationCenter:openSettingsForNotification:))]
        fn settings(&self, center: &UNUserNotificationCenter, notification: Option<&UNNotification>) {
            if let Some(previous) = self.ivars().previous.as_ref().filter(|p| p.respondsToSelector(sel!(userNotificationCenter:openSettingsForNotification:))) {
                previous.userNotificationCenter_openSettingsForNotification(center, notification);
            }
        }
    }
);
/// weak 槽位的单 owner 租约；原代理由 ivars 持有，恢复前后都不会悬空。
pub(super) struct Lease {
    delegate: Retained<NotificationDelegate>,
}
impl Lease {
    /// 只允许空槽位或本版本已审阅的 GPUI 代理，未知代理不写入。
    pub fn install(center: &impl Center, clicked: SyncSender<super::driver::Event>) -> Result<Self, &'static str> {
        Self::install_using(center, clicked, |delegate| {
            let object: &objc2::runtime::AnyObject = delegate.as_ref();
            supported_class_name(object.class().name())
        })
    }
    /// 同一生产安装路径允许回归注入已知代理身份，不启动真实应用或通知中心。
    fn install_using(center: &impl Center, clicked: SyncSender<super::driver::Event>, supported: impl FnOnce(&ProtocolObject<dyn UNUserNotificationCenterDelegate>) -> bool) -> Result<Self, &'static str> {
        let previous = center.get();
        if previous.as_ref().is_some_and(|p| !supported(p)) {
            return Err("delegate_conflict");
        }
        let allocated = NotificationDelegate::alloc().set_ivars(DelegateIvars { clicked, previous });
        // 安全：NSObject.init 是指定初始化器，ivars 已完整建立。
        let delegate: Retained<NotificationDelegate> = unsafe { msg_send![super(allocated), init] };
        let lease = Self { delegate };
        center.set(Some(lease.protocol()));
        if !center.get().is_some_and(|current| std::ptr::eq(&*current, lease.protocol())) {
            // 槽位若被别的 owner 替换，不覆盖它；若仍为自身则归还。
            lease.release(center);
            return Err("delegate_readback");
        }
        Ok(lease)
    }
    /// 读取协议视图只借用自身强引用。
    fn protocol(&self) -> &ProtocolObject<dyn UNUserNotificationCenterDelegate> {
        ProtocolObject::from_ref(&*self.delegate)
    }
    /// 只归还仍由自己持有的槽位；后来替换的代理保持原样。
    pub fn release(&self, center: &impl Center) {
        if center.get().is_some_and(|current| std::ptr::eq(&*current, self.protocol())) {
            center.set(self.delegate.ivars().previous.as_deref());
        }
    }
}

#[cfg(test)]
#[path = "macos_delegate_tests.rs"]
mod tests;
