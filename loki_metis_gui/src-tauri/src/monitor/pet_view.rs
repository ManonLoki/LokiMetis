//! 桌宠 position-first 分页 DTO 与本机图片读取边界。

use std::{collections::HashSet, path::Path};

use loki_metis_core::{
    AiTool, HookError, PetLayout, PetOverlayPage, PetOverlayToolState,
    project_pet_overlay_from_drafts, project_pet_overlay_page,
};
use serde::Serialize;

use super::images::{read_monitor_image, readable_monitor_image_ids};
use super::profiles::load_profile_drafts;

/// 已占用位置的动态 Agent 展示内容。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayTileDto {
    /// 当前占用位置的 Agent。
    pub tool: AiTool,
    /// 动态展示名。
    pub name: String,
    /// 当前行为配置的文案。
    pub content: String,
    /// 本机图库 ID。
    pub image_id: Option<String>,
}

/// 前端纯位置槽位；空槽不携带 Agent 身份。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlaySlotDto {
    /// 从零开始的绝对位置编号。
    pub slot_index: u8,
    /// 动态占用内容。
    pub tile: Option<PetOverlayTileDto>,
}

/// 前端桌宠当前页快照。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayViewDto {
    /// 当前布局。
    pub layout: PetLayout,
    /// 从零开始的当前页。
    pub page_index: usize,
    /// 当前布局总页数。
    pub page_count: usize,
    /// 当前页是否有图片。
    pub page_has_image: bool,
    /// 十二个位置中是否有图片。
    pub has_any_image: bool,
    /// 当前页位置。
    pub slots: Vec<PetOverlaySlotDto>,
}

/// 把已保存草稿与带版本迁移投影为当前布局的一页 DTO。
pub fn pet_overlay_view_from_drafts(
    config_dir: &Path,
    app_data_dir: &Path,
    states: &[PetOverlayToolState],
    layout: PetLayout,
    page_index: usize,
) -> Result<PetOverlayViewDto, HookError> {
    let drafts = load_profile_drafts(config_dir)?;
    let readable_ids = readable_monitor_image_ids(app_data_dir)?;
    let has_any_image = project_pet_overlay_from_drafts(&drafts.drafts, states)
        .slots
        .iter()
        .filter_map(|slot| slot.tile.as_ref()?.image_key.as_ref())
        .any(|image_id| readable_ids.contains(image_id));
    Ok(to_view_dto(
        project_pet_overlay_page(&drafts.drafts, states, layout, page_index),
        &readable_ids,
        has_any_image,
    ))
}

/// 把 core 页投影转换为前端 camelCase DTO。
fn to_view_dto(
    page: PetOverlayPage,
    readable_ids: &HashSet<String>,
    has_any_image: bool,
) -> PetOverlayViewDto {
    let slots = page
        .slots
        .into_iter()
        .map(|slot| PetOverlaySlotDto {
            slot_index: slot.slot_index,
            tile: slot.tile.map(|tile| PetOverlayTileDto {
                tool: tile.tool,
                name: tile.name,
                content: tile.content,
                image_id: tile
                    .image_key
                    .filter(|image_id| readable_ids.contains(image_id)),
            }),
        })
        .collect::<Vec<_>>();
    let page_has_image = slots.iter().any(|slot| {
        slot.tile
            .as_ref()
            .and_then(|tile| tile.image_id.as_ref())
            .is_some()
    });
    PetOverlayViewDto {
        layout: page.layout,
        page_index: page.page_index,
        page_count: page.page_count,
        page_has_image,
        has_any_image,
        slots,
    }
}

/// 读取一张本机监控图字节。
pub fn overlay_image_bytes(app_data_dir: &Path, id: &str) -> Result<Vec<u8>, HookError> {
    read_monitor_image(app_data_dir, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::HookBehavior;
    use tempfile::tempdir;

    #[test]
    fn current_page_contains_identity_free_empty_positions() {
        let root = tempdir().expect("temp");
        let view = pet_overlay_view_from_drafts(root.path(), root.path(), &[], PetLayout::Grid, 0)
            .expect("empty page");
        assert_eq!(view.page_count, 3);
        assert_eq!(view.slots.len(), 4);
        assert_eq!(view.slots[0].slot_index, 0);
        assert!(view.slots.iter().all(|slot| slot.tile.is_none()));
    }

    #[test]
    fn configured_agent_is_projected_by_event_position_and_revision() {
        let root = tempdir().expect("temp");
        let data = root.path().join("data");
        let config = root.path().join("config");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::create_dir_all(&data).expect("data");
        let mut draft = loki_metis_core::AiProfileDraft::default_for(AiTool::Codex);
        draft.slot = 1;
        for hook in &mut draft.hooks {
            if hook.behavior == HookBehavior::Idle {
                hook.content = "ready".to_owned();
            }
        }
        super::super::save_profile_draft(&config, &data, draft).expect("draft");
        let states = [PetOverlayToolState {
            tool: AiTool::Codex,
            slot_index: 8,
            behavior: Some(HookBehavior::Idle),
            revision: 7,
        }];
        let view = pet_overlay_view_from_drafts(&config, &data, &states, PetLayout::Grid, 2)
            .expect("third page");
        assert_eq!(view.slots[0].slot_index, 8);
        let tile = view.slots[0].tile.as_ref().expect("position 9 tile");
        assert_eq!(tile.tool, AiTool::Codex);
        assert_eq!(tile.name, "Codex");
        assert_eq!(tile.content, "ready");
    }

    #[test]
    fn missing_indexed_file_is_not_exposed_as_a_renderable_image() {
        let root = tempdir().expect("temp");
        let data = root.path().join("data");
        let config = root.path().join("config");
        let images = data.join("monitor-images");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::create_dir_all(&images).expect("images");
        let index = [super::super::images::MonitorImageRecord {
            id: "missing-image".to_owned(),
            filename: "missing.png".to_owned(),
            stored_name: "missing.png".to_owned(),
        }];
        std::fs::write(
            images.join("index.json"),
            serde_json::to_vec_pretty(&index).expect("index json"),
        )
        .expect("index");
        let mut draft = loki_metis_core::AiProfileDraft::default_for(AiTool::Codex);
        for hook in &mut draft.hooks {
            if hook.behavior == HookBehavior::Idle {
                hook.content = "ready".to_owned();
                hook.image = "missing-image".to_owned();
            }
        }
        super::super::save_profile_draft(&config, &data, draft).expect("draft");
        let states = [PetOverlayToolState {
            tool: AiTool::Codex,
            slot_index: 0,
            behavior: Some(HookBehavior::Idle),
            revision: 1,
        }];

        let view = pet_overlay_view_from_drafts(&config, &data, &states, PetLayout::Grid, 0)
            .expect("view");

        let tile = view.slots[0].tile.as_ref().expect("active tile");
        assert_eq!(tile.content, "ready");
        assert_eq!(tile.image_id, None);
        assert!(!view.page_has_image);
        assert!(!view.has_any_image);
    }
}
