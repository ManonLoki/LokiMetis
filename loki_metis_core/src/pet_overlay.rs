//! 桌宠悬浮窗的四 Agent 宫格投影与窗口规格；不含局域网发现。

use crate::{AiTool, ai_tool_name};

/// 一张待投影到桌宠槽位的本机图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetOverlayImageRef {
    /// 调用方已知的工具；未知时从 [`Self::label`] 推断。
    pub tool: Option<AiTool>,
    /// 文件名或其它标签，用于推断工具。
    pub label: String,
    /// 本机图库中的稳定键。
    pub image_key: String,
}

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

/// 从文件名一类标签推断桌宠槽位工具；未批准工具返回 `None`。
pub fn pet_overlay_tool_from_label(label: &str) -> Option<AiTool> {
    let normalized = label.to_ascii_lowercase().replace('_', "-");
    if normalized.contains("cursor")
        || normalized.contains("opencode")
        || normalized.contains("open-code")
        || normalized.contains("hermes")
    {
        return None;
    }
    if normalized.contains("workbuddy") || normalized.contains("work-buddy") {
        return Some(AiTool::WorkBuddy);
    }
    if normalized.contains("claude-code")
        || normalized.contains("claudecode")
        || normalized.contains("claude")
    {
        return Some(AiTool::ClaudeCode);
    }
    if normalized.contains("codex") {
        return Some(AiTool::Codex);
    }
    if normalized.contains("grok") {
        return Some(AiTool::Grok);
    }
    None
}

/// 把本机图库投影为固定四槽宫格；同工具多图时后出现的覆盖先前的。
pub fn project_pet_overlay_slots(images: &[PetOverlayImageRef]) -> PetOverlayView {
    let mut keys: [Option<String>; 4] = Default::default();
    for image in images {
        let Some(tool) = image.tool.or_else(|| pet_overlay_tool_from_label(&image.label)) else {
            continue;
        };
        let index = tool_slot_index(tool);
        keys[index] = Some(image.image_key.clone());
    }
    PetOverlayView {
        slots: AiTool::ALL
            .into_iter()
            .enumerate()
            .map(|(index, tool)| {
                let image_key = keys[index].clone();
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

/// 返回已交付的悬浮窗规格。
pub fn pet_overlay_window_spec() -> PetOverlayWindowSpec {
    PET_OVERLAY_WINDOW_SPEC
}

fn tool_slot_index(tool: AiTool) -> usize {
    AiTool::ALL
        .iter()
        .position(|item| *item == tool)
        .expect("AiTool::ALL 必须包含全部已批准工具")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(label: &str, key: &str) -> PetOverlayImageRef {
        PetOverlayImageRef {
            tool: None,
            label: label.to_owned(),
            image_key: key.to_owned(),
        }
    }

    #[test]
    fn overlay_has_exactly_four_approved_agent_slots() {
        let view = project_pet_overlay_slots(&[]);
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

    #[test]
    fn occupied_and_empty_slots_follow_local_images() {
        let view = project_pet_overlay_slots(&[
            image("codex-avatar.png", "img-codex"),
            image("cursor.png", "img-cursor"),
            image("opencode.webp", "img-opencode"),
            image("claude-code.png", "img-claude"),
        ]);
        assert_eq!(view.slots[0].image_key.as_deref(), Some("img-codex"));
        assert!(view.slots[0].occupied);
        assert_eq!(view.slots[1].image_key.as_deref(), Some("img-claude"));
        assert!(view.slots[1].occupied);
        assert!(!view.slots[2].occupied);
        assert!(!view.slots[3].occupied);
        assert!(
            view.slots
                .iter()
                .all(|slot| slot.image_key.as_deref() != Some("img-cursor"))
        );
        assert_eq!(pet_overlay_tool_from_label("cursor.png"), None);
    }

    #[test]
    fn later_image_for_same_tool_replaces_earlier_key() {
        let view = project_pet_overlay_slots(&[
            image("grok-1.png", "first"),
            image("grok-2.png", "second"),
        ]);
        let grok = view
            .slots
            .iter()
            .find(|slot| slot.tool == AiTool::Grok)
            .expect("grok slot");
        assert_eq!(grok.image_key.as_deref(), Some("second"));
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
