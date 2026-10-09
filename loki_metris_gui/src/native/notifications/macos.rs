//! 直接查询 UserNotifications 授权，不替换 GPUI delegate；后台回调只复制状态。

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
    NSBundle::mainBundle().bundleIdentifier().is_some_and(|id| id.to_string()=="com.loki-metris.app")
}
/// 只读权限查询不会触发首次授权弹窗。
pub(super) async fn permission() -> Permission {
    if !bundled() {return Permission::Unavailable;}
    query().await.unwrap_or(Permission::Unavailable)
}

/// 裸二进制缺少本产品 bundle 时拒绝调用可抛 Objective-C 异常的通知中心。
pub(super) async fn authorize(
    executor: BackgroundExecutor,
    deadline: Instant,
) -> Result<(), Error> {
    if !bundled() {return Err(Error::Unavailable);}
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

/// 完成回执只确认 API 调用，最终是否可保存开启仍由随后查询的授权状态决定。
fn request() -> oneshot::Receiver<Result<(), Error>> {
    let (sender, receiver) = oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let block = RcBlock::new(move |_: objc2::runtime::Bool, error: *mut NSError| {
        reply(
            &sender,
            if error.is_null() {
                Ok(())
            } else {
                // NSError仅在完成块内有效；只记录平台域和数值码，不输出描述或userInfo。
                let error = unsafe { &*error };
                tracing::warn!(
                    event = "notification_authorization_failed",
                    domain = %error.domain(),
                    code = error.code(),
                );
                Err(Error::Unavailable)
            },
        );
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &block,
        );
    receiver
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
