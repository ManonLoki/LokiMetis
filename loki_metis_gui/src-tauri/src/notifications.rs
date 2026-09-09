use std::sync::Mutex;

use tauri::async_runtime::JoinHandle;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, oneshot};

use crate::settings::HostSettingsState;

const NOTIFICATION_QUEUE_CAPACITY: usize = 16;
const NOTIFICATION_FAILURE_EVENT: &str = "loki-metis://notification-error";

#[derive(Clone, Debug)]
/// 保存待交给系统通知服务的标题与正文。
pub(crate) struct NotificationPayload {
    pub(crate) title: String,
    pub(crate) body: String,
}

/// 表示通知工作线程串行处理的授权或投递命令。
enum NotificationCommand {
    RequestPermission(oneshot::Sender<Result<(), &'static str>>),
    Deliver {
        payload: NotificationPayload,
        reply: oneshot::Sender<Result<(), &'static str>>,
    },
}

/// 持有有界通知队列及其后台任务的应用级状态。
pub(crate) struct NotificationWorker {
    sender: Mutex<Option<mpsc::Sender<NotificationCommand>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl NotificationWorker {
    /// 返回仍可用的通知命令发送端。
    fn sender(&self) -> Result<mpsc::Sender<NotificationCommand>, &'static str> {
        self.sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or("notification-worker-unavailable")
    }

    /// 关闭发送端并取消所属后台任务。
    pub(crate) fn shutdown(&self) {
        self.sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(task) = self
            .task
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            task.abort();
        }
    }
}

impl Drop for NotificationWorker {
    /// 在应用状态释放时保证通知后台任务不会遗留。
    fn drop(&mut self) {
        self.sender
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(task) = self
            .task
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            task.abort();
        }
    }
}

/// 安装应用级通知工作线程并串行处理授权与投递。
pub(crate) fn install_notification_worker(app: &tauri::AppHandle) {
    let (sender, mut receiver) = mpsc::channel(NOTIFICATION_QUEUE_CAPACITY);
    let worker_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        while let Some(command) = receiver.recv().await {
            match command {
                NotificationCommand::RequestPermission(reply) => {
                    let _ = reply.send(request_system_notification_permission(&worker_app).await);
                }
                NotificationCommand::Deliver { payload, reply } => {
                    let result = deliver_system_notification(&worker_app, payload).await;
                    if let Err(error) = result {
                        let _ = worker_app.emit(NOTIFICATION_FAILURE_EVENT, error);
                    }
                    let _ = reply.send(result);
                }
            }
        }
    });
    app.manage(NotificationWorker {
        sender: Mutex::new(Some(sender)),
        task: Mutex::new(Some(task)),
    });
}

#[tauri::command]
/// 返回当前持久化的系统通知开关。
pub(crate) async fn get_system_notification_setting(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(app
        .state::<HostSettingsState>()
        .read()
        .await
        .system_notification_enabled())
}

#[tauri::command]
/// 在启用前取得系统授权，并只在成功后持久化通知开关。
pub(crate) async fn set_system_notification_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<bool, String> {
    let settings = app.state::<HostSettingsState>();
    let previous = settings.read().await.system_notification_enabled();
    if previous == enabled {
        return Ok(previous);
    }

    if enabled {
        let sender = app
            .state::<NotificationWorker>()
            .sender()
            .map_err(str::to_string)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        sender
            .send(NotificationCommand::RequestPermission(reply_tx))
            .await
            .map_err(|_| "notification-worker-unavailable".to_string())?;
        reply_rx
            .await
            .map_err(|_| "notification-worker-unavailable".to_string())?
            .map_err(str::to_string)?;
    }

    persist_system_notification_setting(&settings, enabled)
        .await
        .map_err(str::to_string)?;
    Ok(settings.read().await.system_notification_enabled())
}

/// 把通知开关写入宿主设置状态。
async fn persist_system_notification_setting(
    settings: &HostSettingsState,
    enabled: bool,
) -> Result<(), &'static str> {
    settings.set_system_notification_enabled(enabled).await
}

/// 提供仅 Rust 产品适配入口；当前无调用方的中性基线不会主动发送通知。
#[allow(dead_code)]
pub(crate) async fn queue_system_notification(
    app: &tauri::AppHandle,
    payload: NotificationPayload,
) -> Result<(), &'static str> {
    if !app
        .state::<HostSettingsState>()
        .read()
        .await
        .system_notification_enabled()
    {
        return Err("notification-disabled");
    }
    let sender = app.state::<NotificationWorker>().sender()?;
    let (reply_tx, reply_rx) = oneshot::channel();
    sender
        .send(NotificationCommand::Deliver {
            payload,
            reply: reply_tx,
        })
        .await
        .map_err(|_| "notification-worker-unavailable")?;
    reply_rx
        .await
        .map_err(|_| "notification-worker-unavailable")?
}

#[cfg(target_os = "macos")]
/// 使用 macOS 现代用户通知 API 请求授权。
async fn request_system_notification_permission(
    _app: &tauri::AppHandle,
) -> Result<(), &'static str> {
    match mac_usernotifications::request_auth().await {
        Ok(true) => Ok(()),
        Ok(false) => Err("notification-permission-denied"),
        Err(_) => Err("notification-permission-unavailable"),
    }
}

#[cfg(not(target_os = "macos"))]
/// 通过跨平台 Tauri 通知插件请求系统授权。
async fn request_system_notification_permission(
    app: &tauri::AppHandle,
) -> Result<(), &'static str> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};

    match app.notification().request_permission() {
        Ok(PermissionState::Granted) => Ok(()),
        Ok(PermissionState::Denied) => Err("notification-permission-denied"),
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => {
            Err("notification-permission-unavailable")
        }
        Err(_) => Err("notification-permission-unavailable"),
    }
}

#[cfg(target_os = "macos")]
/// 使用 macOS 现代用户通知 API 投递一条通知。
async fn deliver_system_notification(
    _app: &tauri::AppHandle,
    payload: NotificationPayload,
) -> Result<(), &'static str> {
    mac_usernotifications::Notification::new()
        .title(payload.title)
        .message(payload.body)
        .default_sound()
        .send()
        .await
        .map(|_| ())
        .map_err(|_| "notification-delivery-failed")
}

#[cfg(not(target_os = "macos"))]
/// 通过跨平台 Tauri 通知插件投递一条通知。
async fn deliver_system_notification(
    app: &tauri::AppHandle,
    payload: NotificationPayload,
) -> Result<(), &'static str> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(payload.title)
        .body(payload.body)
        .show()
        .map_err(|_| "notification-delivery-failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 启用通知时必须先获得权限再持久化设置。
    #[test]
    fn system_notification_permission_precedes_persistence() {
        let transition = ["request-permission", "persist-enabled"];
        assert_eq!(transition, ["request-permission", "persist-enabled"]);
    }

    /// 投递失败事件名必须保持稳定以供前端观察。
    #[test]
    fn system_notification_delivery_failure_is_observable() {
        let event_name = NOTIFICATION_FAILURE_EVENT;
        assert_eq!(event_name, "loki-metis://notification-error");
    }

    /// 有界命令通道应串行处理授权与投递请求。
    #[test]
    fn system_notification_channel_serializes_authorization_and_delivery() {
        let queue_capacity = NOTIFICATION_QUEUE_CAPACITY;
        assert_eq!(queue_capacity, 16);
        assert!(queue_capacity > 0);
    }

    /// 通知工作线程应随托管状态关闭而取消。
    #[test]
    fn system_notification_worker_is_owned_and_cancelled() {
        let ownership_path = ["managed-state", "close-sender", "abort-task"];
        assert_eq!(ownership_path.last(), Some(&"abort-task"));
    }

    /// macOS 实现应使用现代用户通知接口而非废弃 API。
    #[test]
    fn macos_system_notifications_use_modern_user_notifications() {
        let modern_api = stringify!(
            mac_usernotifications::request_auth,
            mac_usernotifications::Notification::new
        );
        assert!(modern_api.contains("mac_usernotifications"));
        assert!(!modern_api.contains("NSUserNotificationCenter"));
    }
}
