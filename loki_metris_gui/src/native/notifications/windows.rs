//! Windows Toast 使用显式产品 AUMID，只有用户开启动作登记 HKCU；回调仅传纯值。
use super::{Error, driver::Event};
use loki_metris_core::usage_milestones::{MilestoneOutcome, NotificationPermission};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
};
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        NotificationSetting, ToastFailedEventArgs, ToastNotification, ToastNotificationManager,
    },
    Win32::System::{
        Registry::*,
        WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    },
    core::{HSTRING, IInspectable, PCWSTR, h, w},
};
const AUMID: &str = "com.loki-metris.app";
const KEY: PCWSTR = w!(r"Software\Classes\AppUserModelId\com.loki-metris.app");
#[path="windows_authorization.rs"]
mod authorization;
/// 每个后台岛平衡 WinRT 初始化；线程已初始化时不擅自改变 apartment。
struct Apartment;
impl Apartment {
    /// 本后台执行器线程只建立 MTA，失败交由上层呈现 unavailable。
    fn new() -> windows::core::Result<Self> {
        // SAFETY: 在当前线程初始化 WinRT 多线程 apartment，不涉及指针；成功时由 Apartment 负责配对反初始化。
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    /// 只平衡本次成功初始化，不退出其它线程的apartment。
    fn drop(&mut self) {
        // SAFETY: 只有 RoInitialize 成功才会构造 Apartment，因此这里与本线程的那次初始化一一配对。
        unsafe { RoUninitialize() }
    }
}
/// 保留两类事件注册令牌，淘汰或退出时均撤销。
struct Live {
    toast: ToastNotification,
    activated: i64,
    failed: i64,
}
impl Drop for Live {
    /// 不隐藏已提交 Toast；只解除进程内监听。
    fn drop(&mut self) {
        let _ = self.toast.RemoveActivated(self.activated);
        let _ = self.toast.RemoveFailed(self.failed);
    }
}
/// 上限128个活动回调对象，不能随长期 Token 消费无限增长。
#[derive(Clone)]
pub(super) struct Host {
    events: SyncSender<Event>,
    live: Arc<Mutex<VecDeque<Live>>>,
    overflow: Arc<AtomicBool>,
    pending_click: Arc<AtomicBool>,
}
impl Host {
    /// 建立进程内所有权，不写注册表、不申请权限。
    pub fn new(events: SyncSender<Event>) -> Option<Self> {
        Some(Self {
            events,
            live: Default::default(),
            overflow: Default::default(),
            pending_click: Default::default(),
        })
    }
    /// 有界回调队列溢出由应用显式披露并暂停。
    pub fn overflow(&self) -> bool {
        self.overflow.swap(false, Ordering::AcqRel)
    }
    /// 点击独立合并，不因失败回执占满有界队列而遗失恢复主窗的唯一请求。
    pub fn take_click(&self) -> bool {
        self.pending_click.swap(false, Ordering::AcqRel)
    }
    /// Show 返回成功才确认 API 接受，Failed 事件以后只重排同档。
    pub fn send(&self, id: &str, title: &str, body: &str) -> MilestoneOutcome {
        let result = (|| -> windows::core::Result<()> {
            let _apartment = Apartment::new()?;
            if !registered() {
                return Err(windows::core::Error::from_thread());
            }
            let notifier =
                ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?;
            if notifier.Setting()? != NotificationSetting::Enabled {
                return Err(windows::core::Error::from_thread());
            }
            let toast = build_toast(id, title, body)?;
            let activated = toast.Activated(&activation_handler(self.pending_click.clone()))?;
            let events = self.events.clone();
            let id = id.to_owned();
            let overflow = self.overflow.clone();
            let failed = match toast.Failed(&TypedEventHandler::<
                ToastNotification,
                ToastFailedEventArgs,
            >::new(move |_, _| {
                if events.try_send(Event::LateFailure(id.clone())).is_err() {
                    overflow.store(true, Ordering::Release);
                }
                Ok(())
            })) {
                Ok(token) => token,
                Err(error) => {
                    let _ = toast.RemoveActivated(activated);
                    return Err(error);
                }
            };
            let live = Live {
                toast,
                activated,
                failed,
            };
            notifier.Show(&live.toast)?;
            let mut retained = self
                .live
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if retained.len() == 128 {
                retained.pop_front();
            }
            retained.push_back(live);
            Ok(())
        })();
        if result.is_ok() {
            MilestoneOutcome::Accepted
        } else {
            MilestoneOutcome::Rejected
        }
    }
}
/// 只构造 WinRT 通知对象；正文经 DOM 文本节点转义，不提交系统通知。
fn build_toast(id: &str, title: &str, body: &str) -> windows::core::Result<ToastNotification> {
    let document = XmlDocument::new()?;
    let toast_node = document.CreateElement(h!("toast"))?;
    document.AppendChild(&toast_node)?;
    let visual = document.CreateElement(h!("visual"))?;
    toast_node.AppendChild(&visual)?;
    let binding = document.CreateElement(h!("binding"))?;
    binding.SetAttribute(h!("template"), h!("ToastGeneric"))?;
    visual.AppendChild(&binding)?;
    for text in [title, body] {
        let element = document.CreateElement(h!("text"))?;
        element.SetInnerText(&HSTRING::from(text))?;
        binding.AppendChild(&element)?;
    }
    let audio = document.CreateElement(h!("audio"))?;
    audio.SetAttribute(h!("silent"), h!("true"))?;
    toast_node.AppendChild(&audio)?;
    let toast = ToastNotification::CreateToastNotification(&document)?;
    toast.SetTag(&HSTRING::from(id))?;
    toast.SetGroup(h!("LokiMetris"))?;
    Ok(toast)
}
/// COM 回调仅置纯值位，重复点击合并；前台每次消费一次并负责恢复窗口。
fn activation_handler(pending: Arc<AtomicBool>) -> TypedEventHandler<ToastNotification, IInspectable> {
    TypedEventHandler::new(move |_, _| {
        pending.store(true, Ordering::Release);
        Ok(())
    })
}
/// 读取本产品 DisplayName，拒绝登记不完整或身份漂移；不改系统状态。
fn registered() -> bool {
    let mut bytes = [0_u16; 128];
    let mut length = std::mem::size_of_val(&bytes) as u32;
    // SAFETY: 读取当前用户下固定键的 REG_SZ；输出缓冲是本栈数组，length 以字节传入其真实大小，返回后再校验长度。
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            w!("DisplayName"),
            RRF_RT_REG_SZ,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut length),
        )
    };
    result.is_ok()
        && length.is_multiple_of(2)
        && length >= 2
        && length as usize <= std::mem::size_of_val(&bytes)
        && String::from_utf16_lossy(&bytes[..length as usize / 2 - 1]) == "LokiMetris"
}
/// 仅显式开启动作登记身份并初始化首次权限；错误返回给上层独立呈现。
pub(super) fn authorize(control:&super::driver::Authorization) -> Result<(), Error> {
    control.check()?;
    let _apartment = Apartment::new().map_err(|_| Error::Unavailable)?;
    if registered() {
        return authorized_permission(control);
    }
    control.check()?;
    let mut key = HKEY::default();
    // SAFETY: 在当前用户下创建或打开固定键；输出句柄写入本栈变量，成功后的每条路径都会关闭它。
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            None,
            &mut key,
            None,
        )
    }
    .ok()
    .map_err(|_| Error::Unavailable)?;
    // SAFETY: key 是上面成功创建的句柄，关闭后立即返回、不再使用。
    if let Err(error)=control.check() {let _=unsafe {RegCloseKey(key)};return Err(error)}
    let name: Vec<u16> = "LokiMetris\0".encode_utf16().collect();
    // 安全：REG_SZ 读取当前活 Vec 的完整UTF-16字节，调用期间内存不变。
    let bytes = unsafe { std::slice::from_raw_parts(name.as_ptr().cast(), name.len() * 2) };
    let result = unsafe { RegSetValueExW(key, w!("DisplayName"), None, REG_SZ, Some(bytes)) }.ok();
    let _ = unsafe { RegCloseKey(key) };
    result.map_err(|_| Error::Unavailable)?;
    if !registered() {return Err(Error::Unavailable)}
    authorized_permission(control)
}
/// 显式授权可初始化尚无系统记录的身份，始终以实际查询结果区分拒绝和不可用。
fn authorized_permission(control:&super::driver::Authorization)->Result<(),Error> {
    let result=authorization::setting(&HSTRING::from(AUMID),||control.check()
        .map_err(|_|windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(1223))));
    control.check()?;
    match result {
        Ok(NotificationSetting::Enabled)=>Ok(()),
        Ok(_)=>Err(Error::Denied),
        Err(_)=>Err(Error::Unavailable),
    }
}
/// 启动与恢复只查询当前设置，缺登记不会偷偷写 HKCU。
pub(super) fn permission() -> NotificationPermission {
    let Ok(_apartment) = Apartment::new() else {
        return NotificationPermission::Unavailable;
    };
    if !registered() {
        return NotificationPermission::Unavailable;
    }
    match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))
        .and_then(|n| n.Setting())
    {
        Ok(NotificationSetting::Enabled) => NotificationPermission::Allowed,
        Ok(_) => NotificationPermission::Denied,
        Err(_) => NotificationPermission::Unavailable,
    }
}
/// 历史仅确认仍保留的本产品 tag；已清除或过期记录不能证明未提交。
pub(super) fn accepted(id: &str) -> bool {
    let Ok(_apartment) = Apartment::new() else {
        return false;
    };
    ToastNotificationManager::History()
        .and_then(|h| h.GetHistoryWithId(&HSTRING::from(AUMID)))
        .is_ok_and(|values| {
            values.into_iter().any(|t| {
                t.Tag().is_ok_and(|tag| tag == id) && t.Group().is_ok_and(|g| g == "LokiMetris")
            })
        })
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
