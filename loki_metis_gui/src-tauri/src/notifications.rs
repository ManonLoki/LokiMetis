use std::{future::Future, sync::Mutex, time::Duration};

#[cfg(any(not(target_os = "macos"), test))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tauri::async_runtime::JoinHandle;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, oneshot, watch};

#[path = "notifications_worker_owner.rs"]
mod notifications_worker_owner;

#[cfg(target_os = "macos")]
use notifications_worker_owner::NotificationOpenerChildGuard;
#[cfg(any(target_os = "macos", test))]
use notifications_worker_owner::reserve_notification_opener_slot;
use notifications_worker_owner::{
    NotificationShutdownSchedule, NotificationShutdownTaskBatchGuard, NotificationWorkerTaskOwner,
};
#[cfg(test)]
use notifications_worker_owner::{
    RetainableNotificationOpenerChild, retained_notification_opener_is_owned,
    retained_notification_owner_owns_task,
};

#[cfg(not(target_os = "macos"))]
use crate::runtime::AppRuntimeState;
use crate::settings::HostSettingsState;

const NOTIFICATION_QUEUE_CAPACITY: usize = 16;
const NOTIFICATION_FAILURE_EVENT: &str = "loki-metis://notification-error";
/// 单次通知设置或投递从进入 command 到完成的总墙钟时限。
const NOTIFICATION_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
/// 应用退出时通知 worker 从停止接单到确认 JoinHandle 终态的总时限。
const NOTIFICATION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// 总关闭时限尾部保留给 abort 后的 JoinHandle 终态确认。
const NOTIFICATION_SHUTDOWN_ABORT_REAP_BUDGET: Duration = Duration::from_millis(500);
const NOTIFICATION_OPERATION_TIMEOUT_ERROR: &str = "notification-operation-timeout";
#[cfg(any(not(target_os = "macos"), test))]
const NATIVE_NOTIFICATION_TASK_NAME: &str = "native-notification-delivery";
#[cfg(any(target_os = "macos", test))]
const MACOS_NOTIFICATION_SETTINGS_PREFIX: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=";
#[cfg(target_os = "macos")]
const MACOS_NOTIFICATION_SETTINGS_OPEN_COMMAND: &str = "/usr/bin/open";
#[cfg(target_os = "macos")]
const MACOS_NOTIFICATION_SETTINGS_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const MACOS_NOTIFICATION_SETTINGS_REAP_TIMEOUT: Duration = Duration::from_secs(1);
#[cfg(any(target_os = "macos", test))]
const MACOS_NOTIFICATION_SETTINGS_OPEN_FAILED: &str = "notification-settings-open-failed";
#[cfg(any(target_os = "macos", test))]
const MACOS_NOTIFICATION_SETTINGS_OPEN_TIMEOUT_ERROR: &str = "notification-settings-open-timeout";

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 归一化 macOS 通知授权状态，Unknown 作为受限或未知状态处理。
enum MacosNotificationAuthorizationStatus {
    NotDetermined,
    Denied,
    Authorized,
    Provisional,
    Ephemeral,
    RestrictedOrUnknown,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 区分首次读取与权限请求后的授权判断阶段。
enum MacosNotificationAuthorizationPhase {
    BeforeRequest,
    AfterRequest,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 表示 macOS 授权状态机下一步允许执行的操作。
enum MacosNotificationAuthorizationAction {
    Allow,
    Request,
    OpenSettings,
}

#[cfg(any(target_os = "macos", test))]
/// 根据真实状态快照决定授权、首次请求或恢复设置入口。
fn macos_notification_authorization_action(
    status: MacosNotificationAuthorizationStatus,
    phase: MacosNotificationAuthorizationPhase,
) -> MacosNotificationAuthorizationAction {
    match (status, phase) {
        (
            MacosNotificationAuthorizationStatus::Authorized
            | MacosNotificationAuthorizationStatus::Provisional
            | MacosNotificationAuthorizationStatus::Ephemeral,
            _,
        ) => MacosNotificationAuthorizationAction::Allow,
        (
            MacosNotificationAuthorizationStatus::NotDetermined,
            MacosNotificationAuthorizationPhase::BeforeRequest,
        ) => MacosNotificationAuthorizationAction::Request,
        (
            MacosNotificationAuthorizationStatus::NotDetermined
            | MacosNotificationAuthorizationStatus::Denied
            | MacosNotificationAuthorizationStatus::RestrictedOrUnknown,
            _,
        ) => MacosNotificationAuthorizationAction::OpenSettings,
    }
}

#[cfg(target_os = "macos")]
impl From<mac_usernotifications::AuthorizationStatus> for MacosNotificationAuthorizationStatus {
    /// 把 crate 的 Unknown 明确归一化为受限或未知状态。
    fn from(status: mac_usernotifications::AuthorizationStatus) -> Self {
        use mac_usernotifications::AuthorizationStatus;

        match status {
            AuthorizationStatus::NotDetermined => Self::NotDetermined,
            AuthorizationStatus::Denied => Self::Denied,
            AuthorizationStatus::Authorized => Self::Authorized,
            AuthorizationStatus::Provisional => Self::Provisional,
            AuthorizationStatus::Ephemeral => Self::Ephemeral,
            AuthorizationStatus::Unknown => Self::RestrictedOrUnknown,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
/// 仅从固定系统设置前缀与当前应用标识生成通知设置入口。
fn macos_notification_settings_url(bundle_identifier: &str) -> String {
    format!("{MACOS_NOTIFICATION_SETTINGS_PREFIX}{bundle_identifier}")
}

#[cfg(any(target_os = "macos", test))]
/// 把系统 opener 的实际退出状态映射为稳定结果。
fn macos_notification_settings_exit_result(success: bool) -> Result<(), &'static str> {
    if success {
        Ok(())
    } else {
        Err(MACOS_NOTIFICATION_SETTINGS_OPEN_FAILED)
    }
}

#[derive(Clone, Debug)]
/// 保存待交给系统通知服务的标题与正文。
pub(crate) struct NotificationPayload {
    pub(crate) title: String,
    pub(crate) body: String,
}

#[cfg(any(not(target_os = "macos"), test))]
/// 保证同一通知 worker 最多只有一个不可强停的原生投递在执行。
struct NativeNotificationLease(Arc<AtomicBool>);

#[cfg(any(not(target_os = "macos"), test))]
impl NativeNotificationLease {
    /// 原子取得原生投递权，前一项未终止时拒绝堆积新 blocking 任务。
    fn acquire(in_flight: &Arc<AtomicBool>) -> Result<Self, &'static str> {
        in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self(Arc::clone(in_flight)))
            .map_err(|_| "notification-delivery-in-progress")
    }
}

#[cfg(any(not(target_os = "macos"), test))]
impl Drop for NativeNotificationLease {
    /// 仅在真实原生调用返回或任务未能登记时释放串行门禁。
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 表示通知工作线程串行处理的授权或投递命令。
enum NotificationCommand {
    RequestPermission {
        deadline: tokio::time::Instant,
        reply: oneshot::Sender<Result<(), &'static str>>,
    },
    Deliver {
        payload: NotificationPayload,
        deadline: tokio::time::Instant,
        reply: oneshot::Sender<Result<(), &'static str>>,
    },
}

/// 持有有界通知队列及其后台任务的应用级状态。
pub(crate) struct NotificationWorker {
    sender: Mutex<Option<mpsc::Sender<NotificationCommand>>>,
    shutdown: watch::Sender<bool>,
    tasks: NotificationWorkerTaskOwner,
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

    /// 关闭发送端、广播取消，并在同一个总时限内确认 worker 已达到终态。
    pub(crate) async fn shutdown(&self) {
        self.shutdown_with_timeout(NOTIFICATION_SHUTDOWN_TIMEOUT)
            .await;
    }

    /// 使用可注入总时限关闭 worker，供生产门限与快速回归复用。
    async fn shutdown_with_timeout(&self, timeout: Duration) {
        let (mut batch, schedule) = self.begin_shutdown(timeout);

        reap_notification_worker_tasks_until(&mut batch.tasks, schedule.cooperative_deadline).await;
        for task in &batch.tasks {
            task.abort();
        }
        reap_notification_worker_tasks_until(&mut batch.tasks, schedule.final_deadline).await;
        if !batch.tasks.is_empty() {
            tracing::error!(
                task_count = batch.tasks.len(),
                "notification worker did not reach a terminal state after bounded abort"
            );
        }
        // batch 正常返回时也会把超时句柄交还 owner；取消与 panic 走同一路径。
    }

    /// 原子接管当前句柄并冻结首次关闭时限，随后才广播取消。
    fn begin_shutdown(
        &self,
        timeout: Duration,
    ) -> (
        NotificationShutdownTaskBatchGuard<'_>,
        NotificationShutdownSchedule,
    ) {
        self.sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let (batch, schedule) = self
            .tasks
            .begin_shutdown(timeout, NOTIFICATION_SHUTDOWN_ABORT_REAP_BUDGET);
        let _ = self.shutdown.send(true);
        (batch, schedule)
    }

    /// 返回 worker 是否已被真实回收。
    #[cfg(test)]
    fn is_reaped(&self) -> bool {
        self.tasks.task_count() == 0
    }

    /// 返回稳定 owner 当前持有的 worker 句柄数。
    #[cfg(test)]
    fn owned_task_count(&self) -> usize {
        self.tasks.task_count()
    }

    /// 返回首次关闭冻结的时限，供重复调用预算回归验证。
    #[cfg(test)]
    fn shutdown_schedule(&self) -> Option<NotificationShutdownSchedule> {
        self.tasks.shutdown_schedule()
    }
}

impl Drop for NotificationWorker {
    /// 在应用状态释放时保证通知后台任务不会遗留。
    fn drop(&mut self) {
        self.sender
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let _ = self.shutdown.send(true);
        self.tasks.abort_and_retain_all();
    }
}

/// 记录 worker 的最终 JoinHandle 结果；被 owner 主动 abort 属于预期关闭路径。
fn log_notification_worker_join_result(result: tauri::Result<()>) {
    match result {
        Ok(()) => {}
        Err(tauri::Error::JoinError(error)) if error.is_cancelled() => {}
        Err(error) => tracing::warn!(%error, "notification worker stopped unexpectedly"),
    }
}

/// 在共享截止时间前等待并移除已终态 worker 句柄，未终态项持续由 batch 持有。
async fn reap_notification_worker_tasks_until(
    tasks: &mut Vec<JoinHandle<()>>,
    deadline: tokio::time::Instant,
) {
    let mut index = 0;
    while index < tasks.len() {
        let result = if tasks[index].inner().is_finished() {
            Some((&mut tasks[index]).await)
        } else {
            tokio::time::timeout_at(deadline, &mut tasks[index])
                .await
                .ok()
        };
        if let Some(result) = result {
            let _terminal_task = tasks.swap_remove(index);
            log_notification_worker_join_result(result);
        } else {
            index += 1;
        }
    }
}

/// 等待通知 worker 的关闭广播；发送端消失也按关闭处理。
async fn notification_shutdown_requested(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

/// 在通知总截止时间内执行一个可取消步骤。
async fn notification_step_before_deadline<T, Step>(
    shutdown: &mut watch::Receiver<bool>,
    deadline: tokio::time::Instant,
    step: Step,
) -> Result<T, &'static str>
where
    Step: Future<Output = Result<T, &'static str>>,
{
    if *shutdown.borrow() {
        return Err("notification-worker-unavailable");
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(NOTIFICATION_OPERATION_TIMEOUT_ERROR);
    }
    tokio::select! {
        biased;
        _ = notification_shutdown_requested(shutdown) => Err("notification-worker-unavailable"),
        result = tokio::time::timeout_at(deadline, step) => {
            result.map_err(|_| NOTIFICATION_OPERATION_TIMEOUT_ERROR)?
        }
    }
}

/// 把命令放入有界队列并等待回复，排队和执行共享同一个总截止时间。
async fn enqueue_notification_command(
    sender: mpsc::Sender<NotificationCommand>,
    command: NotificationCommand,
    reply: oneshot::Receiver<Result<(), &'static str>>,
    deadline: tokio::time::Instant,
) -> Result<(), &'static str> {
    tokio::time::timeout_at(deadline, sender.send(command))
        .await
        .map_err(|_| NOTIFICATION_OPERATION_TIMEOUT_ERROR)?
        .map_err(|_| "notification-worker-unavailable")?;
    tokio::time::timeout_at(deadline, reply)
        .await
        .map_err(|_| NOTIFICATION_OPERATION_TIMEOUT_ERROR)?
        .map_err(|_| "notification-worker-unavailable")?
}

/// 安装应用级通知工作线程并串行处理授权与投递。
pub(crate) fn install_notification_worker(app: &tauri::AppHandle) {
    let (sender, mut receiver) = mpsc::channel(NOTIFICATION_QUEUE_CAPACITY);
    let (shutdown, mut shutdown_receiver) = watch::channel(false);
    #[cfg(not(target_os = "macos"))]
    let native_delivery_in_flight = Arc::new(AtomicBool::new(false));
    let worker_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let command = tokio::select! {
                biased;
                _ = notification_shutdown_requested(&mut shutdown_receiver) => break,
                command = receiver.recv() => command,
            };
            let Some(command) = command else {
                break;
            };
            match command {
                NotificationCommand::RequestPermission { deadline, reply } => {
                    let _ = reply.send(
                        request_system_notification_permission(
                            &worker_app,
                            deadline,
                            &mut shutdown_receiver,
                        )
                        .await,
                    );
                }
                NotificationCommand::Deliver {
                    payload,
                    deadline,
                    reply,
                } => {
                    if reply.is_closed() {
                        continue;
                    }
                    #[cfg(target_os = "macos")]
                    let result = deliver_system_notification(
                        &worker_app,
                        payload,
                        deadline,
                        &mut shutdown_receiver,
                    )
                    .await;
                    #[cfg(not(target_os = "macos"))]
                    let result = deliver_system_notification(
                        &worker_app,
                        payload,
                        deadline,
                        &mut shutdown_receiver,
                        &native_delivery_in_flight,
                    )
                    .await;
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
        shutdown,
        tasks: NotificationWorkerTaskOwner::new(task),
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
    let deadline = tokio::time::Instant::now() + NOTIFICATION_OPERATION_TIMEOUT;
    let settings = app.state::<HostSettingsState>();
    let previous = tokio::time::timeout_at(deadline, settings.read())
        .await
        .map_err(|_| NOTIFICATION_OPERATION_TIMEOUT_ERROR.to_owned())?
        .system_notification_enabled();
    if previous == enabled {
        return Ok(previous);
    }

    if enabled {
        let sender = app
            .state::<NotificationWorker>()
            .sender()
            .map_err(str::to_string)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        enqueue_notification_command(
            sender,
            NotificationCommand::RequestPermission {
                deadline,
                reply: reply_tx,
            },
            reply_rx,
            deadline,
        )
        .await
        .map_err(str::to_string)?;
    }

    tokio::time::timeout_at(
        deadline,
        persist_system_notification_setting(&settings, enabled),
    )
    .await
    .map_err(|_| NOTIFICATION_OPERATION_TIMEOUT_ERROR.to_owned())?
    .map_err(str::to_string)?;
    Ok(tokio::time::timeout_at(deadline, settings.read())
        .await
        .map_err(|_| NOTIFICATION_OPERATION_TIMEOUT_ERROR.to_owned())?
        .system_notification_enabled())
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
    let deadline = tokio::time::Instant::now() + NOTIFICATION_OPERATION_TIMEOUT;
    if !tokio::time::timeout_at(deadline, app.state::<HostSettingsState>().read())
        .await
        .map_err(|_| NOTIFICATION_OPERATION_TIMEOUT_ERROR)?
        .system_notification_enabled()
    {
        return Err("notification-disabled");
    }
    let sender = app.state::<NotificationWorker>().sender()?;
    let (reply_tx, reply_rx) = oneshot::channel();
    enqueue_notification_command(
        sender,
        NotificationCommand::Deliver {
            payload,
            deadline,
            reply: reply_tx,
        },
        reply_rx,
        deadline,
    )
    .await
}

#[cfg(target_os = "macos")]
/// 终止并在有界时间内回收系统设置 opener 子进程。
async fn stop_macos_notification_settings_opener(
    child: &mut NotificationOpenerChildGuard<tokio::process::Child>,
    deadline: tokio::time::Instant,
) -> bool {
    if child.start_kill().is_err() {
        return false;
    }
    match tokio::time::timeout_at(deadline, child.child_mut().wait()).await {
        Ok(Ok(_)) => {
            child.mark_terminal();
            true
        }
        Ok(Err(_)) | Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
/// 使用固定系统入口打开当前应用的 macOS 通知设置。
async fn open_macos_notification_settings(
    app: &tauri::AppHandle,
    deadline: tokio::time::Instant,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), &'static str> {
    let now = tokio::time::Instant::now();
    let Some(latest_wait_deadline) = deadline.checked_sub(MACOS_NOTIFICATION_SETTINGS_REAP_TIMEOUT)
    else {
        return Err(NOTIFICATION_OPERATION_TIMEOUT_ERROR);
    };
    if now >= latest_wait_deadline || *shutdown.borrow() {
        return Err(if *shutdown.borrow() {
            "notification-worker-unavailable"
        } else {
            NOTIFICATION_OPERATION_TIMEOUT_ERROR
        });
    }
    let settings_url = macos_notification_settings_url(&app.config().identifier);
    let opener_reservation = reserve_notification_opener_slot()?;
    let mut command = tokio::process::Command::new(MACOS_NOTIFICATION_SETTINGS_OPEN_COMMAND);
    command.arg(settings_url).kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|_| MACOS_NOTIFICATION_SETTINGS_OPEN_FAILED)?;
    let mut child = opener_reservation.adopt(child);

    let wait_deadline = (now + MACOS_NOTIFICATION_SETTINGS_OPEN_TIMEOUT).min(latest_wait_deadline);
    let wait_result = tokio::select! {
        biased;
        _ = notification_shutdown_requested(shutdown) => {
            let shutdown_reap_deadline =
                (tokio::time::Instant::now() + MACOS_NOTIFICATION_SETTINGS_REAP_TIMEOUT)
                    .min(deadline);
            let reaped = stop_macos_notification_settings_opener(
                &mut child,
                shutdown_reap_deadline,
            )
            .await;
            if !reaped {
                tracing::error!("macOS notification settings opener was not reaped during shutdown");
            }
            return Err("notification-worker-unavailable");
        }
        result = tokio::time::timeout_at(wait_deadline, child.child_mut().wait()) => result,
    };
    let success = match wait_result {
        Ok(Ok(status)) => {
            let success = status.success();
            child.mark_terminal();
            success
        }
        Ok(Err(_)) => {
            if !stop_macos_notification_settings_opener(&mut child, deadline).await {
                tracing::error!("macOS notification settings opener failed and was not reaped");
            }
            return Err(MACOS_NOTIFICATION_SETTINGS_OPEN_FAILED);
        }
        Err(_) => {
            if !stop_macos_notification_settings_opener(&mut child, deadline).await {
                tracing::error!("timed out macOS notification settings opener was not reaped");
            }
            return Err(MACOS_NOTIFICATION_SETTINGS_OPEN_TIMEOUT_ERROR);
        }
    };

    macos_notification_settings_exit_result(success)
}

#[cfg(target_os = "macos")]
/// 打开恢复入口后返回拒绝状态，避免把尚未授权写成已启用。
async fn open_macos_notification_settings_for_denial(
    app: &tauri::AppHandle,
    deadline: tokio::time::Instant,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), &'static str> {
    open_macos_notification_settings(app, deadline, shutdown).await?;
    Err("notification-permission-denied")
}

#[cfg(target_os = "macos")]
/// 使用 macOS 现代用户通知 API 读取状态，仅在首次未决定时请求授权。
async fn request_system_notification_permission(
    app: &tauri::AppHandle,
    deadline: tokio::time::Instant,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), &'static str> {
    let initial_status = notification_step_before_deadline(shutdown, deadline, async {
        mac_usernotifications::get_notification_settings()
            .await
            .map(|settings| settings.authorization_status.into())
            .map_err(|_| "notification-permission-unavailable")
    })
    .await?;
    match macos_notification_authorization_action(
        initial_status,
        MacosNotificationAuthorizationPhase::BeforeRequest,
    ) {
        MacosNotificationAuthorizationAction::Allow => Ok(()),
        MacosNotificationAuthorizationAction::OpenSettings => {
            open_macos_notification_settings_for_denial(app, deadline, shutdown).await
        }
        MacosNotificationAuthorizationAction::Request => {
            notification_step_before_deadline(shutdown, deadline, async {
                mac_usernotifications::request_auth()
                    .await
                    .map(|_| ())
                    .map_err(|_| "notification-permission-unavailable")
            })
            .await?;
            let refreshed_status = notification_step_before_deadline(shutdown, deadline, async {
                mac_usernotifications::get_notification_settings()
                    .await
                    .map(|settings| settings.authorization_status.into())
                    .map_err(|_| "notification-permission-unavailable")
            })
            .await?;
            match macos_notification_authorization_action(
                refreshed_status,
                MacosNotificationAuthorizationPhase::AfterRequest,
            ) {
                MacosNotificationAuthorizationAction::Allow => Ok(()),
                MacosNotificationAuthorizationAction::OpenSettings => {
                    open_macos_notification_settings_for_denial(app, deadline, shutdown).await
                }
                MacosNotificationAuthorizationAction::Request => {
                    Err("notification-permission-unavailable")
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
/// 桌面插件在 Windows/Linux 固定返回 Granted，此处直接映射并保留取消与时限检查。
async fn request_system_notification_permission(
    _app: &tauri::AppHandle,
    deadline: tokio::time::Instant,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), &'static str> {
    if *shutdown.borrow() {
        Err("notification-worker-unavailable")
    } else if tokio::time::Instant::now() >= deadline {
        Err(NOTIFICATION_OPERATION_TIMEOUT_ERROR)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
/// 使用 macOS 现代用户通知 API 投递一条通知。
async fn deliver_system_notification(
    _app: &tauri::AppHandle,
    payload: NotificationPayload,
    deadline: tokio::time::Instant,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), &'static str> {
    notification_step_before_deadline(shutdown, deadline, async {
        mac_usernotifications::Notification::new()
            .title(payload.title)
            .message(payload.body)
            .default_sound()
            .send()
            .await
            .map(|_| ())
            .map_err(|_| "notification-delivery-failed")
    })
    .await
}

#[cfg(not(target_os = "macos"))]
/// 把 Windows/Linux 投递交给应用级 BackgroundTaskOwner 的唯一具名 blocking 任务。
async fn deliver_system_notification(
    app: &tauri::AppHandle,
    payload: NotificationPayload,
    deadline: tokio::time::Instant,
    shutdown: &mut watch::Receiver<bool>,
    in_flight: &Arc<AtomicBool>,
) -> Result<(), &'static str> {
    let lease = NativeNotificationLease::acquire(in_flight)?;
    let identifier = app.config().identifier.clone();
    let native_deadline = deadline.into_std();
    let (reply, response) = oneshot::channel();
    app.state::<AppRuntimeState>()
        .background_tasks
        .spawn_blocking(NATIVE_NOTIFICATION_TASK_NAME, move |task_shutdown| {
            let _lease = lease;
            let result = if task_shutdown.is_cancelled() || reply.is_closed() {
                Err("notification-worker-unavailable")
            } else if std::time::Instant::now() >= native_deadline {
                Err(NOTIFICATION_OPERATION_TIMEOUT_ERROR)
            } else {
                show_native_notification(payload, &identifier)
            };
            let _ = reply.send(result);
        })
        .map_err(|_| "notification-worker-unavailable")?;
    notification_step_before_deadline(shutdown, deadline, async {
        response
            .await
            .map_err(|_| "notification-worker-unavailable")?
    })
    .await
}

#[cfg(not(target_os = "macos"))]
/// 同步调用 notify-rust 并返回真实投递结果；只能在受管 blocking 边界执行。
fn show_native_notification(
    payload: NotificationPayload,
    identifier: &str,
) -> Result<(), &'static str> {
    let mut notification = notify_rust::Notification::new();
    notification
        .summary(&payload.title)
        .body(&payload.body)
        .auto_icon();
    #[cfg(target_os = "windows")]
    {
        let executable =
            tauri::utils::platform::current_exe().map_err(|_| "notification-delivery-failed")?;
        let directory = executable.parent().ok_or("notification-delivery-failed")?;
        if !directory.ends_with(std::path::Path::new("target").join("debug"))
            && !directory.ends_with(std::path::Path::new("target").join("release"))
        {
            notification.app_id(identifier);
        }
    }
    notification
        .show()
        .map(|_| ())
        .map_err(|_| "notification-delivery-failed")
}

#[cfg(test)]
#[path = "notifications_deadline_tests.rs"]
mod notification_deadline_tests;
