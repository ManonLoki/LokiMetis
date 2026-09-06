//! 桌宠悬浮窗的四 Agent 宫格投影与窗口规格；不含局域网发现。

use serde::{Deserialize, Serialize};

use crate::{AiProfileDraft, AiTool, HookBehavior, ai_tool_name};

/// 桌宠宫格中的一个槽位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetOverlaySlot {
    /// 槽位对应的已批准 Agent。
    pub tool: AiTool,
    /// 展示名。
    pub name: String,
    /// 是否已有对应本机图。
    pub occupied: bool,
    /// 占用时的图库键。
    pub image_key: Option<String>,
}

/// 桌宠宫格完整投影。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetOverlayView {
    /// 固定四个槽位，顺序与 [`AiTool::ALL`] 一致。
    pub slots: Vec<PetOverlaySlot>,
}

/// 桌宠悬浮窗的宿主无关规格。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PetOverlayWindowSpec {
    /// 原生窗口标签。
    pub label: &'static str,
    /// 是否绘制系统标题栏。
    pub decorations: bool,
    /// 是否透明背景。
    pub transparent: bool,
    /// 是否默认始终置顶。
    pub always_on_top: bool,
    /// 是否从任务栏/Dock 隐藏独立入口。
    pub skip_taskbar: bool,
    /// 打开时是否隐藏主窗口。
    pub hide_main_window: bool,
    /// 打开或关闭时是否停止 Hook listener。
    pub stop_hook_listener: bool,
    /// 默认逻辑宽度。
    pub default_width: f64,
    /// 默认逻辑高度。
    pub default_height: f64,
}

/// 已交付的桌宠悬浮窗规格：透明无边框置顶，且主窗口保持可显示。
pub const PET_OVERLAY_WINDOW_SPEC: PetOverlayWindowSpec = PetOverlayWindowSpec {
    label: "pet",
    decorations: false,
    transparent: true,
    always_on_top: true,
    skip_taskbar: true,
    hide_main_window: false,
    stop_hook_listener: false,
    default_width: 360.0,
    default_height: 360.0,
};

/// 某工具当前应展示的 Hook 行为。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayToolBehavior {
    /// 已批准 Agent。
    pub tool: AiTool,
    /// 当前展示行为。
    pub behavior: HookBehavior,
}

/// 从已保存展示草稿和当前行为投影四槽宫格；无当前行为时保持空槽。
pub fn project_pet_overlay_from_drafts(
    drafts: &[AiProfileDraft],
    current_behaviors: &[PetOverlayToolBehavior],
) -> PetOverlayView {
    PetOverlayView {
        slots: AiTool::ALL
            .into_iter()
            .map(|tool| {
                let image_key = current_behaviors
                    .iter()
                    .find(|item| item.tool == tool)
                    .and_then(|item| {
                        drafts
                            .iter()
                            .find(|draft| draft.tool == tool)
                            .and_then(|draft| draft_image_key(draft, item.behavior))
                    });
                PetOverlaySlot {
                    tool,
                    name: ai_tool_name(tool).to_owned(),
                    occupied: image_key.is_some(),
                    image_key,
                }
            })
            .collect(),
    }
}

fn draft_image_key(draft: &AiProfileDraft, behavior: HookBehavior) -> Option<String> {
    draft
        .hooks
        .iter()
        .find(|hook| hook.behavior == behavior)
        .map(|hook| hook.image.trim())
        .filter(|image| !image.is_empty())
        .map(str::to_owned)
}

/// 返回已交付的悬浮窗规格。
pub fn pet_overlay_window_spec() -> PetOverlayWindowSpec {
    PET_OVERLAY_WINDOW_SPEC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_has_exactly_four_approved_agent_slots() {
        let view = project_pet_overlay_from_drafts(&[], &[]);
        let tools: Vec<_> = view.slots.iter().map(|slot| slot.tool).collect();
        assert_eq!(
            tools,
            vec![
                AiTool::Codex,
                AiTool::ClaudeCode,
                AiTool::Grok,
                AiTool::WorkBuddy
            ]
        );
        assert_eq!(view.slots.len(), AiTool::ALL.len());
        for slot in &view.slots {
            assert!(!slot.occupied);
            assert_eq!(slot.name, ai_tool_name(slot.tool));
        }
        let names: String = view.slots.iter().map(|slot| slot.name.clone()).collect();
        assert!(!names.to_ascii_lowercase().contains("cursor"));
        assert!(!names.to_ascii_lowercase().contains("opencode"));
    }

    fn draft_with_images(tool: AiTool, idle: &str, running: &str) -> AiProfileDraft {
        let mut draft = AiProfileDraft::default_for(tool);
        for hook in &mut draft.hooks {
            hook.image = match hook.behavior {
                HookBehavior::Idle => idle.to_owned(),
                HookBehavior::Running => running.to_owned(),
                _ => String::new(),
            };
        }
        draft
    }

    #[test]
    fn overlay_slots_use_saved_draft_image_ids_not_filenames() {
        let drafts = vec![draft_with_images(AiTool::Codex, "img-idle", "img-run")];
        let view = project_pet_overlay_from_drafts(
            &drafts,
            &[PetOverlayToolBehavior {
                tool: AiTool::Codex,
                behavior: HookBehavior::Idle,
            }],
        );
        assert_eq!(view.slots[0].image_key.as_deref(), Some("img-idle"));
        assert!(view.slots[0].occupied);
        assert!(!view.slots[1].occupied);
        assert!(!view.slots[2].occupied);
        assert!(!view.slots[3].occupied);
        let unlabeled = project_pet_overlay_from_drafts(
            &[],
            &[],
        );
        assert!(unlabeled.slots.iter().all(|slot| !slot.occupied));
        assert!(
            unlabeled
                .slots
                .iter()
                .all(|slot| slot.image_key.as_deref() != Some("codex.png"))
        );
    }

    #[test]
    fn overlay_without_current_behavior_has_no_initial_image() {
        let drafts = vec![draft_with_images(AiTool::Grok, "img-idle", "img-run")];
        let view = project_pet_overlay_from_drafts(&drafts, &[]);
        let grok = view
            .slots
            .iter()
            .find(|slot| slot.tool == AiTool::Grok)
            .expect("grok");
        assert!(!grok.occupied);
        assert_eq!(grok.image_key, None);
    }

    #[test]
    fn overlay_uses_configured_image_for_explicit_behavior() {
        let drafts = vec![draft_with_images(AiTool::Grok, "img-idle", "img-run")];
        let running_view = project_pet_overlay_from_drafts(
            &drafts,
            &[PetOverlayToolBehavior {
                tool: AiTool::Grok,
                behavior: HookBehavior::Running,
            }],
        );
        let running_grok = running_view
            .slots
            .iter()
            .find(|slot| slot.tool == AiTool::Grok)
            .expect("grok");
        assert_eq!(running_grok.image_key.as_deref(), Some("img-run"));
        let idle_view = project_pet_overlay_from_drafts(
            &drafts,
            &[PetOverlayToolBehavior {
                tool: AiTool::Grok,
                behavior: HookBehavior::Idle,
            }],
        );
        let idle_grok = idle_view
            .slots
            .iter()
            .find(|slot| slot.tool == AiTool::Grok)
            .expect("grok idle");
        assert_eq!(idle_grok.image_key.as_deref(), Some("img-idle"));
    }

    #[test]
    fn overlay_window_is_frameless_transparent_always_on_top() {
        let spec = pet_overlay_window_spec();
        assert_eq!(spec.label, "pet");
        assert!(!spec.decorations);
        assert!(spec.transparent);
        assert!(spec.always_on_top);
        assert!(!spec.hide_main_window);
        assert!(!spec.stop_hook_listener);
    }
}
