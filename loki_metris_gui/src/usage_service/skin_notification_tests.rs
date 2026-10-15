//! 应用后台终态观察与丢弃窗口回执的生命周期回归，零真实宿主操作。
use super::*;
use crate::{notification_events::Activity, skins::SkinError};
use loki_metris_core::SkinHostKind;
use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};

/// 真实后台 owner 在窗口回执被丢弃后仍完成成功/失败观察并投递通知。
#[test]
fn skin_notification_observer_survives_dropped_window_receipt() {
    for success in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let service = UsageService::in_test_directory(root.path().to_path_buf()).unwrap();
        let queue = service.activity_notifications();
        queue.configure(true);
        let observed = Arc::new(AtomicBool::new(false));
        let done = observed.clone();
        let receiver = service.skin_host_work_observed(
            move |_| async move { if success { Ok(()) } else { Err(SkinError::new("skin.install_failed", "fixture failure")) } },
            move |result| {
                assert_eq!(result.is_ok(), success);
                queue.push(Activity::Skin { host: SkinHostKind::Codex, success: result.is_ok() });
                done.store(true, Ordering::Release);
            },
        ).unwrap();
        drop(receiver);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !observed.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "skin owner must retain terminal observation");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(service.activity_notifications().take().unwrap().activity, Activity::Skin { host: SkinHostKind::Codex, success });
        service.finish();
        assert!(service.activity_notifications().take().is_none());
    }
}
