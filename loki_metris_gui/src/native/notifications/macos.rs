//! 前台发起 UserNotifications 调用，系统任意队列上的回调只复制状态。

use super::Error;
use block2::RcBlock;
use gpui_kit::BackgroundExecutor;
use objc2_foundation::{NSBundle, NSError};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNNotificationSettings, UNUserNotificationCenter,
};
use std::{
    ptr::NonNull,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::oneshot;

use loki_metris_core::usage_milestones::NotificationPermission as Permission;

/// 当前进程必须拥有本产品的正式 bundle 身份。
pub(super) fn bundled() -> bool {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some_and(|id| id.to_string() == "com.loki-metris.app")
}
/// 只读权限查询不会触发首次授权弹窗。
pub(super) async fn permission() -> Permission {
    if objc2::MainThreadMarker::new().is_none() || !bundled() {
        return Permission::Unavailable;
    }
    query().await.unwrap_or(Permission::Unavailable)
}

/// 裸二进制缺少本产品 bundle 时拒绝调用可抛 Objective-C 异常的通知中心。
pub(super) async fn authorize(
    executor: BackgroundExecutor,
    deadline: Instant,
) -> Result<(), Error> {
    if objc2::MainThreadMarker::new().is_none() || !bundled() {
        return Err(Error::Unavailable);
    }
    before_request(&executor, deadline)?;
    let mut permission = query().await.map_err(|_| Error::Unavailable)?;
    if permission == Permission::NotDetermined {
        before_request(&executor, deadline)?;
        request().await.map_err(|_| Error::Unavailable)??;
        before_request(&executor, deadline)?;
        permission = query().await.map_err(|_| Error::Unavailable)?;
    }
    match permission {
        Permission::Allowed => Ok(()),
        Permission::Denied => Err(Error::Denied),
        _ => {
            tracing::warn!(event = "notification_permission_unknown");
            Err(Error::Unavailable)
        }
    }
}

/// 主循环晚唤醒时也不能在总期限后发起下一步系统查询或授权请求。
fn before_request(executor: &BackgroundExecutor, deadline: Instant) -> Result<(), Error> {
    if executor.now() >= deadline {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}

/// 回调所有权在系统保留的 block 内；迟到响应不能引用 App、Window 或偏好 writer。
fn query() -> oneshot::Receiver<Permission> {
    let (sender, receiver) = oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // 系统回调参数在回调期间有效，立即复制枚举，不能跨线程保留其借用。
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        let permission = match status {
            UNAuthorizationStatus::NotDetermined => Permission::NotDetermined,
            UNAuthorizationStatus::Authorized
            | UNAuthorizationStatus::Provisional
            | UNAuthorizationStatus::Ephemeral => Permission::Allowed,
            UNAuthorizationStatus::Denied => Permission::Denied,
            _ => Permission::Unavailable,
        };
        reply(&sender, permission);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .getNotificationSettingsWithCompletionHandler(&block);
    receiver
}

/// 完成回执保留 granted 和错误；随后只读查询才确认系统权限，不决定本地开关。
fn request() -> oneshot::Receiver<Result<(), Error>> {
    let (sender, receiver) = oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let block = RcBlock::new(move |granted: objc2::runtime::Bool, error: *mut NSError| {
        let result = if error.is_null() {
            authorization_response(granted.as_bool(), None)
        } else {
            // NSError仅在完成块内有效；只记录平台域和数值码，不输出描述或userInfo。
            let error = unsafe { &*error };
            tracing::warn!(
                event = "notification_authorization_failed",
                domain = %error.domain(),
                code = error.code(),
            );
            authorization_response(
                granted.as_bool(),
                Some((&error.domain().to_string(), error.code())),
            )
        };
        reply(&sender, result);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &block,
        );
    receiver
}

/// SDK UNError.h 的 NotificationsNotAllowed=1 表示系统不允许；不能推断用户拒绝或签名原因。
/// 官方语义：https://developer.apple.com/documentation/usernotifications/unerror/code/notificationsnotallowed
fn authorization_response(granted: bool, error: Option<(&str, isize)>) -> Result<(), Error> {
    match error {
        Some(("UNErrorDomain", 1)) => Err(Error::Denied),
        Some(_) => Err(Error::Unavailable),
        None if granted => Ok(()),
        None => Err(Error::Denied),
    }
}

/// 每个系统完成块最多交付一次；接收方已取消时丢弃纯值即可。
fn reply<T>(sender: &Mutex<Option<oneshot::Sender<T>>>, value: T) {
    if let Some(sender) = sender
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        let _ = sender.send(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 系统不允许必须保留拒绝类别；未知域或码失败关闭，不能用 granted 掩盖 NSError。
    #[test]
    fn notification_authorization_maps_domain_code_and_granted() {
        assert_eq!(
            authorization_response(false, Some(("UNErrorDomain", 1))),
            Err(Error::Denied)
        );
        assert_eq!(
            authorization_response(true, Some(("UNErrorDomain", 1))),
            Err(Error::Denied)
        );
        assert_eq!(
            authorization_response(false, Some(("NSCocoaErrorDomain", 1))),
            Err(Error::Unavailable)
        );
        assert_eq!(
            authorization_response(false, Some(("UNErrorDomain", 99))),
            Err(Error::Unavailable)
        );
        assert_eq!(authorization_response(false, None), Err(Error::Denied));
        assert_eq!(authorization_response(true, None), Ok(()));
    }
}
