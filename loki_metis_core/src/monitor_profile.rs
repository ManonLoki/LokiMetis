//! 本机 Agent 展示草稿：默认四行为、槽位范围与保存校验。
//!
//! 草稿绑定本机图库 ID，不引入设备归属。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{AiTool, HookBehavior, HookError};

/// 新草稿默认使用的展示位。
pub const DEFAULT_PROFILE_SLOT: u8 = 1;
/// 展示位闭区间最小值。
pub const MIN_PROFILE_SLOT: u8 = 1;
/// 展示位闭区间最大值（单行最多 6 槽）。
pub const MAX_PROFILE_SLOT: u8 = 6;

/// 前端控件使用的闭区间能力。
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorCapabilityRange<T> {
    /// 默认值。
    pub default: T,
    /// 最小值。
    pub min: T,
    /// 最大值。
    pub max: T,
}

/// 返回展示位取值范围。
pub fn profile_slot_range() -> MonitorCapabilityRange<u8> {
    MonitorCapabilityRange {
        default: DEFAULT_PROFILE_SLOT,
        min: MIN_PROFILE_SLOT,
        max: MAX_PROFILE_SLOT,
    }
}

/// 某一展示行为的文案与本机图片 ID。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookContent {
    /// 行为。
    pub behavior: HookBehavior,
    /// 可选文案。
    #[serde(default)]
    pub content: String,
    /// 本机图库 ID；空字符串表示未选图。
    #[serde(default)]
    pub image: String,
}

/// 一个 Agent 的可编辑展示草稿。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiProfileDraft {
    /// 绑定的 Agent。
    pub tool: AiTool,
    /// 展示位 1–25。
    pub slot: u8,
    /// 四种行为的配置。
    #[serde(default)]
    pub hooks: Vec<HookContent>,
}

impl AiProfileDraft {
    /// 按默认槽位和固定行为顺序生成空白草稿。
    pub fn default_for(tool: AiTool) -> Self {
        Self {
            tool,
            slot: DEFAULT_PROFILE_SLOT,
            hooks: HookBehavior::DISPLAY_BEHAVIORS
                .into_iter()
                .map(|behavior| HookContent {
                    behavior,
                    content: String::new(),
                    image: String::new(),
                })
                .collect(),
        }
    }

    /// 已选择图片的行为数量，仅用于完成度展示。
    pub fn configured_behavior_count(&self) -> usize {
        self.hooks
            .iter()
            .filter(|hook| !hook.image.trim().is_empty())
            .count()
    }
}

/// 一次读取得到的完整草稿集合。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiProfileDraftSet {
    /// 四项 Agent 的草稿，缺项由 [`merge_profile_drafts`] 补齐。
    pub drafts: Vec<AiProfileDraft>,
}

/// 为全部已批准 Agent 生成默认草稿。
pub fn default_profile_drafts() -> Vec<AiProfileDraft> {
    AiTool::ALL
        .into_iter()
        .map(AiProfileDraft::default_for)
        .collect()
}

/// 把展示位夹紧到当前闭区间。
pub fn clamp_profile_slot(slot: u8) -> u8 {
    slot.clamp(MIN_PROFILE_SLOT, MAX_PROFILE_SLOT)
}

/// 按固定工具顺序合并已保存草稿；未知工具丢弃，缺项用默认草稿补齐。
pub fn merge_profile_drafts(saved: Vec<AiProfileDraft>) -> Vec<AiProfileDraft> {
    AiTool::ALL
        .into_iter()
        .map(|tool| {
            let mut draft = saved
                .iter()
                .find(|draft| draft.tool == tool)
                .cloned()
                .unwrap_or_else(|| AiProfileDraft::default_for(tool));
            draft.slot = clamp_profile_slot(draft.slot);
            draft
        })
        .collect()
}

/// 只保留当前已启用 Agent 的草稿；未启用时得到空列表。
pub fn visible_profile_drafts(
    drafts: &[AiProfileDraft],
    enabled: &[AiTool],
) -> Vec<AiProfileDraft> {
    drafts
        .iter()
        .filter(|draft| enabled.contains(&draft.tool))
        .cloned()
        .collect()
}

/// 保存前校验槽位、四行为完整性，以及所选图片必须存在于本机图库。
pub fn validate_profile_draft(
    mut draft: AiProfileDraft,
    known_image_ids: &HashSet<String>,
) -> Result<AiProfileDraft, HookError> {
    if !(MIN_PROFILE_SLOT..=MAX_PROFILE_SLOT).contains(&draft.slot) {
        return Err(HookError::new("error.monitor.slotOutOfRange")
            .param("min", MIN_PROFILE_SLOT.to_string())
            .param("max", MAX_PROFILE_SLOT.to_string()));
    }
    if draft.hooks.len() != HookBehavior::DISPLAY_BEHAVIORS.len() {
        return Err(HookError::new("error.monitor.behaviorsIncomplete"));
    }
    let mut behaviors = HashSet::new();
    for hook in &mut draft.hooks {
        hook.content = hook.content.trim().to_owned();
        hook.image = hook.image.trim().to_owned();
        if !hook.image.is_empty() && !known_image_ids.contains(&hook.image) {
            return Err(HookError::new("error.monitor.unknownImage"));
        }
        if !behaviors.insert(hook.behavior) {
            return Err(HookError::new("error.monitor.behaviorDuplicate"));
        }
    }
    if !HookBehavior::DISPLAY_BEHAVIORS
        .iter()
        .all(|behavior| behaviors.contains(behavior))
    {
        return Err(HookError::new("error.monitor.behaviorsIncomplete"));
    }
    Ok(draft)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        AiProfileDraft, DEFAULT_PROFILE_SLOT, MAX_PROFILE_SLOT, MIN_PROFILE_SLOT,
        clamp_profile_slot, merge_profile_drafts, profile_slot_range, validate_profile_draft,
        visible_profile_drafts,
    };
    use crate::{AiTool, HookBehavior};

    fn known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn complete_draft(tool: AiTool) -> AiProfileDraft {
        let mut draft = AiProfileDraft::default_for(tool);
        for (index, hook) in draft.hooks.iter_mut().enumerate() {
            hook.image = format!("img-{index}");
        }
        draft
    }

    #[test]
    fn default_draft_covers_four_empty_behaviors() {
        let draft = AiProfileDraft::default_for(AiTool::Codex);
        assert_eq!(draft.slot, DEFAULT_PROFILE_SLOT);
        assert_eq!(
            draft
                .hooks
                .iter()
                .map(|hook| hook.behavior)
                .collect::<Vec<_>>(),
            HookBehavior::DISPLAY_BEHAVIORS
        );
        assert_eq!(draft.configured_behavior_count(), 0);
        assert!(
            draft
                .hooks
                .iter()
                .all(|hook| hook.content.is_empty() && hook.image.is_empty())
        );
    }

    #[test]
    fn merge_fills_missing_tools_and_drops_unknown_order() {
        let mut saved = AiProfileDraft::default_for(AiTool::Grok);
        saved.slot = 3;
        let merged = merge_profile_drafts(vec![saved]);
        let tools: Vec<_> = merged.iter().map(|draft| draft.tool).collect();
        assert_eq!(
            tools,
            vec![
                AiTool::Codex,
                AiTool::ClaudeCode,
                AiTool::Grok,
                AiTool::WorkBuddy
            ]
        );
        assert_eq!(merged[2].slot, 3);
        assert_eq!(merged[0].slot, DEFAULT_PROFILE_SLOT);
    }

    #[test]
    fn merge_clamps_saved_slot_into_the_six_slot_range() {
        let mut saved = AiProfileDraft::default_for(AiTool::Codex);
        saved.slot = 25;
        let merged = merge_profile_drafts(vec![saved]);
        assert_eq!(merged[0].slot, MAX_PROFILE_SLOT);
        assert_eq!(profile_slot_range().max, 6);
        assert_eq!(clamp_profile_slot(0), MIN_PROFILE_SLOT);
        assert_eq!(clamp_profile_slot(7), MAX_PROFILE_SLOT);
    }

    #[test]
    fn no_enabled_tools_yields_empty_visible_drafts() {
        let drafts = merge_profile_drafts(Vec::new());
        assert!(visible_profile_drafts(&drafts, &[]).is_empty());
        let visible = visible_profile_drafts(&drafts, &[AiTool::Codex]);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].tool, AiTool::Codex);
    }

    #[test]
    fn validate_rejects_slot_out_of_range_and_unknown_image() {
        let mut draft = complete_draft(AiTool::Codex);
        draft.slot = MAX_PROFILE_SLOT + 1;
        let slot_error =
            validate_profile_draft(draft.clone(), &known(&["img-0", "img-1", "img-2", "img-3"]))
                .expect_err("slot");
        assert_eq!(slot_error.code, "error.monitor.slotOutOfRange");

        draft.slot = 2;
        let unknown = validate_profile_draft(draft, &known(&["img-0"])).expect_err("unknown");
        assert_eq!(unknown.code, "error.monitor.unknownImage");
    }

    #[test]
    fn validate_accepts_incomplete_images_when_ids_exist_or_empty() {
        let draft = AiProfileDraft::default_for(AiTool::WorkBuddy);
        let saved = validate_profile_draft(draft, &known(&[])).expect("empty images allowed");
        assert_eq!(saved.configured_behavior_count(), 0);

        let complete = complete_draft(AiTool::ClaudeCode);
        let ids = known(&["img-0", "img-1", "img-2", "img-3"]);
        let saved = validate_profile_draft(complete, &ids).expect("complete");
        assert_eq!(saved.configured_behavior_count(), 4);
    }
}
