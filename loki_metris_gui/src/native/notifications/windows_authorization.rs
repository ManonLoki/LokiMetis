//! 未打包应用只在显式授权时初始化首次通知；启动和后台权限查询继续只读。
use windows::{
    Foundation::{DateTime,IReference,PropertyValue},
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{NotificationSetting,ToastNotification,ToastNotificationManager,ToastNotifier},
    core::{HSTRING,Interface,h},
    Win32::Foundation::ERROR_NOT_FOUND,
};

const TAG:&str="lm-permission-v1";
const GROUP:&str="LokiMetrisInit";

/// 只有精确的首次身份缺失才初始化；拒绝、其它错误和失败重试都不能被解释为允许。
fn resolve(mut query:impl FnMut()->windows::core::Result<NotificationSetting>,initialize:impl FnOnce()->windows::core::Result<()>)
    ->windows::core::Result<NotificationSetting> {
    match query() {
        Err(error) if error.code()==ERROR_NOT_FOUND.to_hresult()=>{initialize()?;query()},
        result=>result,
    }
}

/// 仅显式开启或权限重试调用；最终权限仍来自系统，不预先假定允许。
pub(super) fn setting(aumid:&HSTRING,check:impl Fn()->windows::core::Result<()>)->windows::core::Result<NotificationSetting> {
    check()?;
    let notifier=ToastNotificationManager::CreateToastNotifierWithId(aumid)?;
    resolve(||{check()?;let result=notifier.Setting();check()?;result},||initialize(&notifier,aumid,&check))
}

/// Community Toolkit 同样先隐藏 Show 再删除，以解除首次 Setting 的 ERROR_NOT_FOUND 循环。
/// 来源：https://github.com/CommunityToolkit/WindowsCommunityToolkit/blob/main/Microsoft.Toolkit.Uwp.Notifications/Toasts/Compat/ToastNotificationManagerCompat.cs
/// 仅在 Windows 不再要求首次发送且实测确认后移除；清理失败仍有十五秒过期兜底。
fn initialize(notifier:&ToastNotifier,aumid:&HSTRING,check:&impl Fn()->windows::core::Result<()>)->windows::core::Result<()> {
    check()?;
    let unix=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_|windows::core::Error::from_hresult(windows::core::HRESULT(0x80070057_u32 as i32)))?.as_millis();
    let ticks=unix.checked_add(15_000).and_then(|value|value.checked_mul(10_000))
        .and_then(|value|value.checked_add(116_444_736_000_000_000)).and_then(|value|i64::try_from(value).ok())
        .ok_or_else(||windows::core::Error::from_hresult(windows::core::HRESULT(0x80070057_u32 as i32)))?;
    let toast=initialization_toast(DateTime{UniversalTime:ticks})?;
    submit_initialization(||notifier.Show(&toast),||ToastNotificationManager::History()?.RemoveGroupedTagWithId(&HSTRING::from(TAG),&HSTRING::from(GROUP),aumid),check)
}

/// 已提交的初始化必须清理，即使同步 Show 返回前外层已取消或超时。
fn submit_initialization(show:impl FnOnce()->windows::core::Result<()>,remove:impl FnOnce()->windows::core::Result<()>,check:&impl Fn()->windows::core::Result<()>)->windows::core::Result<()> {
    check()?;show()?;
    let removed=remove();check()?;removed
}

/// 专属短 tag/group 不占用里程碑身份，禁止弹出且显式短期过期。
fn initialization_toast(expiration:DateTime)->windows::core::Result<ToastNotification> {
    let document=XmlDocument::new()?;
    document.LoadXml(h!("<toast><visual><binding template=\"ToastGeneric\"><text>LokiMetris</text></binding></visual><audio silent=\"true\"/></toast>"))?;
    let toast=ToastNotification::CreateToastNotification(&document)?;
    toast.SetTag(&HSTRING::from(TAG))?;toast.SetGroup(&HSTRING::from(GROUP))?;
    toast.SetSuppressPopup(true)?;
    let value:IReference<DateTime>=PropertyValue::CreateDateTime(expiration)?.cast()?;
    toast.SetExpirationTime(&value)?;
    Ok(toast)
}

#[cfg(test)]
#[path="windows_authorization_tests.rs"]
mod tests;
