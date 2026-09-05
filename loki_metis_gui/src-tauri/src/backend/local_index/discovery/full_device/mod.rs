//! 用户主动发起的全设备发现：从显式卷起点递归遍历，以确定性批次并行目录 I/O，
//! 同时由单一协调器维护预算、签名、进度和取消语义。
//!
//! 整体架构是“单协调器 + 多 worker 线程池”，但刻意做成确定性、可复现的：
//! 协调器每一轮固定派发 `DISCOVERY_ROUND_SIZE`（8）个带编号（`order`）的
//! 任务给最多 `MAX_DISCOVERY_WORKERS`（2）个 worker 并行执行目录 I/O
//! （真正耗时的部分——打开目录、读取目录项、探测文件签名），
//! worker 算完后把结果连同编号一起通过 channel 送回；协调器收集完
//! 一整轮的结果后，**始终按 order 编号顺序**提交到共享状态（预算扣减、
//! 已发现根列表等），而不是按“谁先算完”的到达顺序提交。
//! 这样即使 worker 之间执行速度不同（磁盘 I/O 延迟天然不稳定），
//! 最终得到的“发现了哪些根、按什么顺序”依然和单线程顺序执行完全一致，
//! 使得测试可以断言确定性结果，也让并行只提升速度、不改变行为。

mod coordinator;
mod types;
mod worker;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};

use super::{DiscoveryProgress, DiscoveryResult, FullDiscoveryOptions};
use crate::backend::local_index::CancellationToken;
use coordinator::DiscoveryCoordinator;
use types::{WorkerOutcome, WorkerResult, WorkerTask};
use worker::execute_worker_task;

/// 生产扫描最多使用两个原生目录 I/O worker，避免抢占 Token 索引 CPU 时间。
pub(super) const MAX_DISCOVERY_WORKERS: usize =
    loki_metis_core::LOCAL_DISCOVERY_WORKER_LIMIT;
/// 每轮固定派发八个确定性 ticket，使单 worker 与多 worker 使用同一提交顺序。
pub(super) const DISCOVERY_ROUND_SIZE: usize = 8;
/// 单个目录枚举 ticket 最多读取 256 项，限制乱序结果与取消等待的内存边界。
pub(super) const DIRECTORY_ENTRY_CHUNK: u64 = 256;

/// 递归执行用户主动发起的发现，逐目录检查取消且绝不跟随符号链接。
#[cfg(test)]
pub(super) fn discover_full_device(
    options: &FullDiscoveryOptions,
    cancellation: &CancellationToken,
) -> DiscoveryResult {
    discover_full_device_with_workers(options, cancellation, 1, |_| {}, &|_| {})
}

/// 递归执行主动发现，并按有界频率发布不含路径的目录与根计数。
pub fn discover_full_device_with_progress<F>(
    options: &FullDiscoveryOptions,
    cancellation: &CancellationToken,
    on_progress: F,
) -> DiscoveryResult
where
    F: FnMut(DiscoveryProgress),
{
    discover_full_device_with_workers(
        options,
        cancellation,
        production_worker_count(),
        on_progress,
        &|_| {},
    )
}

/// 使用测试可注入 worker 数和只读观察钩子运行同一协调器；生产调用传入空钩子。
pub(super) fn discover_full_device_with_workers<F, H>(
    options: &FullDiscoveryOptions,
    cancellation: &CancellationToken,
    requested_workers: usize,
    on_progress: F,
    worker_hook: &H,
) -> DiscoveryResult
where
    F: FnMut(DiscoveryProgress),
    H: Fn(&Path) + Sync,
{
    let worker_count = requested_workers.clamp(1, MAX_DISCOVERY_WORKERS);
    let mut coordinator = DiscoveryCoordinator::new(options, cancellation, on_progress);
    std::thread::scope(|scope| {
        let (task_sender, task_receiver) = mpsc::sync_channel::<WorkerTask>(DISCOVERY_ROUND_SIZE);
        let task_receiver = Arc::new(Mutex::new(task_receiver));
        let (result_sender, result_receiver) = mpsc::channel::<WorkerResult>();
        for _ in 0..worker_count {
            let task_receiver = Arc::clone(&task_receiver);
            let result_sender = result_sender.clone();
            scope.spawn(move || {
                loop {
                    let task = {
                        let receiver = task_receiver
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        receiver.recv()
                    };
                    let Ok(task) = task else {
                        break;
                    };
                    let order = task.order;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        worker_hook(task.pending.path());
                        execute_worker_task(task, cancellation)
                    }))
                    .unwrap_or(WorkerResult {
                        order,
                        outcome: WorkerOutcome::Failed,
                    });
                    if result_sender.send(result).is_err() {
                        break;
                    }
                }
            });
        }
        drop(result_sender);

        loop {
            if coordinator.user_cancelled || coordinator.budget_exhausted {
                break;
            }
            let tasks = coordinator.prepare_round();
            if tasks.is_empty() {
                break;
            }
            let task_count = tasks.len();
            for task in tasks {
                if task_sender.send(task).is_err() {
                    coordinator.budget_exhausted = true;
                    coordinator.skipped_count = coordinator.skipped_count.saturating_add(1);
                    break;
                }
            }
            let mut results = Vec::with_capacity(task_count);
            for _ in 0..task_count {
                match result_receiver.recv() {
                    Ok(result) => results.push(result),
                    Err(_) => {
                        coordinator.budget_exhausted = true;
                        coordinator.skipped_count = coordinator.skipped_count.saturating_add(1);
                        break;
                    }
                }
            }
            coordinator.commit_round(results);
        }
        drop(task_sender);
    });
    coordinator.finish()
}

/// 返回生产环境 worker 数，不超过宿主可用并行度和固定四线程上限。
fn production_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
        .clamp(1, MAX_DISCOVERY_WORKERS)
}
