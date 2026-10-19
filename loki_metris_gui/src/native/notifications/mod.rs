//! 通知开关由应用级 owner 持有；授权回调只传值，落盘回执才更新已确认状态。

mod driver;
#[cfg(target_os = "linux")]
mod linux;
mod host;
mod activity;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_delivery;
#[cfg(target_os = "macos")]
mod macos_delegate;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod refresh_tests;
#[cfg(windows)]
mod windows;

use crate::preferences::{PreferencePatch, Writer};
use futures::{
    FutureExt as _,
    future::{Either, select},
};
use gpui_kit::{App, Task};
use std::{
    cell::RefCell,
    future::Future,
    rc::Rc,
    time::{Duration, Instant},
};

/// 平台权限与保存失败分别保留；只有保存失败才保留原有本地开关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// 系统明确拒绝通知权限，保持关闭并引导到系统设置。
    Denied,
    /// 当前平台或会话没有可用的通知服务。
    Unavailable,
    /// 授权在 10 秒期限内没有得到结果。
    Timeout,
    /// 本地偏好保存失败，开关保留原值。
    Save,
}

/// 本地偏好是展示权威；启动只读查询权限，不申请或隐式重写已经保存的值。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// 已保存的本地通知开关，是展示的唯一权威。
    pub enabled: bool,
    /// 开关切换进行中，期间控件禁用。
    pub busy: bool,
    /// 切换进行中的目标值；空闲时为 None。
    pub desired: Option<bool>,
    /// 最近一次切换失败的类别；成功或重新开始切换时清除。
    pub error: Option<Error>,
    /// OS 权限与本地开关分别呈现。
    pub permission: loki_metris_core::usage_milestones::NotificationPermission,
    /// 发送器可用性不冒充 OS 授权状态；缺少 owner 时暂停提交。
    pub host_available: bool,
    /// 已持久化的队列与结果计数。
    pub progress: crate::usage_service::NotificationProgress,
}

/// 高频宿主轮询只比较展示键；业务快照仍由 owner 持有最新读取时间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RefreshKey {
    /// 本地通知偏好是否开启。
    enabled: bool,
    /// 是否有开关切换在途。
    busy: bool,
    /// 切换在途时的目标值。
    desired: Option<bool>,
    /// 最近一次切换的失败类别。
    error: Option<Error>,
    /// 最近读取的系统通知权限。
    permission: loki_metris_core::usage_milestones::NotificationPermission,
    /// 平台通知宿主是否可用。
    host_available: bool,
    /// 持久通知进度的修订号；只比较修订号即可判断进度是否变化。
    progress_revision: u64,
}

impl State {
    /// 系统允许且原生 owner 存活才向领域发送器提供可提交的权限。
    fn delivery_permission(&self) -> loki_metris_core::usage_milestones::NotificationPermission {
        if self.host_available {
            self.permission
        } else {
            loki_metris_core::usage_milestones::NotificationPermission::Unavailable
        }
    }
}

/// 唯一前台任务拥有操作代次；退出先失效，再排空已经接受的偏好写入。
struct Owner {
    /// 设置页与托盘读取的通知展示状态。
    state: State,
    /// 开关操作代次；每次切换或退出递增，迟到的授权与保存结果按代次丢弃。
    generation: u64,
    /// 应用正在退出；置位后不再接受新操作或调度任何系统调用。
    closing: bool,
    /// 当前开关切换（授权加偏好保存）任务；同一时刻只有一个。
    task: Option<Task<()>>,
    /// 平台通知宿主；平台不支持或创建失败时为 None，提交暂停。
    host: Option<Rc<driver::Host>>,
    /// 每 200ms 处理回调、进度、权限复查与调度的前台轮询任务。
    poll: Option<Task<()>>,
    /// 在途的只读权限查询；存在时暂停新的发送。
    permission_task: Option<Task<()>>,
    /// 在途的单次通知提交；同一时刻至多一个系统调用。
    delivery_task: Option<Task<()>>,
    /// 在途的 Unknown 回执历史核对任务。
    reconciliation_task: Option<Task<()>>,
    /// 提供待发送预约与回执落盘的用量服务；安装前为 None。
    usage: Option<std::sync::Arc<crate::usage_service::UsageService>>,
    /// 通知标题与正文使用的当前语言。
    locale: String,
    /// 最近一次权限查询开始的时刻，用于限制轮询每 30 秒最多复查一次。
    permission_at: Option<Instant>,
    /// 已核对过的 Unknown 通知 ID；同一 ID 只核对一次。
    checked_unknown: Option<String>,
}

/// 克隆仅共享应用级状态，不依赖任何页面 Entity 的生命周期。
#[derive(Clone)]
pub struct Service {
    /// 前台共享的唯一 owner；所有克隆共享同一状态。
    owner: Rc<RefCell<Owner>>,
    /// 后台串行写入通知偏好的写入器。
    writer: Writer,
}

impl Service {
    /// 初始化只接收已读取的持久值；没有系统通知或授权副作用。
    pub fn new(enabled: bool, writer: Writer) -> Self {
        Self {
            owner: Rc::new(RefCell::new(Owner {
                state: State {
                    enabled,
                    ..Default::default()
                },
                generation: 0,
                closing: false,
                task: None,
                host: None,
                poll: None,
                permission_task: None,
                delivery_task: None,
                reconciliation_task: None,
                usage: None,
                locale: "en-US".into(),
                permission_at: None,
                checked_unknown: None,
            })),
            writer,
        }
    }

    /// 渲染只复制本地快照，不触发系统查询。
    pub fn state(&self) -> State {
        self.owner.borrow().state.clone()
    }

    /// 同一次借用先比较 Copy 键，变化时才复制带字符串的完整展示快照。
    pub(crate) fn state_if_changed(&self, previous: Option<RefreshKey>) -> Option<(RefreshKey, State)> {
        let owner = self.owner.borrow();
        let state = &owner.state;
        let key = RefreshKey {
            enabled: state.enabled,
            busy: state.busy,
            desired: state.desired,
            error: state.error,
            permission: state.permission,
            host_available: state.host_available,
            progress_revision: state.progress.revision,
        };
        if previous == Some(key) {
            return None;
        }
        Some((key, state.clone()))
    }

    /// 只有实际开关事件可以授权；关闭与等值操作都不请求系统权限。
    pub fn set(&self, enabled: bool, cx: &mut App) {
        let executor = cx.background_executor().clone();
        self.set_using(
            enabled,
            move |deadline| async move { driver::authorize(executor, deadline).await },
            cx,
        );
    }

    /// 授权有界且不决定本地意图；保存成功才确认开关，权限失败仍单独披露并暂停发送。
    fn set_using<F: Future<Output = Result<(), Error>> + 'static>(
        &self,
        enabled: bool,
        authorize: impl FnOnce(Instant) -> F + 'static,
        cx: &mut App,
    ) {
        let generation = {
            let mut owner = self.owner.borrow_mut();
            if owner.closing || owner.state.busy || owner.state.enabled == enabled {
                return;
            }
            owner.generation += 1;
            owner.state.busy = true;
            owner.state.desired = Some(enabled);
            owner.state.error = None;
            #[cfg(target_os = "linux")]
            if !enabled && let Some(host) = &owner.host { host.set_delivery_allowed(false); }
            owner.generation
        };
        let executor = cx.background_executor().clone();
        let deadline = executor.now() + Duration::from_secs(10);
        let weak = Rc::downgrade(&self.owner);
        let writer = self.writer.clone();
        let task = cx.spawn(async move |cx| {
            let mut permission = if !enabled {
                Ok(())
            } else if executor.now() >= deadline {
                Err(Error::Timeout)
            } else {
                match select(
                    authorize(deadline).boxed_local(),
                    executor
                        .timer(deadline.saturating_duration_since(executor.now()))
                        .boxed_local(),
                )
                .await
                {
                    Either::Left((result, _)) => result,
                    Either::Right(_) => Err(Error::Timeout),
                }
            };
            let Some(owner) = weak.upgrade() else { return };
            if !current(&owner.borrow(), generation) {
                return;
            }
            if enabled && permission.is_ok() && executor.now() >= deadline {
                permission = Err(Error::Timeout);
            }
            if enabled {
                use loki_metris_core::usage_milestones::NotificationPermission as Permission;
                owner.borrow_mut().state.permission = match permission {
                    Ok(()) => Permission::Allowed,
                    Err(Error::Denied) => Permission::Denied,
                    Err(_) => Permission::Unavailable,
                };
                owner.borrow_mut().permission_at = Some(executor.now());
            }
            // 授权超时只结束系统操作，不能撤销用户明确提交的本地偏好。
            let receipt = writer
                .mutate(PreferencePatch::SystemNotification(enabled))
                .map_err(|_| Error::Save);
            let result = match receipt {
                Ok(receipt) => match crate::usage_service::poll_receipt(
                    receipt,
                    cx.background_executor().clone(),
                    std::time::Duration::from_millis(20),
                )
                .await
                {
                    Some(Ok(PreferencePatch::SystemNotification(actual))) if actual == enabled => {
                        Ok(())
                    }
                    _ => Err(Error::Save),
                },
                Err(error) => Err(error),
            };
            let mut state = owner.borrow_mut();
            if !current(&state, generation) {
                return;
            }
            state.state.busy = false;
            state.state.desired = None;
            match result {
                Ok(()) => {
                    state.state.enabled = enabled;
                    state.state.error = permission.err();
                }
                Err(error) => state.state.error = Some(error),
            }
            #[cfg(target_os = "linux")]
            if let Some(host) = &state.host {
                host.set_delivery_allowed(state.state.enabled && state.state.permission == loki_metris_core::usage_milestones::NotificationPermission::Allowed);
            }
            if let Some(usage) = &state.usage {
                usage.configure_notifications(
                    state.state.enabled,
                    state.state.delivery_permission(),
                );
            }
            drop(state);
            cx.update(|cx| {
                if result.is_ok() && permission == Err(Error::Denied) {
                    open_settings(cx);
                }
                cx.refresh_windows();
            });
        });
        self.owner.borrow_mut().task = Some(task);
        cx.refresh_windows();
    }

    /// 退出同步取消权限任务；系统迟到回调最多发送到已断开的 oneshot。
    pub fn stop(&self) {
        let mut owner = self.owner.borrow_mut();
        owner.closing = true;
        #[cfg(target_os = "linux")]
        if let Some(host) = &owner.host { host.set_delivery_allowed(false); }
        if let Some(usage) = &owner.usage { usage.activity_notifications().configure(false); }
        owner.generation += 1;
        owner.task.take();
        owner.poll.take();
        owner.permission_task.take();
        owner.delivery_task.take();
        owner.reconciliation_task.take();
        owner.host.take();
        owner.state.host_available = false;
        owner.state.busy = false;
        owner.state.desired = None;
    }
}

/// 授权和回执都必须仍属于当前应用操作，不能在退出后更新或开始写入。
fn current(owner: &Owner, generation: u64) -> bool {
    !owner.closing && owner.generation == generation
}

/// 拒绝时打开当前产品的通知设置，与参考应用一致；关闭不会撤销系统授权。
pub(crate) fn open_settings(cx: &App) {
    #[cfg(all(target_os = "macos",not(test)))]
    cx.open_url("x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=com.loki-metris.app");
    #[cfg(all(windows, not(test)))]
    cx.open_url("ms-settings:notifications");
    #[cfg(any(not(any(target_os = "macos", windows)), test))]
    let _ = cx;
}

/// 权限标签在所有入口复用，未知系统值不会显示“已允许”。
pub(crate) fn permission_label(
    locale: &str,
    permission: loki_metris_core::usage_milestones::NotificationPermission,
) -> String {
    use loki_metris_core::usage_milestones::NotificationPermission::*;
    rust_i18n::t!(
        match permission {
            Allowed => "notification.permission_allowed",
            Denied => "notification.permission_denied",
            NotDetermined => "notification.permission_ask",
            Unavailable => "notification.permission_unavailable",
        },
        locale = locale
    )
    .to_string()
}
/// 勾选仅表达本地开启，标签单独披露忙碌、权限或积压。
pub(crate) fn menu_label(locale: &str, state: &State) -> String {
    if state.busy {
        return rust_i18n::t!("notification.busy", locale = locale).to_string();
    }
    if !state.enabled {
        return rust_i18n::t!("notification.menu_disabled", locale = locale).to_string();
    }
    if state.permission != loki_metris_core::usage_milestones::NotificationPermission::Allowed {
        return permission_label(locale, state.permission);
    }
    if !state.host_available {
        return rust_i18n::t!("notification.menu_unavailable", locale = locale).to_string();
    }
    if state.progress.unknown_id.is_some() {
        return rust_i18n::t!("notification.menu_unknown", locale = locale).to_string();
    }
    rust_i18n::t!(
        "notification.menu_queue",
        locale = locale,
        count = state.progress.pending
    )
    .to_string()
}
