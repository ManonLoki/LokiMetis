//! 桌宠悬浮窗的位置投影、分页布局与窗口规格；不含宿主窗口实现。

use serde::{Deserialize, Serialize};

use crate::{AiProfileDraft, AiTool, HookBehavior, HookTransition, MAX_PROFILE_SLOT, ai_tool_name};

/// 桌宠可配置的固定位置总数。
pub const PET_OVERLAY_SLOT_COUNT: usize = MAX_PROFILE_SLOT as usize;

/// 桌宠支持的六种位置布局。
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PetLayout {
    /// 单个位置。
    Single,
    /// 一行两列。
    Row,
    /// 两行一列。
    Column,
    /// 一行三列。
    Row3,
    /// 三行一列。
    Column3,
    /// 两行两列。
    #[default]
    Grid,
}

impl PetLayout {
    /// 返回布局的行数与列数。
    pub const fn dimensions(self) -> (usize, usize) {
        match self {
            Self::Single => (1, 1),
            Self::Row => (1, 2),
            Self::Column => (2, 1),
            Self::Row3 => (1, 3),
            Self::Column3 => (3, 1),
            Self::Grid => (2, 2),
        }
    }

    /// 返回一页可展示的位置数。
    pub const fn capacity(self) -> usize {
        let (rows, columns) = self.dimensions();
        rows * columns
    }
}

/// 某 Agent 最近一次影响桌宠的迁移状态。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayToolState {
    /// 迁移所属 Agent。
    pub tool: AiTool,
    /// 事件发生时从零开始的绝对位置；后续配置变化不会重定位此状态。
    pub slot_index: u8,
    /// 当前展示行为；`None` 表示最近一次迁移为释放位置。
    pub behavior: Option<HookBehavior>,
    /// 全局单调递增的迁移版本，用于解决多个 Agent 选择同一位置时的覆盖顺序。
    pub revision: u64,
}

/// 把一次已拦截迁移提交到位置状态；每个事件只分配一个全局版本。
pub fn apply_pet_overlay_transition(
    states: &mut Vec<PetOverlayToolState>,
    revision: &mut u64,
    tool: AiTool,
    slot_index: u8,
    transition: HookTransition,
) {
    *revision = revision.saturating_add(1);
    let event_revision = *revision;
    let active_slots = active_slots_for_tool(states, tool);
    match transition {
        HookTransition::Display(behavior) => {
            for old_slot in active_slots
                .into_iter()
                .filter(|old_slot| *old_slot != slot_index)
            {
                replace_position_state(
                    states,
                    PetOverlayToolState {
                        tool,
                        slot_index: old_slot,
                        behavior: None,
                        revision: event_revision,
                    },
                );
            }
            replace_position_state(
                states,
                PetOverlayToolState {
                    tool,
                    slot_index,
                    behavior: Some(behavior),
                    revision: event_revision,
                },
            );
        }
        HookTransition::Release => {
            for old_slot in active_slots {
                replace_position_state(
                    states,
                    PetOverlayToolState {
                        tool,
                        slot_index: old_slot,
                        behavior: None,
                        revision: event_revision,
                    },
                );
            }
        }
    }
}

/// 返回当前确实由指定 Agent 占用的位置，不把已被更新版本覆盖的历史状态算作活跃。
fn active_slots_for_tool(states: &[PetOverlayToolState], tool: AiTool) -> Vec<u8> {
    (0..PET_OVERLAY_SLOT_COUNT as u8)
        .filter(|slot_index| {
            latest_state_for_position(states, *slot_index)
                .is_some_and(|state| state.tool == tool && state.behavior.is_some())
        })
        .collect()
}

/// 用新状态整体替换同一位置的历史记录，维持每位置一条最新真相。
fn replace_position_state(states: &mut Vec<PetOverlayToolState>, state: PetOverlayToolState) {
    states.retain(|current| current.slot_index != state.slot_index);
    states.push(state);
}

/// 返回一个位置中版本最高的状态，兼容升级前可能存在的重复位置记录。
fn latest_state_for_position(
    states: &[PetOverlayToolState],
    slot_index: u8,
) -> Option<&PetOverlayToolState> {
    states
        .iter()
        .filter(|state| state.slot_index == slot_index)
        .max_by_key(|state| state.revision)
}

/// 已占用位置中的动态展示内容。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetOverlayTile {
    /// 当前占用位置的 Agent。
    pub tool: AiTool,
    /// Agent 动态展示名。
    pub name: String,
    /// 当前行为配置的展示文案。
    pub content: String,
    /// 当前行为配置的本机图库键；未选择图片时为空。
    pub image_key: Option<String>,
}

/// 桌宠中的一个纯位置槽位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetOverlaySlot {
    /// 从零开始的绝对位置编号。
    pub slot_index: u8,
    /// 动态占用内容；空位置不携带任何 Agent 身份。
    pub tile: Option<PetOverlayTile>,
}

impl PetOverlaySlot {
    /// 当前位置是否配置了可展示图片。
    pub fn has_image(&self) -> bool {
        self.tile
            .as_ref()
            .and_then(|tile| tile.image_key.as_ref())
            .is_some()
    }
}

/// 桌宠全部十二个位置的一致投影。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetOverlayView {
    /// 按绝对位置编号排列的十二个位置。
    pub slots: Vec<PetOverlaySlot>,
}

/// 桌宠当前布局与页码的一致投影。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PetOverlayPage {
    /// 当前布局。
    pub layout: PetLayout,
    /// 规范化后的从零开始页码。
    pub page_index: usize,
    /// 当前布局对应的总页数。
    pub page_count: usize,
    /// 当前页是否含有至少一张图片。
    pub page_has_image: bool,
    /// 十二个位置中是否含有至少一张图片。
    pub has_any_image: bool,
    /// 当前页按绝对位置编号排列的有效位置；尾页不会生成越界位置。
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
    /// 是否允许用户调整窗口大小。
    pub resizable: bool,
    /// 打开时是否隐藏主窗口。
    pub hide_main_window: bool,
    /// 打开或关闭时是否停止 Hook listener。
    pub stop_hook_listener: bool,
    /// 默认布局。
    pub default_layout: PetLayout,
    /// 默认单元格逻辑边长。
    pub default_cell_size: f64,
    /// 单元格最小逻辑边长。
    pub min_cell_size: f64,
    /// 默认逻辑宽度。
    pub default_width: f64,
    /// 默认逻辑高度。
    pub default_height: f64,
}

/// 已交付的桌宠悬浮窗规格：默认两行两列、每格 64，透明无边框且允许缩放。
pub const PET_OVERLAY_WINDOW_SPEC: PetOverlayWindowSpec = PetOverlayWindowSpec {
    label: "pet",
    decorations: false,
    transparent: true,
    always_on_top: true,
    skip_taskbar: true,
    resizable: true,
    hide_main_window: false,
    stop_hook_listener: false,
    default_layout: PetLayout::Grid,
    default_cell_size: 64.0,
    min_cell_size: 32.0,
    default_width: 128.0,
    default_height: 128.0,
};

/// 从已保存展示草稿和最近迁移投影十二个纯位置；没有行为时不回退空闲图。
pub fn project_pet_overlay_from_drafts(
    drafts: &[AiProfileDraft],
    current_states: &[PetOverlayToolState],
) -> PetOverlayView {
    let slots = (0..PET_OVERLAY_SLOT_COUNT)
        .map(|slot_index| PetOverlaySlot {
            slot_index: slot_index as u8,
            tile: tile_for_position(slot_index, drafts, current_states),
        })
        .collect();
    PetOverlayView { slots }
}

/// 按布局和页码投影当前页；页码自动循环，尾页仅返回十二个位置内的槽位。
pub fn project_pet_overlay_page(
    drafts: &[AiProfileDraft],
    current_states: &[PetOverlayToolState],
    layout: PetLayout,
    page_index: usize,
) -> PetOverlayPage {
    let view = project_pet_overlay_from_drafts(drafts, current_states);
    let page_count = pet_overlay_page_count(layout);
    let page_index = page_index % page_count;
    let start = page_index * layout.capacity();
    let end = (start + layout.capacity()).min(PET_OVERLAY_SLOT_COUNT);
    let has_any_image = view.slots.iter().any(PetOverlaySlot::has_image);
    let slots = view.slots[start..end].to_vec();
    let page_has_image = slots.iter().any(PetOverlaySlot::has_image);
    PetOverlayPage {
        layout,
        page_index,
        page_count,
        page_has_image,
        has_any_image,
        slots,
    }
}

/// 返回十二个位置在指定布局下所需的页数。
pub const fn pet_overlay_page_count(layout: PetLayout) -> usize {
    PET_OVERLAY_SLOT_COUNT.div_ceil(layout.capacity())
}

/// 依据有符号步数循环桌宠页码。
pub fn wrap_pet_overlay_page(layout: PetLayout, page_index: usize, delta: isize) -> usize {
    let page_count = pet_overlay_page_count(layout);
    let normalized = page_index % page_count;
    let wrapped_delta = delta.rem_euclid(page_count as isize) as usize;
    (normalized + wrapped_delta) % page_count
}

/// 选择指定绝对位置中全局版本最新的迁移，并生成动态展示内容。
fn tile_for_position(
    slot_index: usize,
    drafts: &[AiProfileDraft],
    current_states: &[PetOverlayToolState],
) -> Option<PetOverlayTile> {
    let state = latest_state_for_position(current_states, slot_index as u8)?;
    let behavior = state.behavior?;
    let hook = drafts
        .iter()
        .find(|draft| draft.tool == state.tool)
        .and_then(|draft| draft.hooks.iter().find(|hook| hook.behavior == behavior));
    Some(PetOverlayTile {
        tool: state.tool,
        name: ai_tool_name(state.tool).to_owned(),
        content: hook
            .map(|hook| hook.content.trim().to_owned())
            .unwrap_or_default(),
        image_key: hook
            .map(|hook| hook.image.trim())
            .filter(|image| !image.is_empty())
            .map(str::to_owned),
    })
}

/// 返回已交付的悬浮窗规格。
pub fn pet_overlay_window_spec() -> PetOverlayWindowSpec {
    PET_OVERLAY_WINDOW_SPEC
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造带位置、文案与图片的测试草稿。
    fn draft_with_content(
        tool: AiTool,
        slot: u8,
        idle: (&str, &str),
        running: (&str, &str),
    ) -> AiProfileDraft {
        let mut draft = AiProfileDraft::default_for(tool);
        draft.slot = slot;
        for hook in &mut draft.hooks {
            let (content, image) = match hook.behavior {
                HookBehavior::Idle => idle,
                HookBehavior::Running => running,
                HookBehavior::Asking | HookBehavior::Error => ("", ""),
            };
            hook.content = content.to_owned();
            hook.image = image.to_owned();
        }
        draft
    }

    /// 构造指定版本的测试迁移状态。
    fn state(
        tool: AiTool,
        slot_index: u8,
        behavior: Option<HookBehavior>,
        revision: u64,
    ) -> PetOverlayToolState {
        PetOverlayToolState {
            tool,
            slot_index,
            behavior,
            revision,
        }
    }

    /// 验证空投影仍保留十二个无身份、无内容的固定位置。
    #[test]
    fn empty_projection_has_twelve_identity_free_positions() {
        let view = project_pet_overlay_from_drafts(&[], &[]);
        assert_eq!(view.slots.len(), 12);
        assert_eq!(
            view.slots
                .iter()
                .map(|slot| slot.slot_index)
                .collect::<Vec<_>>(),
            (0_u8..12).collect::<Vec<_>>()
        );
        assert!(view.slots.iter().all(|slot| slot.tile.is_none()));
    }

    /// 验证配置中的第九槽会映射到从零计数的第八索引。
    #[test]
    fn configured_slot_nine_maps_to_zero_based_position_eight() {
        let drafts = [draft_with_content(
            AiTool::Grok,
            9,
            ("idle", "img-idle"),
            ("running", "img-running"),
        )];
        let view = project_pet_overlay_from_drafts(
            &drafts,
            &[state(AiTool::Grok, 8, Some(HookBehavior::Running), 1)],
        );
        assert!(view.slots[..8].iter().all(|slot| slot.tile.is_none()));
        let tile = view.slots[8].tile.as_ref().expect("position 9 tile");
        assert_eq!(tile.tool, AiTool::Grok);
        assert_eq!(tile.name, "Grok Build");
        assert_eq!(tile.content, "running");
        assert_eq!(tile.image_key.as_deref(), Some("img-running"));
    }

    /// 验证仅有草稿图片而没有迁移事件时不会产生初始展示块。
    #[test]
    fn draft_images_do_not_create_an_initial_tile_without_an_event() {
        let drafts = [draft_with_content(
            AiTool::Codex,
            1,
            ("idle", "img-idle"),
            ("running", "img-running"),
        )];
        let view = project_pet_overlay_from_drafts(&drafts, &[]);
        assert!(view.slots.iter().all(|slot| slot.tile.is_none()));
    }

    /// 验证多个 Agent 共用位置时由全局版本最新的迁移获胜。
    #[test]
    fn greatest_global_revision_wins_a_shared_position() {
        let drafts = [
            draft_with_content(AiTool::Codex, 3, ("codex", "codex-idle"), ("", "")),
            draft_with_content(AiTool::ClaudeCode, 3, ("claude", "claude-idle"), ("", "")),
        ];
        let view = project_pet_overlay_from_drafts(
            &drafts,
            &[
                state(AiTool::ClaudeCode, 2, Some(HookBehavior::Idle), 8),
                state(AiTool::Codex, 2, Some(HookBehavior::Idle), 7),
            ],
        );
        assert_eq!(
            view.slots[2].tile.as_ref().map(|tile| tile.tool),
            Some(AiTool::ClaudeCode)
        );
    }

    /// 验证最新释放墓碑会清空共用位置，而不会恢复更旧状态。
    #[test]
    fn newest_release_clears_a_shared_position_instead_of_revealing_older_state() {
        let drafts = [
            draft_with_content(AiTool::Codex, 2, ("codex", "codex-idle"), ("", "")),
            draft_with_content(AiTool::WorkBuddy, 2, ("buddy", "buddy-idle"), ("", "")),
        ];
        let view = project_pet_overlay_from_drafts(
            &drafts,
            &[
                state(AiTool::Codex, 1, Some(HookBehavior::Idle), 4),
                state(AiTool::WorkBuddy, 1, None, 5),
            ],
        );
        assert_eq!(view.slots[1].tile, None);
    }

    /// 验证修改草稿槽位不会追溯移动已经发生的展示状态。
    #[test]
    fn profile_change_does_not_move_an_existing_display_state() {
        let drafts = [draft_with_content(
            AiTool::Codex,
            9,
            ("idle", "codex-idle"),
            ("", ""),
        )];
        let view = project_pet_overlay_from_drafts(
            &drafts,
            &[state(AiTool::Codex, 0, Some(HookBehavior::Idle), 4)],
        );
        assert_eq!(
            view.slots[0].tile.as_ref().map(|tile| tile.tool),
            Some(AiTool::Codex)
        );
        assert_eq!(view.slots[8].tile, None);
    }

    /// 验证旧位置的释放墓碑不会清除同一工具更新的新位置。
    #[test]
    fn old_release_tombstone_does_not_clear_the_tools_new_position() {
        let drafts = [draft_with_content(
            AiTool::Codex,
            9,
            ("idle", "codex-idle"),
            ("", ""),
        )];
        let view = project_pet_overlay_from_drafts(
            &drafts,
            &[
                state(AiTool::Codex, 0, None, 5),
                state(AiTool::Codex, 8, Some(HookBehavior::Idle), 6),
            ],
        );
        assert_eq!(view.slots[0].tile, None);
        assert_eq!(
            view.slots[8].tile.as_ref().map(|tile| tile.tool),
            Some(AiTool::Codex)
        );
    }

    /// 验证工具迁移位置时以同一事件版本释放旧位置并占用新位置。
    #[test]
    fn moving_a_tool_releases_its_previous_position_at_the_same_event_revision() {
        let drafts = [draft_with_content(
            AiTool::Codex,
            5,
            ("idle", "codex-idle"),
            ("running", "codex-running"),
        )];
        let mut states = Vec::new();
        let mut revision = 0;
        apply_pet_overlay_transition(
            &mut states,
            &mut revision,
            AiTool::Codex,
            0,
            HookTransition::Display(HookBehavior::Idle),
        );
        apply_pet_overlay_transition(
            &mut states,
            &mut revision,
            AiTool::Codex,
            4,
            HookTransition::Display(HookBehavior::Running),
        );

        assert_eq!(revision, 2);
        assert_eq!(states.len(), 2);
        assert!(states.iter().any(|state| {
            state.slot_index == 0 && state.behavior.is_none() && state.revision == revision
        }));
        assert!(states.iter().any(|state| {
            state.slot_index == 4
                && state.behavior == Some(HookBehavior::Running)
                && state.revision == revision
        }));
        let view = project_pet_overlay_from_drafts(&drafts, &states);
        assert_eq!(view.slots[0].tile, None);
        assert_eq!(
            view.slots[4].tile.as_ref().map(|tile| tile.tool),
            Some(AiTool::Codex)
        );
    }

    /// 验证释放事件会清空该工具仍拥有的全部展示位置。
    #[test]
    fn release_clears_every_position_still_owned_by_the_tool() {
        let mut states = vec![
            state(AiTool::Codex, 0, Some(HookBehavior::Idle), 1),
            state(AiTool::Codex, 4, Some(HookBehavior::Running), 2),
        ];
        let mut revision = 2;

        apply_pet_overlay_transition(
            &mut states,
            &mut revision,
            AiTool::Codex,
            4,
            HookTransition::Release,
        );

        assert_eq!(revision, 3);
        assert!(states.iter().all(|state| state.behavior.is_none()));
        assert!(states.iter().all(|state| state.revision == revision));
    }

    /// 验证释放某工具时不会清除已由另一工具接管的位置。
    #[test]
    fn release_does_not_clear_a_position_now_owned_by_another_tool() {
        let mut states = Vec::new();
        let mut revision = 0;
        apply_pet_overlay_transition(
            &mut states,
            &mut revision,
            AiTool::Codex,
            0,
            HookTransition::Display(HookBehavior::Idle),
        );
        apply_pet_overlay_transition(
            &mut states,
            &mut revision,
            AiTool::ClaudeCode,
            0,
            HookTransition::Display(HookBehavior::Running),
        );
        apply_pet_overlay_transition(
            &mut states,
            &mut revision,
            AiTool::Codex,
            4,
            HookTransition::Display(HookBehavior::Running),
        );
        apply_pet_overlay_transition(
            &mut states,
            &mut revision,
            AiTool::Codex,
            4,
            HookTransition::Release,
        );

        assert_eq!(revision, 4);
        assert!(states.iter().any(|state| {
            state.slot_index == 0
                && state.tool == AiTool::ClaudeCode
                && state.behavior == Some(HookBehavior::Running)
        }));
        assert!(states.iter().any(|state| {
            state.slot_index == 4 && state.tool == AiTool::Codex && state.behavior.is_none()
        }));
    }

    /// 验证每种布局的行列、容量和十二位置分页数量保持一致。
    #[test]
    fn layouts_report_expected_dimensions_capacity_and_page_count() {
        let cases = [
            (PetLayout::Single, (1, 1), 1, 12),
            (PetLayout::Row, (1, 2), 2, 6),
            (PetLayout::Column, (2, 1), 2, 6),
            (PetLayout::Row3, (1, 3), 3, 4),
            (PetLayout::Column3, (3, 1), 3, 4),
            (PetLayout::Grid, (2, 2), 4, 3),
        ];
        for (layout, dimensions, capacity, pages) in cases {
            assert_eq!(layout.dimensions(), dimensions);
            assert_eq!(layout.capacity(), capacity);
            assert_eq!(pet_overlay_page_count(layout), pages);
        }
    }

    /// 验证分页保留绝对位置编号，并按正负步数循环页码。
    #[test]
    fn page_projection_uses_absolute_positions_and_wraps() {
        let page = project_pet_overlay_page(&[], &[], PetLayout::Grid, 4);
        assert_eq!(page.page_index, 1);
        assert_eq!(page.page_count, 3);
        assert_eq!(
            page.slots
                .iter()
                .map(|slot| slot.slot_index)
                .collect::<Vec<_>>(),
            vec![4, 5, 6, 7]
        );
        assert_eq!(wrap_pet_overlay_page(PetLayout::Grid, 0, -1), 2);
        assert_eq!(wrap_pet_overlay_page(PetLayout::Grid, 2, 1), 0);
        assert_eq!(wrap_pet_overlay_page(PetLayout::Grid, 0, isize::MIN), 1);
        assert_eq!(wrap_pet_overlay_page(PetLayout::Grid, 2, isize::MAX), 0);
    }

    /// 验证浮窗规格保持透明、无边框、可缩放的默认网格基线。
    #[test]
    fn window_spec_matches_resizable_grid_baseline() {
        let spec = pet_overlay_window_spec();
        assert_eq!(spec.label, "pet");
        assert!(!spec.decorations);
        assert!(spec.transparent);
        assert!(spec.always_on_top);
        assert!(spec.skip_taskbar);
        assert!(spec.resizable);
        assert!(!spec.hide_main_window);
        assert!(!spec.stop_hook_listener);
        assert_eq!(spec.default_layout, PetLayout::Grid);
        assert_eq!(spec.default_cell_size, 64.0);
        assert_eq!(spec.min_cell_size, 32.0);
        assert_eq!((spec.default_width, spec.default_height), (128.0, 128.0));
    }
}
