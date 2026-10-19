//! 真实后台owner、store与退出次序回归；测试屏障仅在本cfg(test)模块存在。

use super::*;
use loki_metris_core::PetOverlayPosition;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

/// 真实store mutation锁的测试持有者；panic也释放并join自己的线程。
pub(crate) struct MutationBlocker {
    /// 放行持锁线程的信号。
    release: Option<mpsc::Sender<()>>,
    /// 持有 mutation 锁的线程。
    worker: Option<JoinHandle<()>>,
}
impl MutationBlocker {
    /// 解除实际值锁，使已经接受的正式store写入继续。
    pub(crate) fn release(mut self) {
        self.unblock();
    }
    /// 放行被卡住的保存并等持锁线程退出，保证后续断言看到的是确定状态。
    fn unblock(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.join().expect("test mutation lock holder joins");
        }
    }
}
impl Drop for MutationBlocker {
    /// 测试结束时无论成败都放行并回收线程，避免锁一直被占住。
    fn drop(&mut self) {
        self.unblock();
    }
}
/// 不改变生产调用点，只以测试线程持有与正式保存完全相同的mutex。
pub(crate) fn block_pet_mutations(service: &Arc<UsageService>) -> MutationBlocker {
    let store = service.context.agents.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _guard = store
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = entered_tx.send(());
        let _ = release_rx.recv();
    });
    let blocker = MutationBlocker {
        release: Some(release_tx),
        worker: Some(worker),
    };
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actual mutation lock acquired");
    blocker
}
/// Fixture没有其它background工作；B已接受且被值锁阻塞时必须保持真实登记。
pub(crate) fn pet_background_task_count(service: &UsageService) -> usize {
    service.background.owned_task_count()
}

/// 受控晚到的正式store工作；接受/句柄所有权/超时均使用真实service.background。
struct DelayedPosition {
    /// 放行晚到工作的信号。
    release: Option<mpsc::Sender<()>>,
    /// 工作结果的接收端。
    receipt: Option<mpsc::Receiver<Result<MonitorSettings, HookError>>>,
}
impl DelayedPosition {
    /// 放行延迟的位置写入，并等待它的最终回执（成功或失败）返回。
    fn release_and_wait(mut self) -> Result<MonitorSettings, HookError> {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        self.receipt
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .expect("late actual store write completes")
    }
}
impl Drop for DelayedPosition {
    /// 测试结束时放行未完成的写入并有界等待回执，避免后台线程泄漏到别的测试。
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(receipt) = self.receipt.take() {
            let _ = receipt.recv_timeout(Duration::from_secs(5));
        }
    }
}
/// 与正式pet_settings_work调用同一store方法，屏障位于进入store之前而不替换store行为。
fn accept_delayed_position(
    service: &UsageService,
    position: PetOverlayPosition,
) -> DelayedPosition {
    let store = service.context.agents.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (receipt_tx, receipt_rx) = mpsc::channel();
    service
        .background
        .spawn_blocking("pet-late-position-regression", move |_| {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
            let _ = receipt_tx.send(store.save_pet_blocking(None, Some(position)));
        })
        .unwrap();
    let delayed = DelayedPosition {
        release: Some(release_tx),
        receipt: Some(receipt_rx),
    };
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("real blocking work has started");
    delayed
}
/// 跨原5秒关闭预算存活的旧B实际晚于final A进store；旧实现应观察到磁盘被B覆盖。
#[test]
fn late_accepted_position_cannot_overwrite_final_position_after_shutdown_budget() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let a = PetOverlayPosition { x: -10, y: 20 };
    let b = PetOverlayPosition { x: 30, y: 40 };
    let c = PetOverlayPosition { x: 50, y: 60 };
    let initial = MonitorSettings {
        pet_overlay_position: Some(c),
        ..Default::default()
    };
    crate::monitor::settings::save_monitor_settings(&config, &initial).unwrap();
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    let late = accept_delayed_position(&service, b);
    assert_eq!(pet_background_task_count(&service), 1);
    service.finish_with_pet(super::super::PetShutdownPlan {
        position: Some(a),
        ..Default::default()
    });
    assert_eq!(
        crate::monitor::settings::load_monitor_settings(&config)
            .unwrap()
            .pet_overlay_position,
        Some(a),
        "precondition: final A is already on disk while B remains held"
    );
    // runtime已被首次finish取走，重复finish不能把最终位置改成C。
    service.finish_with_pet(super::super::PetShutdownPlan {
        position: Some(c),
        ..Default::default()
    });
    assert_eq!(
        crate::monitor::settings::load_monitor_settings(&config)
            .unwrap()
            .pet_overlay_position,
        Some(a)
    );
    let receipt = late.release_and_wait().unwrap();
    let actual = crate::monitor::settings::load_monitor_settings(&config).unwrap();
    assert_eq!(
        actual.pet_overlay_position,
        Some(a),
        "late accepted B must not overwrite the observed committed final A"
    );
    assert_eq!(
        receipt.pet_overlay_position,
        Some(a),
        "late receipt must describe the position actually saved"
    );
    assert_eq!(
        service.pet_settings_revision(),
        1,
        "late B coerced to already saved A is a noop"
    );
}

/// 首次None plan不设空屏障，重复finish即使带位置也不重新取得已关闭runtime的命令所有权。
#[test]
fn none_plan_and_repeated_finish_leave_late_position_intent_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    let a = PetOverlayPosition { x: 10, y: 20 };
    let b = PetOverlayPosition { x: 30, y: 40 };
    service.finish();
    service.finish_with_pet(super::super::PetShutdownPlan {
        position: Some(a),
        ..Default::default()
    });
    let receipt = service
        .context
        .agents
        .save_pet_blocking(None, Some(b))
        .unwrap();
    assert_eq!(receipt.pet_overlay_position, Some(b));
    assert_eq!(
        crate::monitor::settings::load_monitor_settings(&config)
            .unwrap()
            .pet_overlay_position,
        Some(b)
    );
}

/// 发布终止命令不等已有mutation锁；正式保存取得同一锁后才选择有效位置。
#[test]
fn terminal_publication_does_not_wait_for_held_mutation_lock() {
    let root = tempfile::tempdir().unwrap();
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    let store = service.context.agents.clone();
    let a = PetOverlayPosition { x: -1, y: 2 };
    let b = PetOverlayPosition { x: 3, y: 4 };
    let guard = store.value.lock().unwrap();
    store.freeze_final_pet_position(Some(a));
    drop(guard);
    let receipt = store.save_pet_blocking(None, Some(b)).unwrap();
    assert_eq!(receipt.pet_overlay_position, Some(a));
    service.finish();
}

/// 失败不制造默认成功事实；终止命令留存，之后可写的旧accepted位置只能尝试最终A。
#[test]
fn failed_final_write_keeps_terminal_intent_without_publishing_false_commit() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    std::fs::create_dir_all(&config).unwrap();
    let path = crate::monitor::settings::settings_path(&config);
    std::fs::write(&path, b"{broken").unwrap();
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    let a = PetOverlayPosition { x: -10, y: 20 };
    let b = PetOverlayPosition { x: 30, y: 40 };
    service.finish_with_pet(super::super::PetShutdownPlan {
        position: Some(a),
        ..Default::default()
    });
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    assert_eq!(service.pet_settings_revision(), 0);
    // 只修复当前临时fixture文件；正式store随后仍须使用保留的终止命令。
    crate::monitor::settings::save_monitor_settings(&config, &MonitorSettings::default()).unwrap();
    let receipt = service
        .context
        .agents
        .save_pet_blocking(None, Some(b))
        .unwrap();
    assert_eq!(receipt.pet_overlay_position, Some(a));
    assert_eq!(service.pet_settings_revision(), 1);
    assert_eq!(
        crate::monitor::settings::load_monitor_settings(&config)
            .unwrap()
            .pet_overlay_position,
        Some(a)
    );
}

/// 终止位置之后的偏好None工作继续fresh合并，不从捕获快照回写旧位置或无关字段。
#[test]
fn late_preference_without_position_preserves_final_position_and_fresh_fields() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("monitor");
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    let a = PetOverlayPosition { x: 11, y: 22 };
    service.finish_with_pet(super::super::PetShutdownPlan {
        position: Some(a),
        ..Default::default()
    });
    crate::monitor::settings::update_monitor_settings(&config, |settings| {
        settings.enabled_ai_tools.clear();
        settings.hook_directories.codex = "/fixture/late-preference".into();
    })
    .unwrap();
    let range =
        loki_metris_core::PetSizeRange::for_work_area(1080., loki_metris_core::PetLayout::Grid);
    let receipt = service
        .context
        .agents
        .save_pet_blocking(
            Some((loki_metris_core::PetPreferenceChange::Locked(true), range)),
            None,
        )
        .unwrap();
    assert_eq!(receipt.pet_overlay_position, Some(a));
    assert!(receipt.pet_window.locked);
    assert!(receipt.enabled_ai_tools.is_empty());
    assert_eq!(receipt.hook_directories.codex, "/fixture/late-preference");
    assert_eq!(
        crate::monitor::settings::load_monitor_settings(&config)
            .unwrap()
            .pet_overlay_position,
        Some(a)
    );
}
