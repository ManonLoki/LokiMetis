//! 真实 WinRT 对象回归，不登记 AUMID、不申请权限、不调用 Show。
use super::*;

/// 中文与 XML 特殊字符必须经真实 DOM 无损读回，并保留本产品通知身份。
#[test]
fn native_toast_preserves_text_and_identity_without_showing() {
    let _apartment = Apartment::new().unwrap();
    let toast = build_toast("windows-native-audit", "中文 <& 标题", "正文 & <内容>").unwrap();
    let text = toast.Content().unwrap().GetElementsByTagName(h!("text")).unwrap();
    assert_eq!(text.Length().unwrap(), 2);
    assert_eq!(text.Item(0).unwrap().InnerText().unwrap(), "中文 <& 标题");
    assert_eq!(text.Item(1).unwrap().InnerText().unwrap(), "正文 & <内容>");
    assert_eq!(toast.Tag().unwrap(), "windows-native-audit");
    assert_eq!(toast.Group().unwrap(), "LokiMetris");
    assert!(toast.SetTag(h!("")).is_err());
}

/// 已有 STA 初始化失败时不能擅自反初始化调用线程的 WinRT apartment。
#[test]
fn native_permission_preserves_existing_sta_on_apartment_failure() {
    std::thread::spawn(|| {
        use windows::Win32::{Foundation::RPC_E_CHANGED_MODE, System::WinRT::RO_INIT_SINGLETHREADED};
        // SAFETY: 在本测试独占的新线程上初始化 STA，结尾在同一线程配对反初始化。
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.unwrap();
        assert_eq!(Apartment::new().err().unwrap().code(), RPC_E_CHANGED_MODE);
        assert_eq!(permission(), NotificationPermission::Unavailable);
        assert!(XmlDocument::new().is_ok());
        // SAFETY: 与本线程开头成功的 RoInitialize 配对。
        unsafe { RoUninitialize() };
    }).join().unwrap();
}

/// 真正 COM delegate 的点击在失败队列满时仍保留一次，重复回调可合并。
#[test]
fn native_activation_survives_full_failure_queue() {
    let _apartment = Apartment::new().unwrap();
    let (events, receiver) = std::sync::mpsc::sync_channel(1);
    events.try_send(Event::LateFailure("full".into())).unwrap();
    let host = Host::new(events).unwrap();
    let handler = activation_handler(host.pending_click.clone());
    handler.Invoke(None::<&ToastNotification>, None::<&IInspectable>).unwrap();
    handler.Invoke(None::<&ToastNotification>, None::<&IInspectable>).unwrap();
    assert!(host.take_click());
    assert!(!host.take_click());
    assert!(matches!(receiver.try_recv(), Ok(Event::LateFailure(id)) if id == "full"));
    assert!(!host.overflow());
}

/// Toast 克隆仍存活时 Live 必须撤销两个系统事件持有的 delegate。
#[test]
fn native_live_drop_revokes_both_callbacks() {
    let _apartment = Apartment::new().unwrap();
    let toast = build_toast("windows-native-audit", "标题", "正文").unwrap();
    let activated_owner = Arc::new(());
    let failed_owner = Arc::new(());
    let retained = activated_owner.clone();
    let activated = toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, _| {
        let _ = &retained;
        Ok(())
    })).unwrap();
    let retained = failed_owner.clone();
    let failed = toast.Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(move |_, _| {
        let _ = &retained;
        Ok(())
    })).unwrap();
    let live = Live { toast: toast.clone(), activated, failed };
    assert_eq!(Arc::strong_count(&activated_owner), 2);
    assert_eq!(Arc::strong_count(&failed_owner), 2);
    drop(live);
    assert_eq!(Arc::strong_count(&activated_owner), 1);
    assert_eq!(Arc::strong_count(&failed_owner), 1);
    assert_eq!(toast.Tag().unwrap(), "windows-native-audit");
}
