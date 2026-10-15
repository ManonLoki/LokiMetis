//! 工具/澄清的等待归属，独立于整个会话与轮次的生命周期。

use super::session::{HookInteraction, HookSessionState};
use crate::agent_hooks::HookEventScope;

/// 只有明确的调用开始与回复才改变交互集合，压缩和非最终 Stop 都不解除等待。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InteractionAction {
    Observe,
    Begin,
    Resolve,
    ElicitationBegin,
    ElicitationResolve,
    ElicitationNotification,
}

/// 原生事件名称归约为交互动作，规则留在 core。
pub(super) fn interaction_action(event: &str, status: Option<&str>) -> InteractionAction {
    match event {
        "PreToolUse" | "preToolUse" | "BeforeTool" | "tool.execute.before" => {
            InteractionAction::Begin
        }
        "PostToolUse"
        | "postToolUse"
        | "PostToolUseFailure"
        | "postToolUseFailure"
        | "AfterTool"
        | "PermissionDenied"
        | "tool.execute.after"
        | "permission.replied"
        | "question.replied"
        | "question.rejected"
        | "post_approval_response"
        | "post_tool_call" => InteractionAction::Resolve,
        "Elicitation" => InteractionAction::ElicitationBegin,
        "ElicitationResult" => InteractionAction::ElicitationResolve,
        "Notification" if status == Some("elicitation_response") => {
            InteractionAction::ElicitationResolve
        }
        "Notification"
            if matches!(
                status,
                Some("elicitation_dialog" | "elicitation_url_dialog")
            ) =>
        {
            InteractionAction::ElicitationNotification
        }
        _ => InteractionAction::Observe,
    }
}

impl HookSessionState {
    /// 关联无调用ID的权限请求，并仅在明确响应时解除匹配等待。
    pub(super) fn update_interactions(
        &mut self,
        scope: HookEventScope<'_>,
        action: InteractionAction,
        asking: bool,
    ) {
        if action == InteractionAction::ElicitationNotification
            && self
                .waiting
                .iter()
                .any(|wait| wait.elicitation && wait.agent_id.as_deref() == scope.agent_id)
        {
            return;
        }
        if action == InteractionAction::ElicitationBegin {
            self.waiting.retain(|wait| {
                !(wait.elicitation
                    && wait.notification_only
                    && wait.agent_id.as_deref() == scope.agent_id)
            });
        }
        let mut interaction = HookInteraction {
            agent_id: scope.agent_id.map(str::to_owned),
            interaction_id: scope.interaction_id.map(str::to_owned),
            tool_name: scope.tool_name.map(str::to_owned),
            elicitation: matches!(
                action,
                InteractionAction::ElicitationBegin
                    | InteractionAction::ElicitationResolve
                    | InteractionAction::ElicitationNotification
            ),
            notification_only: action == InteractionAction::ElicitationNotification,
        };
        if action == InteractionAction::Begin
            && (interaction.interaction_id.is_some() || interaction.tool_name.is_some())
            && !self
                .active_interactions
                .iter()
                .any(|call| same_call(call, &interaction))
        {
            if self.active_interactions.len() < 256 {
                self.active_interactions.push(interaction.clone());
            } else {
                self.active_interactions_overflow = true;
            }
        }
        // 仅名称和Agent都匹配且唯一时补全；并行同名工具不猜测归属。
        if interaction.interaction_id.is_none()
            && !self.active_interactions_overflow
            && (interaction.tool_name.is_some() || asking)
        {
            let mut candidates = self.active_interactions.iter().filter(|call| {
                call.agent_id == interaction.agent_id
                    && (interaction.tool_name.is_none() || call.tool_name == interaction.tool_name)
            });
            if let Some(call) = candidates.next()
                && candidates.next().is_none()
            {
                interaction.interaction_id = call.interaction_id.clone();
                interaction.tool_name = call.tool_name.clone();
            }
        }
        // 官方澄清回执常无调用ID；只在同Agent的澄清等待唯一时补全，不触碰其它询问。
        if action == InteractionAction::ElicitationResolve && scope.interaction_id.is_none() {
            let mut waits = self.waiting.iter().filter(|wait| {
                wait.agent_id == interaction.agent_id
                    && wait.elicitation
                    && (scope.tool_name.is_none() || wait.tool_name.as_deref() == scope.tool_name)
            });
            if let Some(wait) = waits.next()
                && waits.next().is_none()
            {
                interaction.interaction_id = wait.interaction_id.clone();
                interaction.tool_name = wait.tool_name.clone();
            }
        }
        if asking {
            // 同一服务器无ID的并行表单无法区分答复归属，保守保留直到明确终态。
            if action == InteractionAction::ElicitationBegin
                && scope.interaction_id.is_none()
                && self.waiting.iter().any(|wait| {
                    wait.agent_id == interaction.agent_id
                        && wait.elicitation
                        && (scope.tool_name.is_none()
                            || wait.tool_name.as_deref() == scope.tool_name)
                })
            {
                self.waiting_overflow = true;
            }
            self.wait_for_interaction(interaction);
        } else if matches!(
            action,
            InteractionAction::Resolve | InteractionAction::ElicitationResolve
        ) {
            self.waiting.retain(|wait| {
                !(same_call(wait, &interaction)
                    && (action != InteractionAction::ElicitationResolve || wait.elicitation))
            });
            self.active_interactions
                .retain(|call| !same_call(call, &interaction));
        }
    }

    /// 去重无ID通知，并把早期泛化等待收敛为已证实的精确调用。
    fn wait_for_interaction(&mut self, interaction: HookInteraction) {
        if interaction.interaction_id.is_none()
            && interaction.tool_name.is_none()
            && self.waiting.iter().any(|wait| {
                wait.agent_id == interaction.agent_id && wait.elicitation == interaction.elicitation
            })
        {
            return;
        }
        if interaction.interaction_id.is_some() {
            self.waiting.retain(|wait| {
                !(wait.agent_id == interaction.agent_id
                    && wait.elicitation == interaction.elicitation
                    && wait.interaction_id.is_none()
                    && (wait.tool_name.is_none() || wait.tool_name == interaction.tool_name))
            });
        }
        if !self.waiting.iter().any(|wait| {
            same_call(wait, &interaction) && wait.elicitation == interaction.elicitation
        }) {
            if self.waiting.len() < 256 {
                self.waiting.push(interaction);
            } else {
                self.waiting_overflow = true;
            }
        }
    }
}

/// 有ID时必须精确匹配；无ID回复不能消费已知ID的等待。
fn same_call(left: &HookInteraction, right: &HookInteraction) -> bool {
    left.agent_id == right.agent_id
        && match (&left.interaction_id, &right.interaction_id) {
            (Some(a), Some(b)) => a == b,
            (None, None) => left.tool_name == right.tool_name,
            _ => false,
        }
}
