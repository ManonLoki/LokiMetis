//! 应用级前台连接：内存轮询不查询 OS，权限与系统提交使用独立有界异步任务。
use super::{Service, driver};
use crate::usage_service::UsageService;
use futures::{
    FutureExt as _,
    future::{Either, select},
};
use gpui_kit::{AnyWindowHandle, App};
use loki_metris_core::{
    UsageDay,
    usage_milestones::{MilestoneOutcome, NotificationPermission},
};
use std::{
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};

impl Service {
    /// 建立唯一 delegate/notifier owner 后再做第一次只读权限查询。
    pub(crate) fn attach(
        &self,
        usage: Arc<UsageService>,
        locale: &str,
        main: AnyWindowHandle,
        cx: &mut App,
    ) {
        let (events, receiver) = mpsc::sync_channel(128);
        {
            let mut owner = self.owner.borrow_mut();
            owner.host = driver::install(events).map(Rc::new);
            owner.state.host_available = owner.host.is_some();
            owner.usage = Some(usage.clone());
            owner.locale = locale.into();
        }
        usage.configure_notifications(self.state().enabled, NotificationPermission::Unavailable);
        let service = self.clone();
        let poll = cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                cx.update(|cx| {
                    if service.owner.borrow().closing {
                        return;
                    }
                    for event in receiver.try_iter().take(128) {
                        match event {
                            driver::Event::Clicked => crate::native::show(main, cx),
                            driver::Event::LateFailure(id) => usage.notification_late_failure(id),
                        }
                    }
                    let progress = usage.notification_progress();
                    let mut owner = service.owner.borrow_mut();
                    let changed = owner.state.progress.revision != progress.revision;
                    owner.state.progress = progress;
                    if owner
                        .host
                        .as_ref()
                        .is_some_and(|host| driver::overflow(host))
                    {
                        usage.notification_callback_overflow();
                    }
                    let query = owner.state.enabled
                        && owner.permission_at.is_none_or(|at| {
                            cx.background_executor().now().saturating_duration_since(at)
                                >= Duration::from_secs(30)
                        });
                    drop(owner);
                    if changed {
                        cx.refresh_windows();
                    }
                    if query {
                        service.refresh_permission(cx);
                    }
                    service.reconcile(cx);
                    service.dispatch(cx);
                });
            }
        });
        self.owner.borrow_mut().poll = Some(poll);
        self.refresh_permission(cx);
    }
    /// 语言仅更新内存，正文与标题在提交时捕获当前有效语言。
    pub(crate) fn set_language(&self, locale: &str) {
        self.owner.borrow_mut().locale = locale.into();
    }
    /// 启动、设置页、激活、定时查询均只读且单在途，不触发授权。
    pub(crate) fn refresh_permission(&self, cx: &mut App) {
        let executor = cx.background_executor().clone();
        self.refresh_permission_using(driver::permission(executor), cx);
    }
    /// 注入只读响应以覆盖迟到查询；旧操作的状态不能覆盖新授权结果。
    pub(super) fn refresh_permission_using(
        &self,
        permission: impl std::future::Future<Output = NotificationPermission> + 'static,
        cx: &mut App,
    ) {
        let generation = {
            let mut owner = self.owner.borrow_mut();
            if owner.closing || owner.state.busy || owner.permission_task.is_some() {
                return;
            }
            owner.permission_at = Some(cx.background_executor().now());
            owner.generation
        };
        let service = self.clone();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |cx| {
            let value = match select(
                permission.boxed_local(),
                executor.timer(Duration::from_secs(10)).boxed_local(),
            )
            .await
            {
                Either::Left((value, _)) => value,
                Either::Right(_) => NotificationPermission::Unavailable,
            };
            let mut owner = service.owner.borrow_mut();
            owner.permission_task = None;
            if !super::current(&owner, generation) {
                return;
            }
            let clear_error = value == NotificationPermission::Allowed
                && owner
                    .state
                    .error
                    .is_some_and(|error| error != super::Error::Save);
            let changed = owner.state.permission != value || clear_error;
            owner.state.permission = value;
            if changed {
                tracing::info!(event = "notification_permission_changed", permission = ?value);
            }
            if clear_error {
                owner.state.error = None;
            }
            if let Some(usage) = &owner.usage {
                usage.configure_notifications(
                    owner.state.enabled,
                    owner.state.delivery_permission(),
                );
            }
            drop(owner);
            if changed {
                cx.update(|cx| cx.refresh_windows());
            }
        });
        self.owner.borrow_mut().permission_task = Some(task);
    }
    /// 一个已持久预约只有一个前台系统调用；关闭或旧快照均不再提交。
    fn dispatch(&self, cx: &mut App) {
        let owner = self.owner.borrow();
        if owner.closing || owner.delivery_task.is_some() || owner.permission_task.is_some() {
            return;
        }
        let Some(usage) = owner.usage.clone() else {
            return;
        };
        let Some(request) = usage.take_notification() else {
            return;
        };
        let now = crate::backend::local_index::current_epoch_ms();
        let ready = (0..=10_000).contains(&now.saturating_sub(request.reserved_ms))
            && owner.state.enabled
            && owner.state.permission == NotificationPermission::Allowed
            && owner.state.progress.snapshot.as_ref().is_some_and(|s| {
                UsageDay::capture(now, &jiff::tz::TimeZone::system()).is_some_and(|d| {
                    s.eligible(now, &d, &request.source) && s.day.key() == request.day_key
                })
            });
        let Some(host) = owner.host.clone().filter(|_| ready) else {
            usage.notification_unsubmitted(request.id);
            return;
        };
        let locale = owner.locale.clone();
        drop(owner);
        let (title, body) = message(&locale, request.total_tokens);
        let service = self.clone();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_cx| {
            let result = match select(
                driver::send(&host, executor.clone(), request.id.clone(), title, body)
                    .boxed_local(),
                executor.timer(Duration::from_secs(10)).boxed_local(),
            )
            .await
            {
                Either::Left((result, _)) => result,
                Either::Right(_) => MilestoneOutcome::Unknown,
            };
            // 已得到的 OS 回执立即进入持久 worker，退出不会等待另一轮 UI poll。
            usage.notification_receipt(request.id, result);
            service.owner.borrow_mut().delivery_task = None;
        });
        self.owner.borrow_mut().delivery_task = Some(task);
    }
    /// 每个 Unknown 只核对一次；系统历史不匹配时保留显式重试入口。
    fn reconcile(&self, cx: &mut App) {
        let mut owner = self.owner.borrow_mut();
        if owner.closing || owner.reconciliation_task.is_some() || owner.host.is_none() {
            return;
        }
        let Some(id) = owner.state.progress.unknown_id.clone() else {
            owner.checked_unknown = None;
            return;
        };
        if owner.checked_unknown.as_ref() == Some(&id) {
            return;
        }
        owner.checked_unknown = Some(id.clone());
        let Some(usage) = owner.usage.clone() else {
            return;
        };
        drop(owner);
        let executor = cx.background_executor().clone();
        let service = self.clone();
        let task = cx.spawn(async move |_cx| {
            let found = match select(
                driver::accepted(executor.clone(), id.clone()).boxed_local(),
                executor.timer(Duration::from_secs(10)).boxed_local(),
            )
            .await
            {
                Either::Left((found, _)) => found,
                Either::Right(_) => false,
            };
            if found {
                usage.notification_receipt(id, MilestoneOutcome::Accepted);
            }
            service.owner.borrow_mut().reconciliation_task = None;
        });
        self.owner.borrow_mut().reconciliation_task = Some(task);
    }
    /// 人类主动重试可能重复的 Unknown，在设置页文案中明确提示。
    pub(crate) fn retry(&self, cx: &mut App) {
        if let Some(usage) = &self.owner.borrow().usage {
            usage.retry_notifications();
        }
        if self.state().enabled && self.state().permission != NotificationPermission::Allowed {
            self.retry_permission(cx);
        } else {
            self.refresh_permission(cx);
        }
    }
    /// 已保存开启而OS权限丢失时，明确重试动作允许再次授权；不改写本地开关。
    fn retry_permission(&self, cx: &mut App) {
        let generation = {
            let mut owner = self.owner.borrow_mut();
            if owner.closing || owner.state.busy {
                return;
            }
            owner.generation += 1;
            owner.state.busy = true;
            owner.state.error = None;
            owner.generation
        };
        let service = self.clone();
        let executor = cx.background_executor().clone();
        let deadline = executor.now() + Duration::from_secs(10);
        let task = cx.spawn(async move |cx| {
            let result = match select(
                driver::authorize(executor.clone(), deadline).boxed_local(),
                executor.timer(Duration::from_secs(10)).boxed_local(),
            )
            .await
            {
                Either::Left((result, _)) => result,
                Either::Right(_) => Err(super::Error::Timeout),
            };
            let mut owner = service.owner.borrow_mut();
            if !super::current(&owner, generation) {
                return;
            }
            owner.state.busy = false;
            owner.state.error = result.err();
            owner.state.permission = match result {
                Ok(()) => NotificationPermission::Allowed,
                Err(super::Error::Denied) => NotificationPermission::Denied,
                Err(_) => NotificationPermission::Unavailable,
            };
            owner.permission_at = Some(executor.now());
            if let Some(usage) = &owner.usage {
                usage.configure_notifications(
                    owner.state.enabled,
                    owner.state.delivery_permission(),
                );
            }
            drop(owner);
            cx.update(|cx| {
                if result == Err(super::Error::Denied) {
                    super::open_settings(cx);
                }
                cx.refresh_windows();
            });
        });
        self.owner.borrow_mut().task = Some(task);
        cx.refresh_windows();
    }
}
/// 千分位格式只处理整数，不引入浮点或平台 locale 差异。
fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut result = String::with_capacity(digits.len() + 6);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result
}
/// 文案来自同一双语词典；同轮跨多个档位允许使用同一最新真实总量。
fn message(locale: &str, total: u64) -> (String, String) {
    (
        rust_i18n::t!("notification.title", locale = locale).to_string(),
        rust_i18n::t!(
            "notification.milestone",
            locale = locale,
            total = grouped(total)
        )
        .to_string(),
    )
}
#[cfg(test)]
mod tests {
    /// 用户指定中文正文与英语翻译均使用完整当前总量。
    #[test]
    fn notification_message_preserves_exact_total() {
        assert_eq!(
            super::message("zh-CN", 350_000_000),
            (
                "诡秘神谕".into(),
                "恭喜您今天已经使用「350,000,000」的Token了".into()
            )
        );
        assert_eq!(super::grouped(u64::MAX), "18,446,744,073,709,551,615");
        assert!(
            super::message("en-US", 350_000_000)
                .1
                .contains("350,000,000")
        );
    }
}
