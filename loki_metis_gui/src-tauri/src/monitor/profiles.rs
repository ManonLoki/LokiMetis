//! 本机展示草稿的读写；校验与默认值由 core 决定。

use std::{
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

use loki_metis_core::{
    AiProfileDraft, AiProfileDraftSet, HookError, merge_profile_drafts, validate_profile_draft,
};

use super::images::monitor_image_ids;

/// 展示草稿文件的进程内读写锁，避免 listener 与设置页观察到覆盖写中间态。
static MONITOR_PROFILES_LOCK: OnceLock<RwLock<()>> = OnceLock::new();

/// 返回展示草稿文件的全局读写锁。
fn monitor_profiles_lock() -> &'static RwLock<()> {
    MONITOR_PROFILES_LOCK.get_or_init(|| RwLock::new(()))
}

/// 草稿文件路径。
fn profiles_path(config_dir: &Path) -> PathBuf {
    config_dir.join("monitor-profiles.json")
}

/// 读取并补齐全部受支持 Agent 的展示草稿。
pub fn load_profile_drafts(config_dir: &Path) -> Result<AiProfileDraftSet, HookError> {
    let _guard = monitor_profiles_lock().read().map_err(|_| {
        HookError::new("error.monitor.profilesReadFailed")
            .param("detail", "monitor profiles lock poisoned")
    })?;
    load_profile_drafts_unlocked(config_dir)
}

/// 在调用方已持有草稿读锁或写锁时读取并补齐全部受支持 Agent。
fn load_profile_drafts_unlocked(config_dir: &Path) -> Result<AiProfileDraftSet, HookError> {
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
    update_profile_drafts(config_dir, |set| {
        if let Some(existing) = set
            .drafts
            .iter_mut()
            .find(|item| item.tool == validated.tool)
        {
            *existing = validated.clone();
        } else {
            set.drafts.push(validated.clone());
            set.drafts = merge_profile_drafts(std::mem::take(&mut set.drafts));
        }
        Ok(validated)
    })
}

/// 在同一写锁内完成草稿 load→mutate→原子保存，并返回调用方结果。
fn update_profile_drafts<T>(
    config_dir: &Path,
    mutate: impl FnOnce(&mut AiProfileDraftSet) -> Result<T, HookError>,
) -> Result<T, HookError> {
    let _guard = monitor_profiles_lock().write().map_err(|_| {
        HookError::new("error.monitor.profilesWriteFailed")
            .param("detail", "monitor profiles lock poisoned")
    })?;
    let mut set = load_profile_drafts_unlocked(config_dir)?;
    let result = mutate(&mut set)?;
    std::fs::create_dir_all(config_dir).map_err(|error| {
        HookError::new("error.monitor.profilesWriteFailed").param("detail", error.to_string())
    })?;
    let raw = serde_json::to_string_pretty(&set).map_err(|error| {
        HookError::new("error.monitor.profilesWriteFailed").param("detail", error.to_string())
    })?;
    super::atomic_file::write_monitor_file_atomically(
        &profiles_path(config_dir),
        raw.as_bytes(),
        "error.monitor.profilesWriteFailed",
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::AiTool;
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };
    use tempfile::tempdir;

    /// 依次保存不同工具档案时应保留两次更新。
    #[test]
    fn saving_distinct_profiles_keeps_both_updates() {
        let root = tempdir().expect("temp");
        let config = root.path().join("config");
        let data = root.path().join("data");
        let mut codex = AiProfileDraft::default_for(AiTool::Codex);
        codex.slot = 2;
        let mut grok = AiProfileDraft::default_for(AiTool::Grok);
        grok.slot = 9;

        save_profile_draft(&config, &data, codex).expect("codex");
        save_profile_draft(&config, &data, grok).expect("grok");

        let set = load_profile_drafts(&config).expect("profiles");
        assert_eq!(
            set.drafts
                .iter()
                .find(|draft| draft.tool == AiTool::Codex)
                .map(|draft| draft.slot),
            Some(2)
        );
        assert_eq!(
            set.drafts
                .iter()
                .find(|draft| draft.tool == AiTool::Grok)
                .map(|draft| draft.slot),
            Some(9)
        );
    }

    /// 第二个 writer 必须等第一个完整提交，不能在旧快照上并发 merge。
    #[test]
    fn concurrent_profile_updates_serialize_load_merge_and_write() {
        let root = tempdir().expect("temp");
        let config = Arc::new(root.path().join("config"));
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (second_ready_tx, second_ready_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let first_config = Arc::clone(&config);
        let first = thread::spawn(move || {
            update_profile_drafts(&first_config, |set| {
                first_entered_tx.send(()).expect("first entered");
                release_first_rx.recv().expect("release first");
                let mut draft = AiProfileDraft::default_for(AiTool::Codex);
                draft.slot = 2;
                set.drafts.retain(|item| item.tool != AiTool::Codex);
                set.drafts.push(draft);
                Ok(())
            })
            .expect("first update");
        });
        first_entered_rx.recv().expect("first writer entered");
        let second_config = Arc::clone(&config);
        let second = thread::spawn(move || {
            second_ready_tx.send(()).expect("second ready");
            update_profile_drafts(&second_config, |set| {
                second_entered_tx.send(()).expect("second entered");
                let mut draft = AiProfileDraft::default_for(AiTool::Grok);
                draft.slot = 9;
                set.drafts.retain(|item| item.tool != AiTool::Grok);
                set.drafts.push(draft);
                Ok(())
            })
            .expect("second update");
        });
        second_ready_rx.recv().expect("second writer started");
        assert_eq!(
            second_entered_rx.recv_timeout(Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        release_first_tx.send(()).expect("release first writer");
        first.join().expect("first thread");
        second_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second writer entered after first commit");
        second.join().expect("second thread");

        let set = load_profile_drafts(&config).expect("profiles");
        assert_eq!(
            set.drafts
                .iter()
                .find(|draft| draft.tool == AiTool::Codex)
                .map(|draft| draft.slot),
            Some(2)
        );
        assert_eq!(
            set.drafts
                .iter()
                .find(|draft| draft.tool == AiTool::Grok)
                .map(|draft| draft.slot),
            Some(9)
        );
    }
}
