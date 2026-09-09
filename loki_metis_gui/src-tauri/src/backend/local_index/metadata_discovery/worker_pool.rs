//! 为普通元数据兜底提供有界目录探测 worker、取消边界与线程所有权。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use loki_metis_core::{RootCandidateEvidence, RootDiscoveryCoordinator};

use super::super::discovery::metadata_is_link_like;
use super::super::is_obviously_network_path;
use super::candidate_from_file_name;

/// 普通元数据兜底的总墙钟预算，避免不可预测的整卷遍历无限占用后台任务。
const METADATA_TRAVERSAL_TIMEOUT: Duration = Duration::from_secs(60);
/// 单目录最多读取的目录项数；超出时保留覆盖缺口并停止该目录。
const DIRECTORY_ENTRY_LIMIT: usize = 10_000;
/// 结果通道的短轮询间隔，用于及时观察取消与总 deadline。
const RESULT_POLL_INTERVAL: Duration = Duration::from_millis(20);
/// 关闭空闲 worker 时允许的短回收窗口；文件系统调用卡住时转交静态 owner。
const WORKER_SHUTDOWN_GRACE: Duration = Duration::from_millis(50);
/// 生产只有一个全局发现协调器，因此卡住的 worker 最多占满既有核心并发上限。
#[cfg(not(test))]
const LIVE_METADATA_WORKER_LIMIT: usize = loki_metis_core::LOCAL_DISCOVERY_WORKER_LIMIT;
/// 单元测试会并行创建隔离协调器，放宽进程级名额以避免夹具互相影响。
#[cfg(test)]
const LIVE_METADATA_WORKER_LIMIT: usize = 64;

/// 全进程仍在执行的元数据 worker 数；卡住的文件系统调用也占用名额。
static LIVE_METADATA_WORKERS: AtomicUsize = AtomicUsize::new(0);
/// 保存无法在关闭窗口内 join 的 worker，防止丢弃句柄形成 detached thread。
static RETAINED_METADATA_WORKERS: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();

/// 一次兜底遍历可在目录项、结果与提交边界观察的停止原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraversalStopReason {
    /// 用户或应用关闭流程已请求取消。
    Cancelled,
    /// 整次普通元数据兜底超过固定墙钟预算。
    DeadlineExceeded,
    /// 单个目录超过目录项上限，覆盖不再完整。
    DirectoryEntryLimit,
    /// worker 或结果通道提前失效，无法证明遍历完成。
    WorkerUnavailable,
}

impl TraversalStopReason {
    /// 返回可向发现状态公开且不含路径的稳定错误码。
    pub(super) const fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Cancelled => None,
            Self::DeadlineExceeded => Some("metadata_discovery_deadline_exceeded"),
            Self::DirectoryEntryLimit => Some("metadata_discovery_directory_limit_exceeded"),
            Self::WorkerUnavailable => Some("metadata_discovery_worker_unavailable"),
        }
    }
}

/// 把兜底遍历的绝对 deadline 与协调器取消信号组合为统一边界检查。
#[derive(Debug, Clone, Copy)]
pub(super) struct MetadataTraversalBudget {
    deadline: Instant,
}

impl MetadataTraversalBudget {
    /// 从当前时刻创建生产兜底遍历的固定总预算。
    pub(super) fn new() -> Self {
        Self::with_timeout(METADATA_TRAVERSAL_TIMEOUT)
    }

    /// 创建指定时长的预算，供生产构造与合同回归共用。
    pub(super) fn with_timeout(timeout: Duration) -> Self {
        Self {
            deadline: Instant::now() + timeout,
        }
    }

    /// 在下一项工作开始前返回取消或 deadline 停止原因；取消优先。
    pub(super) fn stop_reason(
        self,
        coordinator: &RootDiscoveryCoordinator,
    ) -> Option<TraversalStopReason> {
        if coordinator.is_cancel_requested() {
            Some(TraversalStopReason::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(TraversalStopReason::DeadlineExceeded)
        } else {
            None
        }
    }

    /// 返回不越过总 deadline 的短通道等待时长。
    fn result_wait(self) -> Duration {
        self.deadline
            .saturating_duration_since(Instant::now())
            .min(RESULT_POLL_INTERVAL)
    }
}

/// 单个目录一次遍历产出的子目录、候选、覆盖计数与停止原因。
#[derive(Debug, Default)]
pub(super) struct DirectoryResult {
    /// 待继续入队的子目录。
    pub(super) children: Vec<PathBuf>,
    /// 本次遍历命中的候选路径及其证据类型。
    pub(super) candidates: Vec<(PathBuf, RootCandidateEvidence)>,
    /// 本次检查过的文件名数量。
    pub(super) file_names_checked: u64,
    /// 权限拒绝计数。
    pub(super) permission_denied: u64,
    /// I/O 错误计数。
    pub(super) io_errors: u64,
    /// 因链接、网络或目录项上限跳过的数量。
    pub(super) skipped: u64,
    /// 本目录是否因取消、deadline 或目录项上限提前停止。
    pub(super) stop_reason: Option<TraversalStopReason>,
}

/// 一批公平调度任务的已完成结果与批次级停止原因。
#[derive(Debug, Default)]
pub(super) struct InspectionBatch {
    /// 已在取消或 deadline 前完整接收的 worker 结果。
    pub(super) results: Vec<(usize, Result<DirectoryResult, ()>)>,
    /// 等待或投递期间发现的批次级停止原因。
    pub(super) stop_reason: Option<TraversalStopReason>,
}

/// 覆盖整次发现生命周期的固定目录 worker 池，避免每轮反复创建原生线程。
pub(super) struct MetadataWorkerPool {
    sender: Option<mpsc::SyncSender<(usize, PathBuf)>>,
    results: mpsc::Receiver<(usize, Result<DirectoryResult, ()>)>,
    workers: Vec<JoinHandle<()>>,
    coordinator: RootDiscoveryCoordinator,
    budget: MetadataTraversalBudget,
}

impl MetadataWorkerPool {
    /// 创建受全进程数量上限约束的元数据探测 worker 池。
    pub(super) fn new(
        worker_count: usize,
        coordinator: &RootDiscoveryCoordinator,
        budget: MetadataTraversalBudget,
    ) -> Self {
        reap_finished_workers();
        let capacity = worker_count.saturating_mul(2).max(1);
        let (sender, receiver) = mpsc::sync_channel::<(usize, PathBuf)>(capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let (result_sender, results) = mpsc::channel();
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            if !reserve_worker_slot() {
                break;
            }
            let receiver = Arc::clone(&receiver);
            let result_sender = result_sender.clone();
            let worker_coordinator = coordinator.clone();
            let spawn_result = std::thread::Builder::new()
                .name(format!("metadata-discovery-{index}"))
                .spawn(move || {
                    let _live_slot = LiveWorkerSlot;
                    loop {
                        let job = receiver
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .recv();
                        let Ok((volume, path)) = job else {
                            break;
                        };
                        let result = std::panic::catch_unwind(|| {
                            inspect_directory_metadata(
                                &path,
                                &worker_coordinator,
                                budget,
                                DIRECTORY_ENTRY_LIMIT,
                            )
                        })
                        .map_err(|_| ());
                        if result_sender.send((volume, result)).is_err() {
                            break;
                        }
                    }
                });
            match spawn_result {
                Ok(worker) => workers.push(worker),
                Err(_) => release_worker_slot(),
            }
        }
        drop(receiver);
        drop(result_sender);
        Self {
            sender: Some(sender),
            results,
            workers,
            coordinator: coordinator.clone(),
            budget,
        }
    }

    /// 有界投递一批目录任务，并在每个结果边界复核取消与总 deadline。
    pub(super) fn inspect(&self, jobs: Vec<(usize, PathBuf)>) -> InspectionBatch {
        let Some(sender) = self.sender.as_ref() else {
            return InspectionBatch {
                stop_reason: Some(TraversalStopReason::WorkerUnavailable),
                ..InspectionBatch::default()
            };
        };
        let mut expected = 0_usize;
        for mut job in jobs {
            loop {
                if let Some(stop_reason) = self.budget.stop_reason(&self.coordinator) {
                    return InspectionBatch {
                        stop_reason: Some(stop_reason),
                        ..InspectionBatch::default()
                    };
                }
                match sender.try_send(job) {
                    Ok(()) => {
                        expected = expected.saturating_add(1);
                        break;
                    }
                    Err(mpsc::TrySendError::Full(returned_job)) => {
                        job = returned_job;
                        std::thread::sleep(self.budget.result_wait());
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        return InspectionBatch {
                            stop_reason: Some(TraversalStopReason::WorkerUnavailable),
                            ..InspectionBatch::default()
                        };
                    }
                }
            }
        }

        let mut batch = InspectionBatch {
            results: Vec::with_capacity(expected),
            stop_reason: None,
        };
        while batch.results.len() < expected {
            if let Some(stop_reason) = self.budget.stop_reason(&self.coordinator) {
                batch.stop_reason = Some(stop_reason);
                break;
            }
            match self.results.recv_timeout(self.budget.result_wait()) {
                Ok(result) => {
                    if let Some(stop_reason) = self.budget.stop_reason(&self.coordinator) {
                        batch.stop_reason = Some(stop_reason);
                        break;
                    }
                    batch.results.push(result);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    batch.stop_reason = Some(TraversalStopReason::WorkerUnavailable);
                    break;
                }
            }
        }
        batch
    }
}

impl Drop for MetadataWorkerPool {
    /// 关闭任务通道；只在短 grace 内 join，卡住的文件系统 worker 交由静态 owner 保管。
    fn drop(&mut self) {
        self.sender.take();
        let deadline = Instant::now() + WORKER_SHUTDOWN_GRACE;
        let mut pending = self.workers.drain(..).collect::<Vec<_>>();
        while !pending.is_empty() {
            join_finished_workers(&mut pending);
            if pending.is_empty() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        retain_workers(pending);
    }
}

/// 在线程退出时归还全进程 worker 名额，即使任务以 panic 结束也不泄漏计数。
struct LiveWorkerSlot;

impl Drop for LiveWorkerSlot {
    /// 归还当前 worker 在全进程数量上限中的占位。
    fn drop(&mut self) {
        release_worker_slot();
    }
}

/// 原子预留一个全进程元数据 worker 名额，避免反复取消累积卡住线程。
fn reserve_worker_slot() -> bool {
    LIVE_METADATA_WORKERS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < LIVE_METADATA_WORKER_LIMIT).then_some(current + 1)
        })
        .is_ok()
}

/// 释放一个此前成功预留的全进程元数据 worker 名额。
fn release_worker_slot() {
    let previous = LIVE_METADATA_WORKERS.fetch_sub(1, Ordering::AcqRel);
    debug_assert!(previous > 0, "metadata worker slot underflow");
}

/// 返回持有超时 worker 句柄的进程级 owner。
fn retained_workers() -> &'static Mutex<Vec<JoinHandle<()>>> {
    RETAINED_METADATA_WORKERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// join 已经结束的 worker；`is_finished` 保证这里不会等待文件系统调用。
fn join_finished_workers(workers: &mut Vec<JoinHandle<()>>) {
    let mut index = 0_usize;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            let _ = worker.join();
        } else {
            index += 1;
        }
    }
}

/// 清理静态 owner 中已经退出的 worker，为后续发现释放句柄资源。
fn reap_finished_workers() {
    let mut finished = Vec::new();
    {
        let mut retained = retained_workers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut index = 0_usize;
        while index < retained.len() {
            if retained[index].is_finished() {
                finished.push(retained.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }
    for worker in finished {
        let _ = worker.join();
    }
}

/// 把 grace 内未退出的 worker 转交静态 owner，绝不通过丢弃句柄分离线程。
fn retain_workers(workers: Vec<JoinHandle<()>>) {
    if workers.is_empty() {
        return;
    }
    retained_workers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(workers);
}

/// 只读取一个目录的目录项与链接元数据，并在每个潜在阻塞边界检查预算。
fn inspect_directory_metadata(
    path: &Path,
    coordinator: &RootDiscoveryCoordinator,
    budget: MetadataTraversalBudget,
    entry_limit: usize,
) -> DirectoryResult {
    let mut result = DirectoryResult::default();
    if let Some(stop_reason) = budget.stop_reason(coordinator) {
        result.stop_reason = Some(stop_reason);
        return result;
    }
    if is_obviously_network_path(path) {
        result.skipped = 1;
        return result;
    }
    let mut entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            result.permission_denied = 1;
            return result;
        }
        Err(_) => {
            result.io_errors = 1;
            return result;
        }
    };
    if let Some(stop_reason) = budget.stop_reason(coordinator) {
        result.stop_reason = Some(stop_reason);
        return result;
    }

    let mut entries_checked = 0_usize;
    loop {
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        let Some(entry) = entries.next() else {
            break;
        };
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        if entries_checked >= entry_limit {
            result.skipped = result.skipped.saturating_add(1);
            result.stop_reason = Some(TraversalStopReason::DirectoryEntryLimit);
            break;
        }
        entries_checked = entries_checked.saturating_add(1);
        let Ok(entry) = entry else {
            result.io_errors = result.io_errors.saturating_add(1);
            continue;
        };
        let entry_path = entry.path();
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        let metadata = fs::symlink_metadata(&entry_path);
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        let Ok(metadata) = metadata else {
            result.io_errors = result.io_errors.saturating_add(1);
            continue;
        };
        if metadata_is_link_like(&metadata) {
            result.skipped = result.skipped.saturating_add(1);
            continue;
        }
        if metadata.is_dir() {
            result.children.push(entry_path);
        } else if metadata.is_file() {
            result.file_names_checked = result.file_names_checked.saturating_add(1);
            if let Some(candidate) = candidate_from_file_name(&entry_path) {
                result.candidates.push(candidate);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::{RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStrategy};

    /// 创建运行中的测试协调器，供取消与 deadline 合同复用。
    fn running_coordinator() -> RootDiscoveryCoordinator {
        let coordinator = RootDiscoveryCoordinator::default();
        assert!(coordinator.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::FullLocalVolumes,
            1,
        ));
        coordinator
    }

    /// 目录项上限只处理允许数量，并以可观察停止原因拒绝伪称完整覆盖。
    #[test]
    fn directory_entry_limit_stops_before_extra_metadata() {
        let temp = tempfile::tempdir().expect("temporary directory");
        for name in ["a.jsonl", "b.jsonl", "c.jsonl"] {
            fs::write(temp.path().join(name), b"must not be opened").expect("fixture file");
        }
        let coordinator = running_coordinator();
        let result = inspect_directory_metadata(
            temp.path(),
            &coordinator,
            MetadataTraversalBudget::with_timeout(Duration::from_secs(1)),
            2,
        );

        assert_eq!(result.file_names_checked, 2);
        assert_eq!(result.skipped, 1);
        assert_eq!(
            result.stop_reason,
            Some(TraversalStopReason::DirectoryEntryLimit)
        );
    }

    /// 已请求取消时不得读取首个目录项或返回可继续入队、提交的结果。
    #[test]
    fn directory_inspection_observes_preexisting_cancellation() {
        let temp = tempfile::tempdir().expect("temporary directory");
        fs::write(temp.path().join("a.jsonl"), b"must not be opened").expect("fixture file");
        let coordinator = running_coordinator();
        assert!(coordinator.request_cancel());

        let result = inspect_directory_metadata(
            temp.path(),
            &coordinator,
            MetadataTraversalBudget::with_timeout(Duration::from_secs(1)),
            DIRECTORY_ENTRY_LIMIT,
        );

        assert_eq!(result.stop_reason, Some(TraversalStopReason::Cancelled));
        assert_eq!(result.file_names_checked, 0);
        assert!(result.children.is_empty());
        assert!(result.candidates.is_empty());
    }

    /// 没有结果到达时，接收循环必须由总 deadline 唤醒而不是永久阻塞。
    #[test]
    fn result_receive_is_bounded_by_total_deadline() {
        let coordinator = running_coordinator();
        let budget = MetadataTraversalBudget::with_timeout(Duration::from_millis(20));
        let (sender, results) = mpsc::channel();
        let started = Instant::now();
        let mut stop_reason = None;
        while stop_reason.is_none() {
            if let Some(reason) = budget.stop_reason(&coordinator) {
                stop_reason = Some(reason);
                break;
            }
            match results.recv_timeout(budget.result_wait()) {
                Ok(()) => unreachable!("test channel never sends"),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => unreachable!("sender is retained"),
            }
        }
        drop(sender);

        assert_eq!(stop_reason, Some(TraversalStopReason::DeadlineExceeded));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// 卡住的原生线程句柄转交静态 owner 时不得阻塞调用方或丢失所有权。
    #[test]
    fn unfinished_worker_is_retained_without_blocking_drop() {
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_finished = Arc::clone(&finished);
        let (release_sender, release_receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = release_receiver.recv();
            worker_finished.store(true, Ordering::Release);
        });
        let started = Instant::now();
        retain_workers(vec![worker]);
        assert!(started.elapsed() < Duration::from_millis(100));

        release_sender.send(()).expect("release retained worker");
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            reap_finished_workers();
            if finished.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("retained worker did not become reapable");
    }
}
