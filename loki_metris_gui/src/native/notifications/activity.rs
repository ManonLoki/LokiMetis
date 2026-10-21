//! 短时业务通知共用原生发送 owner 与权限，不创建第二个 delegate。

use super::{Service, driver};
use crate::notification_events::Activity;
use futures::{FutureExt as _, future::{Either, select}};
use gpui_kit::App;
use loki_metris_core::{HookNotification, usage_milestones::{MilestoneOutcome, NotificationPermission}};
use std::time::Duration;

impl Service {
    /// Token 预约优先，其余终态使用同一单在途槽；关闭与权限暂停期间不补发。
    pub(super) fn dispatch_activity(&self, cx: &mut App) {
        let owner = self.owner.borrow();
        if owner.closing || owner.state.busy || owner.delivery_task.is_some() || owner.permission_task.is_some()
            || !owner.state.enabled || owner.state.delivery_permission() != NotificationPermission::Allowed { return; }
        let Some(usage) = &owner.usage else { return; };
        let Some(request) = usage.activity_notifications().take() else { return; };
        let Some(host) = owner.host.clone() else { return; };
        let (title, body) = message(&owner.locale, request.activity);
        drop(owner);
        let service = self.clone();
        let executor = cx.background_executor().clone();
        let delivery = driver::send(&host, executor.clone(), request.id, title, body);
        let task = cx.spawn(async move |_cx| {
            let result = match select(
                delivery.boxed_local(),
                executor.timer(Duration::from_secs(10)).boxed_local(),
            ).await {
                Either::Left((result, _)) => result,
                Either::Right(_) => MilestoneOutcome::Unknown,
            };
            tracing::info!(event = "activity_notification_finished", outcome = ?result);
            service.owner.borrow_mut().delivery_task = None;
        });
        self.owner.borrow_mut().delivery_task = Some(task);
    }
}

/// 只显示公开工具/宿主名称与状态，不泄露载荷、用户输入或失败路径。
fn message(locale: &str, activity: Activity) -> (String, String) {
    let (key, name) = match activity {
        Activity::Hook { tool, state } => (match state {
            HookNotification::Asking => "notification.hook_asking",
            HookNotification::Ended => "notification.hook_ended",
            HookNotification::Error => "notification.hook_error",
        }, loki_metris_core::public_ai_capabilities().iter()
            .find(|capability| capability.monitor_tool == Some(tool))
            .map(|capability| capability.name)
            .unwrap_or_else(|| loki_metris_core::ai_tool_name(tool))),
        Activity::Skin { host, success } => (if success { "notification.skin_success" } else { "notification.skin_failure" }, host.display_name()),
    };
    (rust_i18n::t!("notification.title", locale=locale).to_string(), rust_i18n::t!(key, locale=locale, name=name).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metris_core::SkinHostKind;
    /// 三种 Hook 与换肤两种终态均解析为完整双语正文。
    #[test]
    fn activity_notification_messages_resolve_both_languages() {
        for state in [HookNotification::Asking, HookNotification::Ended, HookNotification::Error] {
            for locale in ["zh-CN", "en-US"] {
                let (_, body) = message(locale, Activity::Hook { tool: loki_metris_core::AiTool::Codex, state });
                assert!(body.contains("Codex"));
                assert!(!body.contains("notification."));
            }
        }
        for (tool, name) in [
            (loki_metris_core::AiTool::Codex, "Codex"),
            (loki_metris_core::AiTool::ClaudeCode, "Claude Code"),
            (loki_metris_core::AiTool::Cursor, "Cursor"),
            (loki_metris_core::AiTool::Grok, "Grok"),
            (loki_metris_core::AiTool::WorkBuddy, "WorkBuddy"),
        ] {
            for (state, suffix) in [
                (HookNotification::Asking, "需要您决策"),
                (HookNotification::Error, "出错了"),
                (HookNotification::Ended, "已完成"),
            ] {
                assert_eq!(message("zh-CN", Activity::Hook { tool, state }).1, format!("{name}{suffix}"));
            }
        }
        assert_eq!(message("zh-CN", Activity::Skin { host: SkinHostKind::WorkBuddy, success: true }).1, "WorkBuddy 皮肤更换成功");
        assert_eq!(message("zh-CN", Activity::Skin { host: SkinHostKind::Codex, success: false }).1, "Codex 皮肤更换失败，请在应用内查看详情");
        assert!(message("en-US", Activity::Skin { host: SkinHostKind::Codex, success: false }).1.contains("failed"));
    }
}
