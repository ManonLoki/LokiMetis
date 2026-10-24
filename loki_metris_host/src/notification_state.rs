//! 单 owner 的应用私有文件驱动；业务状态转换、压缩、重试与去重全部属于 core。
use loki_metris_core::usage_milestones::{
    MilestoneIssue, MilestoneState, NOTIFICATION_STATE_LIMIT,
};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

/// 已成功落盘的状态是唯一发送依据，失败变更不会提交到内存。
pub struct Ledger {
    /// 账本文件路径。
    path: PathBuf,
    /// 最近一次成功落盘的里程碑状态。
    pub state: MilestoneState,
    /// 已有账本读取或解析失败；此时状态以 Storage 问题初始化，并作为错误报告给设置页。
    pub read_error: bool,
}
impl Ledger {
    /// 有界读取；损坏或未知 schema 明确重建基线并向设置保留错误。
    pub fn load(path: PathBuf) -> Self {
        let result = std::fs::File::open(&path).and_then(|file| {
            let mut bytes = vec![];
            file.take(NOTIFICATION_STATE_LIMIT as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > NOTIFICATION_STATE_LIMIT {
                return Err(std::io::ErrorKind::InvalidData.into());
            }
            let mut state: MilestoneState =
                serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
            if !state.upgrade_policy() {
                return Err(std::io::ErrorKind::InvalidData.into());
            }
            Ok(state)
        });
        let (state, read_error) = match result {
            Ok(state) => (state, false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (MilestoneState::default(), false)
            }
            Err(_) => (
                MilestoneState {
                    issue: Some(MilestoneIssue::Storage),
                    initialized: true,
                    ..Default::default()
                },
                true,
            ),
        };
        Self {
            path,
            state,
            read_error,
        }
    }
    /// 同值零写盘；先同步临时文件再原子替换，只有真实成功才更新已确认状态。
    pub fn commit(&mut self, next: MilestoneState) -> Result<bool, &'static str> {
        if next == self.state {
            return Ok(false);
        }
        if !next.valid() {
            return Err("notification_state_invalid");
        }
        let bytes = serde_json::to_vec(&next).map_err(|_| "notification_state_encode")?;
        if bytes.len() > NOTIFICATION_STATE_LIMIT {
            return Err("notification_state_capacity");
        }
        let parent = self.path.parent().ok_or("notification_state_directory")?;
        std::fs::create_dir_all(parent).map_err(|_| "notification_state_write")?;
        let mut file =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| "notification_state_write")?;
        file.write_all(&bytes)
            .and_then(|_| file.as_file().sync_all())
            .map_err(|_| "notification_state_write")?;
        file.persist(&self.path)
            .map_err(|_| "notification_state_write")?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|_| "notification_state_write")?;
        self.state = next;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 同值不写文件；只在实际原子提交后更新已确认状态。
    #[test]
    fn notification_ledger_roundtrip_and_noop() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("usage-notification-state.json");
        let mut ledger = Ledger::load(path.clone());
        assert!(!ledger.read_error);
        assert!(!ledger.commit(ledger.state.clone()).unwrap());
        assert!(!path.exists());
        let mut next = ledger.state.clone();
        next.set_enabled(true);
        assert!(ledger.commit(next.clone()).unwrap());
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!ledger.commit(next.clone()).unwrap());
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(Ledger::load(path).state, next);
    }
    /// 损坏、超限和未知schema只回到明确基线；失败保存不推进内存状态。
    #[test]
    fn notification_ledger_corruption_capacity_and_save_failure() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.json");
        for bytes in [
            b"invalid".to_vec(),
            vec![b' '; NOTIFICATION_STATE_LIMIT + 1],
            b"{\"schemaVersion\":2}".to_vec(),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            let ledger = Ledger::load(path.clone());
            assert!(ledger.read_error);
            assert!(ledger.state.needs_baseline);
            assert!(ledger.state.days.is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let mut ledger = Ledger::load(path);
        let old = ledger.state.clone();
        let mut next = old.clone();
        next.set_enabled(true);
        assert!(ledger.commit(next).is_err());
        assert_eq!(ledger.state, old);
    }
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    use loki_metris_core::{
        Completeness, Confidence, TodayAvailability, TodayUsageSnapshot, UsageDay,
        usage_milestones::*,
    };
    /// 文件中已预约而无结果的请求重启后仍保持相同ID且不自动重发。
    #[test]
    fn notification_ledger_reservation_survives_restart_as_unknown() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.json");
        let mut ledger = Ledger::load(path.clone());
        let now = "2026-10-10T12:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        let mut value = TodayUsageSnapshot {
            day: UsageDay::capture(now, &jiff::tz::TimeZone::UTC).unwrap(),
            source: "fixture-all".into(),
            total_tokens: 90_000_000,
            observed_ms: now,
            availability: TodayAvailability::Available,
            completeness: Completeness::Complete,
            confidence: Confidence::Exact,
        };
        let mut next = ledger.state.clone();
        next.set_enabled(true);
        next.observe(&value, now);
        ledger.commit(next).unwrap();
        value.total_tokens = 1_350_000_000;
        let mut next = ledger.state.clone();
        next.observe(&value, now);
        ledger.commit(next).unwrap();
        let mut next = ledger.state.clone();
        let request = next
            .reserve(&value, NotificationPermission::Allowed, now)
            .unwrap();
        ledger.commit(next).unwrap();
        let mut restored = Ledger::load(path);
        let mut recovered = restored.state.clone();
        recovered.recover();
        restored.commit(recovered).unwrap();
        assert_eq!(restored.state.active().unwrap().unknown_count, 1);
        assert!(
            restored
                .state
                .reserve(&value, NotificationPermission::Allowed, now)
                .is_none()
        );
        assert_eq!(milestone_request_id(&value.day.key(), 1), request.id);
        assert_eq!(restored.state.active().unwrap().submitted, 0);
    }
}
