//! 本机展示草稿的读写；校验与默认值由 core 决定。

use std::path::{Path, PathBuf};

use loki_metis_core::{
    AiProfileDraft, AiProfileDraftSet, HookError, merge_profile_drafts, validate_profile_draft,
};

use super::images::monitor_image_ids;

/// 草稿文件路径。
fn profiles_path(config_dir: &Path) -> PathBuf {
    config_dir.join("monitor-profiles.json")
}

/// 读取并补齐四项 Agent 的展示草稿。
pub fn load_profile_drafts(config_dir: &Path) -> Result<AiProfileDraftSet, HookError> {
    let path = profiles_path(config_dir);
    let saved = if path.exists() {
        let raw = std::fs::read_to_string(&path).map_err(|error| {
            HookError::new("error.monitor.profilesReadFailed").param("detail", error.to_string())
        })?;
        serde_json::from_str::<AiProfileDraftSet>(&raw)
            .map(|set| set.drafts)
            .or_else(|_| serde_json::from_str::<Vec<AiProfileDraft>>(&raw))
            .map_err(|error| {
                HookError::new("error.monitor.profilesInvalid").param("detail", error.to_string())
            })?
    } else {
        Vec::new()
    };
    Ok(AiProfileDraftSet {
        drafts: merge_profile_drafts(saved),
    })
}

/// 校验并保存单个 Agent 的展示草稿。
pub fn save_profile_draft(
    config_dir: &Path,
    app_data_dir: &Path,
    draft: AiProfileDraft,
) -> Result<AiProfileDraft, HookError> {
    let known_ids = monitor_image_ids(app_data_dir)?;
    let validated = validate_profile_draft(draft, &known_ids)?;
    let mut set = load_profile_drafts(config_dir)?;
    if let Some(existing) = set
        .drafts
        .iter_mut()
        .find(|item| item.tool == validated.tool)
    {
        *existing = validated.clone();
    } else {
        set.drafts.push(validated.clone());
        set.drafts = merge_profile_drafts(set.drafts);
    }
    std::fs::create_dir_all(config_dir).map_err(|error| {
        HookError::new("error.monitor.profilesWriteFailed").param("detail", error.to_string())
    })?;
    let raw = serde_json::to_string_pretty(&set).map_err(|error| {
        HookError::new("error.monitor.profilesWriteFailed").param("detail", error.to_string())
    })?;
    std::fs::write(profiles_path(config_dir), raw).map_err(|error| {
        HookError::new("error.monitor.profilesWriteFailed").param("detail", error.to_string())
    })?;
    Ok(validated)
}
