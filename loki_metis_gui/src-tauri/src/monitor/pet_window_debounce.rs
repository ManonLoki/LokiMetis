//! 桌宠移动与缩放的 latest-overwrite 防抖通道及受管 worker。

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use loki_metis_core::{HookError, PetLayout, PetOverlayPosition};
use tauri::AppHandle;
use tokio::sync::watch;

use crate::runtime::{AppRuntimeState, BackgroundTaskOwner, BackgroundTaskShutdown};

use super::pet_window::{
    persist_pet_overlay_position, pet_overlay_work_areas, replace_pet_size_if_unchanged,
};
use super::settings::update_monitor_settings;

/// 每次不同快照之后必须保持的静默期；相同快照不会延后落盘。
const PET_WINDOW_DEBOUNCE_QUIET_PERIOD: Duration = Duration::from_millis(200);
/// 移动持久化 worker 的稳定任务名。
const PET_MOVE_DEBOUNCE_TASK_NAME: &str = "pet-position-debounce";
/// 缩放持久化 worker 的稳定任务名。
const PET_RESIZE_DEBOUNCE_TASK_NAME: &str = "pet-size-debounce";

/// 一次待规范化并持久化的桌宠物理位置快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PetMoveSnapshot {
    position: PetOverlayPosition,
    overlay_size: (u32, u32),
}

/// 一次待按旧偏好条件写入的桌宠单格大小快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PetResizeSnapshot {
    layout: PetLayout,
    expected_size: u16,
    applied_size: u16,
}

/// 用 watch 单槽保存最新不同值，并只允许初始化阶段取走一次接收端。
struct LatestDebounceChannel<T> {
    sender: watch::Sender<Option<T>>,
    receiver: Mutex<Option<watch::Receiver<Option<T>>>>,
}

impl<T> LatestDebounceChannel<T> {
    /// 创建尚未提交任何快照的 latest-overwrite 通道。
    fn new() -> Self {
        let (sender, receiver) = watch::channel(None);
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
        }
    }

    /// 只在值确实变化时覆盖单槽，避免原生递归事件无限延后静默期。
    fn submit(&self, value: T) -> bool
    where
        T: PartialEq,
    {
        self.sender.send_if_modified(|current| {
            if current.as_ref() == Some(&value) {
                return false;
            }
            *current = Some(value);
            true
        })
    }

    /// 初始化时唯一一次取走接收端；重复安装必须失败关闭。
    fn take_receiver(&self) -> Result<watch::Receiver<Option<T>>, &'static str> {
        self.receiver
            .lock()
            .map_err(|_| "pet-window-debounce-receiver-lock-poisoned")?
            .take()
            .ok_or("pet-window-debounce-worker-already-initialized")
    }
}

/// 应用状态持有的桌宠移动 latest-overwrite 通道。
pub(crate) struct PetMoveDebounceChannel {
    inner: LatestDebounceChannel<PetMoveSnapshot>,
}

impl PetMoveDebounceChannel {
    /// 创建等待 setup 注册 worker 的移动通道。
    pub(crate) fn new() -> Self {
        Self {
            inner: LatestDebounceChannel::new(),
        }
    }

    /// 提交最新合法物理位置；相同快照静默去重。
    pub(crate) fn submit(&self, position: PetOverlayPosition, overlay_size: (u32, u32)) -> bool {
        self.inner.submit(PetMoveSnapshot {
            position,
            overlay_size,
        })
    }

    /// 为唯一移动 worker 取走接收端。
    fn take_receiver(&self) -> Result<watch::Receiver<Option<PetMoveSnapshot>>, &'static str> {
        self.inner.take_receiver()
    }
}

/// 应用状态持有的桌宠缩放 latest-overwrite 通道。
pub(crate) struct PetResizeDebounceChannel {
    inner: LatestDebounceChannel<PetResizeSnapshot>,
}

impl PetResizeDebounceChannel {
    /// 创建等待 setup 注册 worker 的缩放通道。
    pub(crate) fn new() -> Self {
        Self {
            inner: LatestDebounceChannel::new(),
        }
    }

    /// 提交最新缩放快照；回到当前大小也会覆盖旧待写值。
    pub(crate) fn submit(&self, layout: PetLayout, expected_size: u16, applied_size: u16) -> bool {
        self.inner.submit(PetResizeSnapshot {
            layout,
            expected_size,
            applied_size,
        })
    }

    /// 为唯一缩放 worker 取走接收端。
    fn take_receiver(&self) -> Result<watch::Receiver<Option<PetResizeSnapshot>>, &'static str> {
        self.inner.take_receiver()
    }
}

/// 等待首个值及其完整静默期，只处理最后一个不同值并持续服务后续更新。
async fn run_latest_debounce_worker<T, Apply>(
    mut receiver: watch::Receiver<Option<T>>,
    mut shutdown: BackgroundTaskShutdown,
    quiet_period: Duration,
    mut apply: Apply,
) where
    T: Clone + Send + Sync + 'static,
    Apply: FnMut(T) + Send + 'static,
{
    loop {
        let changed = tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            changed = receiver.changed() => changed,
        };
        if changed.is_err() {
            return;
        }
        let Some(mut pending) = receiver.borrow_and_update().clone() else {
            continue;
        };

        loop {
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => return,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    if let Some(latest) = receiver.borrow_and_update().clone() {
                        pending = latest;
                    }
                    continue;
                }
                _ = tokio::time::sleep(quiet_period) => {
                    // changed 与 timer 同时就绪时上面的 biased 分支优先；这里再复核一次，
                    // 覆盖值恰好在 timer 唤醒边界发布但尚未被 select 观察的情况。
                    match receiver.has_changed() {
                        Ok(true) => {
                            if let Some(latest) = receiver.borrow_and_update().clone() {
                                pending = latest;
                            }
                            continue;
                        }
                        Ok(false) => {}
                        Err(_) => return,
                    }
                    apply(pending);
                    break;
                }
            }
        }
    }
}

/// 通过应用后台 owner 注册一个长期防抖 worker，事件侧不再创建任务。
fn spawn_latest_debounce_worker<T, Apply>(
    owner: &BackgroundTaskOwner,
    name: &'static str,
    receiver: watch::Receiver<Option<T>>,
    quiet_period: Duration,
    apply: Apply,
) -> Result<(), &'static str>
where
    T: Clone + Send + Sync + 'static,
    Apply: FnMut(T) + Send + 'static,
{
    owner.spawn(name, move |shutdown| {
        run_latest_debounce_worker(receiver, shutdown, quiet_period, apply)
    })
}

/// 成对注册 worker；任一注册失败都关闭 owner 并回收已启动的另一条任务。
async fn register_pet_debounce_worker_pair<RegisterMove, RegisterResize>(
    owner: &BackgroundTaskOwner,
    register_move: RegisterMove,
    register_resize: RegisterResize,
) -> Result<(), &'static str>
where
    RegisterMove: FnOnce(&BackgroundTaskOwner) -> Result<(), &'static str>,
    RegisterResize: FnOnce(&BackgroundTaskOwner) -> Result<(), &'static str>,
{
    if let Err(error) = register_move(owner) {
        owner.shutdown().await;
        return Err(error);
    }
    if let Err(error) = register_resize(owner) {
        owner.shutdown().await;
        return Err(error);
    }
    Ok(())
}

/// 按事件读取时的旧布局与大小条件写入单格大小，避免迟到快照覆盖新设置。
fn persist_pet_resize_snapshot(
    config_dir: &Path,
    snapshot: PetResizeSnapshot,
) -> Result<bool, HookError> {
    let mut changed = false;
    update_monitor_settings(config_dir, |current| {
        changed = snapshot.expected_size != snapshot.applied_size
            && replace_pet_size_if_unchanged(
                current,
                snapshot.layout,
                snapshot.expected_size,
                snapshot.applied_size,
            );
    })?;
    Ok(changed)
}

/// setup 阶段注册移动与缩放两个长期 worker；失败时阻止应用继续启动。
pub(crate) async fn install_pet_window_debounce_workers(
    app: AppHandle,
    config_dir: PathBuf,
    state: &AppRuntimeState,
) -> Result<(), String> {
    let move_receiver = state
        .pet_move_debounce
        .take_receiver()
        .map_err(str::to_owned)?;
    let resize_receiver = state
        .pet_resize_debounce
        .take_receiver()
        .map_err(str::to_owned)?;
    let move_app = app.clone();
    let move_config_dir = config_dir.clone();
    let resize_app = app;

    register_pet_debounce_worker_pair(
        &state.background_tasks,
        move |owner| {
            spawn_latest_debounce_worker(
                owner,
                PET_MOVE_DEBOUNCE_TASK_NAME,
                move_receiver,
                PET_WINDOW_DEBOUNCE_QUIET_PERIOD,
                move |snapshot: PetMoveSnapshot| {
                    let work_areas = pet_overlay_work_areas(&move_app);
                    if let Err(error) = persist_pet_overlay_position(
                        &move_config_dir,
                        snapshot.position,
                        snapshot.overlay_size,
                        &work_areas,
                    ) {
                        tracing::warn!(
                            code = error.code,
                            "failed to persist debounced pet position"
                        );
                    }
                },
            )
        },
        move |owner| {
            spawn_latest_debounce_worker(
                owner,
                PET_RESIZE_DEBOUNCE_TASK_NAME,
                resize_receiver,
                PET_WINDOW_DEBOUNCE_QUIET_PERIOD,
                move |snapshot| match persist_pet_resize_snapshot(&config_dir, snapshot) {
                    Ok(true) => super::pet_events::emit_pet_window_state_changed(&resize_app),
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(code = error.code, "failed to persist debounced pet size")
                    }
                },
            )
        },
    )
    .await
    .map_err(|error| format!("failed to initialize pet window debounce workers: {error}"))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use loki_metis_core::{PetOverlayWorkArea, RootDiscoveryCoordinator};
    use tempfile::tempdir;

    use super::*;
    use crate::monitor::settings::load_monitor_settings;

    /// 高频两路事件始终只占两个任务槽，并把各自最后值合并写入同一设置文件。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn burst_updates_keep_two_workers_and_persist_only_latest_values() {
        let temp = tempdir().expect("isolated monitor settings directory");
        let config_dir = temp.path().to_path_buf();
        let move_channel = PetMoveDebounceChannel::new();
        let resize_channel = PetResizeDebounceChannel::new();
        let move_receiver = move_channel.take_receiver().expect("move receiver");
        let resize_receiver = resize_channel.take_receiver().expect("resize receiver");
        let owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        let move_applies = Arc::new(AtomicUsize::new(0));
        let resize_applies = Arc::new(AtomicUsize::new(0));
        let move_applies_for_worker = Arc::clone(&move_applies);
        let resize_applies_for_worker = Arc::clone(&resize_applies);
        let move_config = config_dir.clone();
        let resize_config = config_dir.clone();
        let work_areas = [PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 10_000,
            height: 10_000,
        }];

        register_pet_debounce_worker_pair(
            &owner,
            move |owner| {
                spawn_latest_debounce_worker(
                    owner,
                    "pet-move-burst-test",
                    move_receiver,
                    Duration::from_millis(30),
                    move |snapshot: PetMoveSnapshot| {
                        persist_pet_overlay_position(
                            &move_config,
                            snapshot.position,
                            snapshot.overlay_size,
                            &work_areas,
                        )
                        .expect("latest position persists");
                        move_applies_for_worker.fetch_add(1, Ordering::SeqCst);
                    },
                )
            },
            move |owner| {
                spawn_latest_debounce_worker(
                    owner,
                    "pet-resize-burst-test",
                    resize_receiver,
                    Duration::from_millis(30),
                    move |snapshot| {
                        assert!(
                            persist_pet_resize_snapshot(&resize_config, snapshot)
                                .expect("latest size persists")
                        );
                        resize_applies_for_worker.fetch_add(1, Ordering::SeqCst);
                    },
                )
            },
        )
        .await
        .expect("both workers register");

        for index in 0..64_i32 {
            assert!(move_channel.submit(PetOverlayPosition { x: index, y: index }, (128, 128),));
            assert!(resize_channel.submit(PetLayout::Grid, 64, 65 + index as u16));
        }
        assert_eq!(owner.owned_task_count(), 2);

        tokio::time::timeout(Duration::from_secs(2), async {
            while move_applies.load(Ordering::SeqCst) != 1
                || resize_applies.load(Ordering::SeqCst) != 1
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("both latest snapshots are persisted");

        let stored = load_monitor_settings(&config_dir).expect("settings reload");
        assert_eq!(
            stored.pet_overlay_position,
            Some(PetOverlayPosition { x: 63, y: 63 })
        );
        assert_eq!(stored.pet_window.pet_size, 128);
        assert_eq!(move_applies.load(Ordering::SeqCst), 1);
        assert_eq!(resize_applies.load(Ordering::SeqCst), 1);
        assert_eq!(owner.owned_task_count(), 2);
        owner.shutdown().await;
    }

    /// 每个不同值都重新开始静默期，而重复快照不会制造新的计时版本。
    #[tokio::test]
    async fn different_updates_reset_quiet_period_while_identical_snapshots_are_deduplicated() {
        let channel = PetMoveDebounceChannel::new();
        let receiver = channel.take_receiver().expect("move receiver");
        let owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        let (observed_sender, mut observed_receiver) = tokio::sync::mpsc::unbounded_channel();
        let quiet_period = Duration::from_millis(200);

        spawn_latest_debounce_worker(
            &owner,
            "pet-move-quiet-reset-test",
            receiver,
            quiet_period,
            move |snapshot: PetMoveSnapshot| {
                let _ = observed_sender.send((snapshot, tokio::time::Instant::now()));
            },
        )
        .expect("worker registers");

        let first = PetOverlayPosition { x: 1, y: 1 };
        let latest = PetOverlayPosition { x: 2, y: 2 };
        assert!(channel.submit(first, (128, 128)));
        assert!(!channel.submit(first, (128, 128)));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let latest_submitted_at = tokio::time::Instant::now();
        assert!(channel.submit(latest, (128, 128)));
        assert!(
            tokio::time::timeout(Duration::from_millis(120), observed_receiver.recv())
                .await
                .is_err()
        );
        let (observed, observed_at) =
            tokio::time::timeout(Duration::from_millis(250), observed_receiver.recv())
                .await
                .expect("latest value is eventually applied")
                .expect("worker remains connected");
        assert_eq!(observed.position, latest);
        assert!(observed_at.duration_since(latest_submitted_at) >= quiet_period);
        owner.shutdown().await;
    }

    /// 一次真实设置写失败不会终止 worker，下一次不同快照仍能恢复并落盘。
    #[tokio::test]
    async fn persistence_failure_does_not_stop_later_updates() {
        let temp = tempdir().expect("isolated root");
        let config_dir = temp.path().join("monitor-config");
        std::fs::write(&config_dir, b"blocks directory creation").expect("blocking file");
        let channel = PetMoveDebounceChannel::new();
        let receiver = channel.take_receiver().expect("move receiver");
        let owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        let failures = Arc::new(AtomicUsize::new(0));
        let successes = Arc::new(AtomicUsize::new(0));
        let failures_for_worker = Arc::clone(&failures);
        let successes_for_worker = Arc::clone(&successes);
        let worker_config = config_dir.clone();
        let work_areas = [PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 1_000,
            height: 1_000,
        }];

        spawn_latest_debounce_worker(
            &owner,
            "pet-move-write-recovery-test",
            receiver,
            Duration::from_millis(20),
            move |snapshot: PetMoveSnapshot| match persist_pet_overlay_position(
                &worker_config,
                snapshot.position,
                snapshot.overlay_size,
                &work_areas,
            ) {
                Ok(_) => {
                    successes_for_worker.fetch_add(1, Ordering::SeqCst);
                }
                Err(_) => {
                    failures_for_worker.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .expect("worker registers");

        assert!(channel.submit(PetOverlayPosition { x: 10, y: 10 }, (128, 128)));
        tokio::time::timeout(Duration::from_secs(1), async {
            while failures.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("first write fails observably");
        std::fs::remove_file(&config_dir).expect("remove blocking file");
        assert!(channel.submit(PetOverlayPosition { x: 20, y: 20 }, (128, 128)));
        tokio::time::timeout(Duration::from_secs(1), async {
            while successes.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("later write recovers");

        assert_eq!(failures.load(Ordering::SeqCst), 1);
        assert_eq!(successes.load(Ordering::SeqCst), 1);
        assert_eq!(
            load_monitor_settings(&config_dir)
                .expect("recovered settings reload")
                .pet_overlay_position,
            Some(PetOverlayPosition { x: 20, y: 20 })
        );
        owner.shutdown().await;
    }

    /// 关闭只取消待写快照且不做同步补写；第二条注册失败会先回收第一条。
    #[tokio::test]
    async fn shutdown_discards_pending_value_and_partial_registration_is_reclaimed() {
        let channel = PetMoveDebounceChannel::new();
        let receiver = channel.take_receiver().expect("move receiver");
        let owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        let applies = Arc::new(AtomicUsize::new(0));
        let applies_for_worker = Arc::clone(&applies);

        spawn_latest_debounce_worker(
            &owner,
            "pet-move-shutdown-test",
            receiver,
            Duration::from_millis(40),
            move |_snapshot: PetMoveSnapshot| {
                applies_for_worker.fetch_add(1, Ordering::SeqCst);
            },
        )
        .expect("worker registers");
        assert!(channel.submit(PetOverlayPosition { x: 1, y: 1 }, (128, 128)));
        owner.shutdown().await;
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(applies.load(Ordering::SeqCst), 0);
        assert_eq!(owner.owned_task_count(), 0);

        let partial_owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        let result = register_pet_debounce_worker_pair(
            &partial_owner,
            |owner| {
                owner.spawn("pet-partial-registration-test", |mut shutdown| async move {
                    shutdown.cancelled().await;
                })
            },
            |_owner| Err("injected-second-worker-registration-failure"),
        )
        .await;
        assert_eq!(result, Err("injected-second-worker-registration-failure"));
        assert_eq!(partial_owner.owned_task_count(), 0);
    }
}
