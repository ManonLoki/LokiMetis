//! 弱槽位、原生协议对象与有界点击回归；不创建系统通知中心或应用进程。
use super::*;
use objc2::rc::Weak;
use std::{cell::{Cell, RefCell}, sync::mpsc};

/// 与系统 weak 属性相同的内存槽，写入不替 delegate 持有强引用。
#[derive(Default)]
struct Slot(RefCell<Option<Weak<ProtocolObject<dyn UNUserNotificationCenterDelegate>>>>);
impl Center for Slot {
    /// 真实 weak load 在桥接前返回临时强引用。
    fn get(&self) -> Option<Retained<ProtocolObject<dyn UNUserNotificationCenterDelegate>>> {
        self.0.borrow().as_ref().and_then(Weak::load)
    }
    /// weak store 不延长外部 owner 生命周期。
    fn set(&self, delegate: Option<&ProtocolObject<dyn UNUserNotificationCenterDelegate>>) {
        *self.0.borrow_mut() = delegate.map(Weak::new);
    }
}
/// 创建有真实 ivars 的隔离原生代理，避免伪造或初始化框架私有类。
fn delegate() -> Retained<NotificationDelegate> {
    let (clicked, _) = mpsc::sync_channel(1);
    let allocated = NotificationDelegate::alloc().set_ivars(DelegateIvars { clicked, previous: None });
    // 安全：与生产相同的完整 ivars 与 NSObject 指定初始化器。
    unsafe { msg_send![super(allocated), init] }
}
/// 真实 weak 槽位重现“框架先安装”；桥接强持有原代理并在结束时归还。
#[test]
fn notification_delegate_bridge_retains_and_restores_framework_owner() {
    let slot = Slot::default();
    let original = delegate();
    let original_weak = Weak::new(&*original);
    slot.set(Some(ProtocolObject::from_ref(&*original)));
    let (clicked, _) = mpsc::sync_channel(1);
    let lease = Lease::install_using(&slot, clicked, |_| true).unwrap();
    assert!(std::ptr::eq(&*slot.get().unwrap(), lease.protocol()));
    drop(original);
    assert!(original_weak.load().is_some(), "bridge owns the displaced weak delegate");
    lease.release(&slot);
    let restored = slot.get().unwrap();
    assert!(std::ptr::eq(&*restored as *const _ as *const (), &*original_weak.load().unwrap() as *const _ as *const ()));
    drop(lease);
    assert!(slot.get().is_some(), "restored owner retained by the original host/readback");
    drop(restored);
    assert!(slot.get().is_none());
}
/// 生产身份门禁拒绝未知代理，且释放不会覆盖后来安装的 owner。
#[test]
fn notification_delegate_rejects_unknown_and_preserves_later_owner() {
    let slot = Slot::default();
    let unknown = delegate();
    slot.set(Some(ProtocolObject::from_ref(&*unknown)));
    let (clicked, _) = mpsc::sync_channel(1);
    assert!(matches!(Lease::install(&slot, clicked), Err("delegate_conflict")));
    assert!(std::ptr::eq(&*slot.get().unwrap(), ProtocolObject::from_ref(&*unknown)));
    let (clicked, _) = mpsc::sync_channel(1);
    let lease = Lease::install_using(&slot, clicked, |_| true).unwrap();
    let later = delegate();
    slot.set(Some(ProtocolObject::from_ref(&*later)));
    lease.release(&slot);
    assert!(std::ptr::eq(&*slot.get().unwrap(), ProtocolObject::from_ref(&*later)));
}
/// 空槽位安装与移除遵守 weak 生命周期，回读失败不能谎报发送器已就绪。
#[test]
fn notification_delegate_empty_slot_and_failed_readback() {
    let slot = Slot::default();
    let (clicked, _) = mpsc::sync_channel(1);
    let lease = Lease::install(&slot, clicked).unwrap();
    lease.release(&slot);
    assert!(slot.get().is_none());
    /// 模拟系统拒绝写入代理，不能修改既有对象或虚构成功。
    struct RejectSlot;
    impl Center for RejectSlot {
        /// 回读始终为空。
        fn get(&self) -> Option<Retained<ProtocolObject<dyn UNUserNotificationCenterDelegate>>> { None }
        /// 模拟未接受的写入。
        fn set(&self, _: Option<&ProtocolObject<dyn UNUserNotificationCenterDelegate>>) {}
    }
    let (clicked, _) = mpsc::sync_channel(1);
    assert!(matches!(Lease::install(&RejectSlot, clicked), Err("delegate_readback")));
}
/// 真实 core tag 点击完成一次；满队列仍完成，Kit 与畸形 tag 只走原回调。
#[test]
fn notification_delegate_routes_owned_clicks_without_swallowing_kit() {
    let (clicked, rx) = mpsc::sync_channel(1);
    let ivars = DelegateIvars { clicked, previous: None };
    let done = Cell::new(0);
    let forwarded = Cell::new(0);
    let id = loki_metris_core::usage_milestones::milestone_request_id("2026-10-10", 3);
    for _ in 0..2 {
        ivars.response(&id, || done.set(done.get() + 1), || panic!("owned response must not reach Kit"));
    }
    assert_eq!(done.get(), 2);
    assert!(matches!(rx.try_recv(), Ok(super::super::driver::Event::Clicked)));
    assert!(rx.try_recv().is_err());
    for id in ["gpui-component-notification-1", "m1-not-a-full-id", "foreign"] {
        ivars.response(id, || panic!("foreign completion belongs to previous delegate"), || forwarded.set(forwarded.get() + 1));
    }
    assert_eq!(forwarded.get(), 3);
    assert_eq!(done.get(), 2);
    assert!(rx.try_recv().is_err());
}
/// 固定依赖的真实自动类名合同不能退化成短名、前缀匹配或接受未经审阅的新版本。
#[test]
fn notification_delegate_identity_requires_exact_reviewed_gpui_version() {
    assert!(supported_class_name(c"gpui_macos::system_notifications::NotificationResponseDelegate0.3.9"));
    for name in [c"NotificationResponseDelegate", c"foreign::NotificationResponseDelegate0.3.9", c"gpui_macos::system_notifications::NotificationResponseDelegate0.3.8", c"gpui_macos::system_notifications::NotificationResponseDelegate0.3.10"] {
        assert!(!supported_class_name(name));
    }
}
