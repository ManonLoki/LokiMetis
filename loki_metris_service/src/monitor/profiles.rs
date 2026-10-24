//! 位置解析只读取当前产品的完整档案，缺失补齐与默认值由 core 决定。

use loki_metris_core::{
    AiProfileDraft, AiProfileDraftSet, HookError, merge_profile_drafts, validate_profile_draft,
};
use std::{io::Read, path::Path};

/// 档案读取上限阻止损坏文件在事件工作线程无界分配。
const MAX_PROFILE_BYTES: u64 = 128 * 1024;

/// 只限制本应用拥有的配置目录本身，系统祖先目录的合法别名仍可使用。
fn ensure_profile_directory(directory: &Path) -> Result<(), HookError> {
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(HookError::new("error.monitor.profilesInvalid")),
    }
}

/// 只读零写入入口；拒绝符号链接、非常规文件及超量输入。
pub(crate) fn load_profile_drafts(directory: &Path) -> Result<AiProfileDraftSet, HookError> {
    let _guard = super::catalog::read_lock()?;
    load_profile_drafts_unlocked(directory)
}

/// 调用方持有图库读取或修改许可，支持原子保存和删除引用检查。
pub(super) fn load_profile_drafts_unlocked(
    directory: &Path,
) -> Result<AiProfileDraftSet, HookError> {
    ensure_profile_directory(directory)?;
    let path = directory.join("monitor-profiles.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AiProfileDraftSet {
                drafts: merge_profile_drafts(Vec::new()),
            });
        }
        Err(_) => return Err(HookError::new("error.monitor.profilesReadFailed")),
    };
    if !metadata.is_file() || metadata.len() > MAX_PROFILE_BYTES {
        return Err(HookError::new("error.monitor.profilesInvalid"));
    }
    let mut raw = Vec::new();
    std::fs::File::open(&path)
        .and_then(|file| file.take(MAX_PROFILE_BYTES + 1).read_to_end(&mut raw))
        .map_err(|_| HookError::new("error.monitor.profilesReadFailed"))?;
    if raw.len() as u64 > MAX_PROFILE_BYTES {
        return Err(HookError::new("error.monitor.profilesInvalid"));
    }
    let drafts = serde_json::from_slice::<AiProfileDraftSet>(&raw)
        .map(|set| set.drafts)
        .or_else(|_| serde_json::from_slice::<Vec<AiProfileDraft>>(&raw))
        .map_err(|_| HookError::new("error.monitor.profilesInvalid"))?;
    let drafts = merge_profile_drafts(drafts);
    for draft in &drafts {
        loki_metris_core::validate_profile_behaviors(&draft.hooks)?;
    }
    Ok(AiProfileDraftSet { drafts })
}

/// 同一事务内读取最新图库、校验单 Agent 并合并完整档案；失败不覆盖旧值。
pub(crate) fn save_profile_draft(
    directory: &Path,
    data: &Path,
    draft: AiProfileDraft,
) -> Result<AiProfileDraft, HookError> {
    let _guard = super::catalog::write_lock()?;
    let known = super::images::monitor_image_ids_unlocked(data)?;
    let validated = validate_profile_draft(draft, &known)?;
    let mut set = load_profile_drafts_unlocked(directory)?;
    let existing = set
        .drafts
        .iter_mut()
        .find(|item| item.tool == validated.tool)
        .ok_or_else(|| HookError::new("error.hooks.toolUnavailable"))?;
    *existing = validated.clone();
    let bytes = serde_json::to_vec_pretty(&set)
        .map_err(|_| HookError::new("error.monitor.profilesWriteFailed"))?;
    if bytes.len() as u64 > MAX_PROFILE_BYTES {
        return Err(HookError::new("error.monitor.profilesInvalid"));
    }
    std::fs::create_dir_all(directory)
        .map_err(|_| HookError::new("error.monitor.profilesWriteFailed"))?;
    super::atomic_file::write_monitor_file_atomically(
        &directory.join("monitor-profiles.json"),
        &bytes,
        "error.monitor.profilesWriteFailed",
    )?;
    Ok(validated)
}
