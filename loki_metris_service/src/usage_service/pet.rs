//! 桌宠字段级提交与其它监控设置共享串行缓存，操作独立于窗口生存期。

use super::UsageService;
use crate::monitor::settings::MonitorSettings;
use loki_metris_core::{HookError, PetOverlayPosition, PetPreferenceChange, PetSizeRange};
use std::sync::atomic::Ordering;
use tokio::sync::oneshot;

/// 原生退出前从仍存活的窗口取出的最终意图，收尾只消费自身字段。
#[derive(Default)]
pub struct PetShutdownPlan {
    /// 尚未交给后台owner的串行字段意图。
    pub changes: Vec<(PetPreferenceChange, PetSizeRange)>,
    /// 防抖尚未落盘的最终物理桌面位置。
    pub position: Option<PetOverlayPosition>,
}

impl UsageService {
    /// 桌宠偏好代数与Agent选择代数分离，移动窗口不重查用量看板。
    pub fn pet_settings_revision(&self) -> u64 {
        self.context.agents.pet_revision.load(Ordering::Acquire)
    }
    /// 接受字段意图后由现有后台owner保证落盘及退出收尾。
    pub fn change_pet_preferences(
        &self,
        change: PetPreferenceChange,
        range: PetSizeRange,
    ) -> Result<oneshot::Receiver<Result<MonitorSettings, HookError>>, &'static str> {
        self.pet_settings_work(Some((change, range)), None)
    }
    /// 几何防抖只更新位置；绝不提交捕获时的旧布局或Agent快照。
    pub fn save_pet_position(
        &self,
        position: PetOverlayPosition,
    ) -> Result<oneshot::Receiver<Result<MonitorSettings, HookError>>, &'static str> {
        self.pet_settings_work(None, Some(position))
    }
    /// 与runtime关闭互斥，回执被窗口丢弃也不取消已接受的操作。
    fn pet_settings_work(
        &self,
        change: Option<(PetPreferenceChange, PetSizeRange)>,
        position: Option<PetOverlayPosition>,
    ) -> Result<oneshot::Receiver<Result<MonitorSettings, HookError>>, &'static str> {
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtime.is_none() {
            return Err("usage_service_closed");
        }
        let store = self.context.agents.clone();
        let (tx, rx) = oneshot::channel();
        self.background
            .spawn_blocking("pet-settings-mutation", move |_| {
                let _ = tx.send(store.save_pet_blocking(change, position));
            })?;
        drop(runtime);
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metris_core::{AiTool, PetLayout};
    /// 桌宠修改保留目录和空启用集合，随后Agent修改读取同一完整缓存。
    #[test]
    fn pet_mutations_merge_latest_fields_and_do_not_reenable_agents() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("monitor");
        crate::monitor::settings::update_monitor_settings(&config, |settings| {
            settings.enabled_ai_tools.clear();
            settings.hook_directories.codex = "/fixture/codex".into();
        })
        .unwrap();
        let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
        let range = PetSizeRange::for_work_area(1080., PetLayout::Grid);
        let changed = service
            .change_pet_preferences(PetPreferenceChange::Locked(true), range)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .unwrap();
        assert!(changed.pet_window.locked);
        assert!(changed.enabled_ai_tools.is_empty());
        assert_eq!(changed.hook_directories.codex, "/fixture/codex");
        let position = PetOverlayPosition { x: -80, y: 120 };
        service
            .save_pet_position(position)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .unwrap();
        let current = service
            .save_enabled_ai_tools(vec![AiTool::Cursor])
            .unwrap()
            .blocking_recv()
            .unwrap()
            .unwrap();
        assert!(current.pet_window.locked);
        assert_eq!(current.pet_overlay_position, Some(position));
        assert_eq!(service.pet_settings_revision(), 2);
        service.finish();
    }
    /// 损坏配置只能用于展示回退，提交失败不把默认配置写回磁盘。
    #[test]
    fn invalid_pet_settings_are_not_repaired_with_default_agents() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("monitor");
        std::fs::create_dir_all(&config).unwrap();
        let path = crate::monitor::settings::settings_path(&config);
        std::fs::write(&path, b"{broken").unwrap();
        let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
        let work = service
            .change_pet_preferences(
                PetPreferenceChange::Locked(true),
                PetSizeRange::for_work_area(1080., PetLayout::Grid),
            )
            .unwrap();
        assert!(work.blocking_recv().unwrap().is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"{broken");
        assert_eq!(service.pet_settings_revision(), 0);
        service.finish();
    }
}

#[cfg(test)]
mod idempotent_position_tests {
    use super::*;
    /// 同值位置保存不替换原文件、不增长Pet版本；实际B后返回A仍各提交一次。
    #[test]
    fn repeated_physical_position_is_a_noop_but_return_after_b_is_committed() {
        let root = tempfile::tempdir().unwrap();
        let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
        let a = PetOverlayPosition { x: -81, y: 120 };
        let b = PetOverlayPosition { x: -80, y: 120 };
        let save = |position| service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
        save(a);
        let path = crate::monitor::settings::settings_path(&root.path().join("monitor"));
        let mut payload: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        payload["futureFixtureField"] = serde_json::json!("preserve unchanged file bytes");
        let original = serde_json::to_vec(&payload).unwrap();
        std::fs::write(&path, &original).unwrap();
        for _ in 0..4 { assert_eq!(save(a).pet_overlay_position, Some(a)); }
        assert_eq!(service.pet_settings_revision(), 1, "same position must not publish new revisions");
        assert_eq!(std::fs::read(&path).unwrap(), original, "noop must not replace file bytes");
        save(b);
        save(a);
        assert_eq!(service.pet_settings_revision(), 3);
        assert_eq!(crate::monitor::settings::load_monitor_settings(&root.path().join("monitor")).unwrap().pet_overlay_position, Some(a));
        service.finish();
    }

    /// 实际保存从最新磁盘合并，不能把缓存中的旧Pet窗口或Agent/目录字段带回。
    #[test]
    fn position_commit_and_noop_refresh_cache_from_latest_disk_fields() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("monitor");
        let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
        let read = || service.monitor_settings().unwrap().blocking_recv().unwrap().unwrap();
        let before = read();
        assert!(!before.enabled_ai_tools.is_empty(), "precondition: cache starts with old agent fields");
        crate::monitor::settings::update_monitor_settings(&config, |settings| {
            settings.enabled_ai_tools.clear();
            settings.hook_directories.codex = "/fixture/fresh".into();
            settings.pet_window.locked = true;
        }).unwrap();
        let position = PetOverlayPosition { x: -30, y: 44 };
        let saved = service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
        assert!(saved.enabled_ai_tools.is_empty());
        assert_eq!(saved.hook_directories.codex, "/fixture/fresh");
        let cached = read();
        assert!(cached.enabled_ai_tools.is_empty(), "successful commit must update the actual cache");
        assert_eq!(cached.hook_directories.codex, "/fixture/fresh");
        assert!(cached.pet_window.locked);
        assert_eq!(cached.pet_overlay_position, Some(position));
        // Pet字段完全不变，只改变磁盘上的无关字段，强制下一次同位置成为真正noop。
        crate::monitor::settings::update_monitor_settings(&config, |settings| {
            settings.enabled_ai_tools = vec![loki_metris_core::AiTool::Cursor];
            settings.hook_directories.codex = "/fixture/noop-fresh".into();
        }).unwrap();
        let saved = service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
        assert_eq!(saved.hook_directories.codex, "/fixture/noop-fresh");
        let cached = read();
        assert_eq!(cached.enabled_ai_tools, vec![loki_metris_core::AiTool::Cursor], "noop must refresh the actual cache too");
        assert_eq!(cached.hook_directories.codex, "/fixture/noop-fresh");
        assert!(cached.pet_window.locked);
        assert_eq!(cached.pet_overlay_position, Some(position));
        assert_eq!(service.pet_settings_revision(), 1, "unrelated disk fields do not turn a same-position noop into a Pet commit");
        service.finish();
    }
}
