//! Hook listener 的位置快照解析与带版本迁移写入。

use std::path::Path;

use loki_metis_core::{AiTool, HookTransition, PetOverlayToolState, apply_pet_overlay_transition};

/// 在迁移发生时读取该 Agent 的当前零基展示位置，避免后续改配置重定位历史事件。
pub(super) fn configured_slot_index(config_dir: &Path, tool: AiTool) -> Result<u8, String> {
    super::profiles::load_profile_drafts(config_dir)
        .map_err(|error| error.to_string())?
        .drafts
        .into_iter()
        .find(|draft| draft.tool == tool)
        .map(|draft| draft.slot.saturating_sub(1).min(11))
        .ok_or_else(|| format!("missing monitor profile for {tool:?}"))
}

/// 把状态机迁移记为带全局版本的展示或释放，冲突位置始终由最后迁移决定。
pub(super) fn apply_pet_transition(
    states: &mut Vec<PetOverlayToolState>,
    revision: &mut u64,
    tool: AiTool,
    slot_index: u8,
    transition: HookTransition,
) {
    apply_pet_overlay_transition(states, revision, tool, slot_index, transition);
}
