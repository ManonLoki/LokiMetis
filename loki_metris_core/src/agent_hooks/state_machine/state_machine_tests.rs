// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造带稳定时间的状态机事件，简化生命周期断言。
    fn apply(
        machine: &mut HookStateMachine,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        second: u64,
    ) -> HookEventDecision {
        apply_tool(machine, AiTool::Codex, event, session_id, turn_id, second)
    }

    /// 构造指定工具的带稳定时间事件。
    fn apply_tool(
        machine: &mut HookStateMachine,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        second: u64,
    ) -> HookEventDecision {
        machine.apply_event_with_status_at(
            tool,
            event,
            session_id,
            turn_id,
            None,
            Duration::from_secs(second),
        )
    }

    /// 全新状态机不创建任何桌宠展示。
    #[test]
    fn fresh_machine_has_no_initial_display() {
        let machine = HookStateMachine::default();
        assert_eq!(machine.current_display_transition(), None);
    }

    /// 重复或迟到事件不得覆盖已经停止的可见状态。
    #[test]
    fn duplicate_and_late_events_do_not_overwrite_the_visible_state() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-1"), None, 1),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-1"), None, 2),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(
                &mut machine,
                "UserPromptSubmit",
                Some("session-1"),
                Some("turn-1"),
                3,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("session-1"), Some("turn-1"), 4,),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply(
                &mut machine,
                "PostToolUse",
                Some("session-1"),
                Some("turn-1"),
                5,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-1"), None, 6),
            HookEventDecision::Forward(HookTransition::Release)
        );
        assert_eq!(machine.current_display_transition(), None);
    }

    /// 多会话共享工具槽位时，只有最后一个会话结束才释放。
    #[test]
    fn only_the_last_live_session_releases_the_tool_slot() {
        let mut machine = HookStateMachine::default();
        let _ = apply(&mut machine, "SessionStart", Some("session-1"), None, 1);
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-2"), None, 2),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-1"), None, 3),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-2"), None, 4),
            HookEventDecision::Forward(HookTransition::Release)
        );
    }

    /// 未知事件返回不支持，且不得建立展示。
    #[test]
    fn unknown_event_is_unsupported_without_creating_a_display() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply(&mut machine, "UnknownEvent", None, None, 1),
            HookEventDecision::Unsupported
        );
        assert_eq!(machine.current_display_transition(), None);
    }

    /// 监控中途启动时，孤立完成事件不得创建幽灵展示。
    #[test]
    fn orphan_completion_is_ignored_without_creating_a_display() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply(
                &mut machine,
                "PostToolUse",
                Some("late-session"),
                Some("turn-1"),
                1,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(machine.current_display_transition(), None);
        assert_eq!(
            apply(
                &mut machine,
                "PreToolUse",
                Some("live-session"),
                Some("turn-1"),
                2,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
    }

    /// WorkBuddy 内置引擎在权限被拒绝或澄清结束后各只发一次收尾事件；
    /// 缺失这两个事件会让展示永远卡在询问态（真实回归：见 work_buddy.rs）。
    #[test]
    fn workbuddy_permission_denied_and_elicitation_result_close_the_asking_state() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::WorkBuddy,
                "PermissionRequest",
                Some("session-1"),
                Some("turn-1"),
                1,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::WorkBuddy,
                "PermissionDenied",
                Some("session-1"),
                Some("turn-1"),
                2,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::WorkBuddy,
                "Elicitation",
                Some("session-1"),
                Some("turn-1"),
                3,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::WorkBuddy,
                "ElicitationResult",
                Some("session-1"),
                Some("turn-1"),
                4,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
    }

    /// 会话墓碑拦截迟到事件，但显式 SessionStart 可恢复并保留旧轮次历史。
    #[test]
    fn ended_tombstone_rejects_late_events_and_resumes_explicitly() {
        let mut machine = HookStateMachine::default();
        let _ = apply(
            &mut machine,
            "UserPromptSubmit",
            Some("session-1"),
            Some("turn-1"),
            1,
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-1"), None, 2),
            HookEventDecision::Forward(HookTransition::Release)
        );
        for event in [
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Stop",
        ] {
            assert_eq!(
                apply(&mut machine, event, Some("session-1"), Some("turn-1"), 100,),
                HookEventDecision::Ignore,
                "墓碑应拦截迟到事件 {event}"
            );
        }
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-1"), None, 101),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply(
                &mut machine,
                "PreToolUse",
                Some("session-1"),
                Some("turn-1"),
                102,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(
                &mut machine,
                "UserPromptSubmit",
                Some("session-1"),
                Some("turn-2"),
                103,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
    }

    /// Goal 续跑允许新轮次进度恢复；孤立完成不恢复工作。
    #[test]
    fn goal_mode_resumes_with_new_turn_progress() {
        let mut machine = HookStateMachine::default();
        let _ = apply(&mut machine, "SessionStart", Some("goal"), None, 1);
        let _ = apply(
            &mut machine,
            "UserPromptSubmit",
            Some("goal"),
            Some("turn-1"),
            2,
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("goal"), Some("turn-1"), 3),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        for event in ["PostToolUse"] {
            assert_eq!(
                apply(&mut machine, event, Some("goal"), Some("turn-1"), 4),
                HookEventDecision::Ignore
            );
        }
        assert_eq!(
            apply(&mut machine, "PreToolUse", Some("goal"), Some("turn-2"), 5,),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("goal"), Some("turn-1"), 6),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("goal"), Some("turn-2"), 7),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
    }

    /// 最后一个活跃会话超时只回落 Idle，不把超时误判为明确 Release。
    #[test]
    fn expiring_last_active_session_falls_back_to_idle() {
        let mut machine = HookStateMachine::default();
        let _ = apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("active"),
            Some("turn-1"),
            0,
        );
        assert_eq!(
            machine.expire_inactive_sessions(Duration::from_secs(11), Duration::from_secs(10)),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            machine.expire_inactive_sessions(Duration::from_secs(12), Duration::from_secs(10)),
            HookEventDecision::Ignore
        );
    }

    /// 多会话展示按 Asking、Error、Running、Idle 聚合，逐层退出后正确降级。
    #[test]
    fn aggregate_phase_uses_aimonitor_priority() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionStart",
                Some("idle"),
                None,
                1,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "UserPromptSubmit",
                Some("running"),
                Some("turn-running"),
                2,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        let _ = apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("error"),
            Some("turn-error"),
            3,
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "PostToolUseFailure",
                Some("error"),
                Some("turn-error"),
                4,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
        );
        let _ = apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("asking"),
            Some("turn-asking"),
            5,
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "PermissionRequest",
                Some("asking"),
                Some("turn-asking"),
                6,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("asking"),
                None,
                7,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("error"),
                None,
                8,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "Stop",
                Some("running"),
                Some("turn-running"),
                9,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("running"),
                None,
                10,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("idle"),
                None,
                11,
            ),
            HookEventDecision::Forward(HookTransition::Release)
        );
    }
}
