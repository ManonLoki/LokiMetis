// 本文件是 pet_overlay.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

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

    /// 图片可读性和首次定位保留绝对位置，缺失图片不抹除状态文案。
    #[test]
    fn available_images_preserve_absolute_first_slot_and_text() {
        let drafts=[draft_with_content(AiTool::Codex,10,("idle","image"),("running","missing"))];
        let states=[state(AiTool::Codex,9,Some(HookBehavior::Idle),1)];
        let page=project_available_pet_overlay_page(&drafts,&states,PetLayout::Grid,0,|id|id=="image");
        assert!(page.has_any_image&&!page.page_has_image);
        assert_eq!(first_populated_pet_slot(&drafts,&states,|id|id=="image"),Some(9));
        let missing=project_available_pet_overlay_page(&drafts,&states,PetLayout::Grid,2,|_|false);
        assert!(!missing.has_any_image);assert_eq!(missing.slots[1].tile.as_ref().unwrap().content,"idle");
        assert_eq!(first_populated_pet_slot(&drafts,&states,|_|false),None);
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
        assert_eq!(spec.min_cell_size, 64.0);
        assert_eq!((spec.default_width, spec.default_height), (128.0, 128.0));
    }
}
