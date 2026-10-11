//! 本机 Hook 配置的受管多文件原子写入与生命周期补写 worker。
//!
//! 配置目录定位与校验位于 `locations` 子模块；本模块只负责串行化写入、显式写入
//! 队列与取消、后台自愈 worker 以及退出时的有界回收。

mod locations;

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

use loki_metris_core::{
    AiTool, HookConfigPreview, HookConfigWriteResult, HookError, HookRegistrationMode, ai_tool_name,
    generate_hook_auxiliary_configs, generate_hook_config, generate_wsl_hook_config,
    hook_config_filename, hook_config_write_result, hook_supports_wsl, normalize_enabled_ai_tools,
};

pub use locations::{list_hook_config_locations, validate_hook_config_directory};

use super::{settings::MonitorSettings, thread_owner::RetainedThreadOwner, wsl::WslDirectory};

use self::locations::{hook_config_path_in, hook_config_target_directories, location_for};

/// 串行化显式写入与后台自动补写，避免同一工具配置被并发读改写覆盖。
static HOOK_CONFIG_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
/// 超过同步退出预算的 Hook writer 仍由后台 owner 持有并在空闲期 join。
static RETAINED_HOOK_WRITERS: OnceLock<RetainedThreadOwner> = OnceLock::new();
/// 外部 Hook writer 停止后，后台自愈在此期限内重新收敛受管条目。
const AUTOMATIC_REPAIR_INTERVAL: Duration = Duration::from_secs(5);
/// 退出回调等待 Hook writer 主动收敛的上限；不可中断的本机文件系统调用最终由进程退出回收。
const HOOK_WRITER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
/// 等待具名线程退出时的短轮询间隔。
const HOOK_WRITER_SHUTDOWN_POLL: Duration = Duration::from_millis(10);
/// 显式写入等待 owner 完成的上限；覆盖一个在途 WSL 写入和本次 WSL 总时限。
const EXPLICIT_HOOK_WRITE_TIMEOUT: Duration = Duration::from_secs(35);
/// 显式写入首次超时发布取消后，IPC 最多再等待这一固定宽限。
const EXPLICIT_HOOK_CANCEL_GRACE: Duration = Duration::from_secs(1);
/// 显式写入队列的硬上限，防止卡住的存储层导致请求无界累积。
const MAX_EXPLICIT_HOOK_WRITE_QUEUE: usize = 32;
/// 队列回收与 worker 取消边界共用的稳定错误详情。
const HOOK_WRITE_CANCELLED_DETAIL: &str = "hook config write was cancelled before completion";
/// worker 未在最终截止时间内确认取消时返回的稳定错误详情。
const HOOK_WRITE_CANCEL_DEADLINE_DETAIL: &str =
    "hook config write cancellation was not acknowledged before the deadline";
/// 有界队列已满时返回的稳定背压错误详情。
const HOOK_WRITE_QUEUE_FULL_DETAIL: &str = "hook config writer request queue is full";

/// 一次由 UI 明确请求、但由生命周期 worker 实际执行的 Hook 配置写入。
struct ExplicitHookWriteRequest {
    /// 监控设置目录只作为受信路径解析结果传入，读取发生在 worker 线程。
    config_directory: PathBuf,
    /// 用户选择写入的公开工具。
    tool: AiTool,
    /// IPC 取消、超时或应用退出时由 owner 观察的单请求令牌。
    cancellation: Arc<AtomicBool>,
    /// 把 worker 结果送回异步 GPUI command；接收端消失不改变 worker 所有权。
    response: oneshot::Sender<Result<HookConfigWriteResult, HookError>>,
}

/// Hook writer 每轮只取一项工作，显式用户请求优先于周期自愈。
enum HookWriterWork {
    /// 必须返回结构化结果的显式写入。
    Explicit(ExplicitHookWriteRequest),
    /// 无需阻塞 UI 的最新设置自愈。
    AutomaticRepair(MonitorSettings),
}

/// 异步等待被取消时同步标记对应请求，避免 worker 变成 detached 写入。
struct HookWriteCancellationGuard {
    /// 与排队请求共享的取消标志。
    cancellation: Arc<AtomicBool>,
    /// worker 已返回结果后解除，避免把已完成请求误标为取消。
    armed: bool,
}

impl HookWriteCancellationGuard {
    /// 为已成功排队的请求建立取消所有权。
    fn new(cancellation: Arc<AtomicBool>) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    /// worker 已完成并交付结果后解除取消动作。
    fn disarm(&mut self) {
        self.armed = false;
    }

    /// 到达首次时限时发布取消，守卫保留到结果或最终截止。
    fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }
}

impl Drop for HookWriteCancellationGuard {
    /// future 被取消或超时时通知仍由生命周期 owner 持有的阻塞工作。
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.store(true, Ordering::Release);
        }
    }
}

/// Hook writer 的请求状态；自动设置合并，显式写入保持逐次结果。
#[derive(Default)]
struct HookWriterState {
    /// 最新设置快照；保留后供低频内容感知自愈重复检查。
    settings: Option<MonitorSettings>,
    /// 保存或启动请求要求立即检查，而不是等待下一个周期。
    repair_requested: bool,
    /// 等待单一具名 worker 执行的显式写入，禁止为每个 IPC 创建 detached 线程。
    explicit_requests: VecDeque<ExplicitHookWriteRequest>,
    /// worker 当前正在执行的显式请求，供应用退出同步触发取消。
    active_explicit_cancellation: Option<Arc<AtomicBool>>,
    /// 应用退出后阻止接受和执行更多写入。
    shutting_down: bool,
}

/// 由 GPUI 应用生命周期拥有的 Hook 显式写入与自动补写 worker。
pub struct HookConfigWriter {
    /// worker 与调用方共享的请求状态和唤醒信号。
    shared: Option<Arc<(Mutex<HookWriterState>, Condvar)>>,
    /// 应用退出时必须回收的具名线程。
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    /// 退出时同步打断 worker 正在等待的受管 WSL 子进程。
    cancellation: Option<Arc<AtomicBool>>,
}

impl HookConfigWriter {
    /// 用确定的 relay 与主目录启动 worker，生产构造与隔离测试复用同一生命周期。
    pub(crate) fn start(relay_executable: PathBuf, home_directory: PathBuf) -> Self {
        Self::start_with_interval(relay_executable, home_directory, AUTOMATIC_REPAIR_INTERVAL)
    }

    /// 用可注入周期启动 worker，隔离测试无需等待生产自愈间隔。
    fn start_with_interval(
        relay_executable: PathBuf,
        home_directory: PathBuf,
        repair_interval: Duration,
    ) -> Self {
        let shared = Arc::new((Mutex::new(HookWriterState::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation);
        let worker = match thread::Builder::new()
            .name("loki-metris-hook-writer".to_owned())
            .spawn(move || {
                hook_config_writer_loop(
                    &worker_shared,
                    &relay_executable,
                    &home_directory,
                    repair_interval,
                    &worker_cancellation,
                )
            }) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(error = %error, "failed to start hook config writer");
                return Self::disabled();
            }
        };
        Self {
            shared: Some(shared),
            worker: Mutex::new(Some(worker)),
            cancellation: Some(cancellation),
        }
    }

    /// 合并排队当前已启用工具的最新设置；不会因磁盘或 WSL I/O 阻塞 UI。
    pub fn request_enabled(&self, settings: MonitorSettings) {
        let Some(shared) = &self.shared else {
            return;
        };
        let (state, wake) = &**shared;
        let Ok(mut state) = state.lock() else {
            tracing::warn!("failed to lock automatic hook repair state");
            return;
        };
        if !state.shutting_down {
            state.settings = Some(settings);
            state.repair_requested = true;
            wake.notify_one();
        }
    }

    /// 将显式 Hook 写入排入生命周期 worker，并异步等待有界结果。
    pub async fn write_config(
        &self,
        config_directory: PathBuf,
        tool: AiTool,
    ) -> Result<HookConfigWriteResult, HookError> {
        self.write_config_with_deadlines(
            config_directory,
            tool,
            EXPLICIT_HOOK_WRITE_TIMEOUT,
            EXPLICIT_HOOK_CANCEL_GRACE,
        )
        .await
    }

    /// 使用可注入的首次与取消期限排队，供硬截止回归复用。
    async fn write_config_with_deadlines(
        &self,
        config_directory: PathBuf,
        tool: AiTool,
        timeout: Duration,
        cancellation_grace: Duration,
    ) -> Result<HookConfigWriteResult, HookError> {
        let response_deadline = tokio::time::Instant::now() + timeout;
        let cancellation_deadline = response_deadline + cancellation_grace;
        let Some(shared) = &self.shared else {
            return Err(hook_write_lifecycle_error(
                "hook config writer is unavailable",
            ));
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let (response, mut receiver) = oneshot::channel();
        {
            let (state, wake) = &**shared;
            let mut state = state
                .lock()
                .map_err(|_| hook_write_lifecycle_error("hook config writer state was poisoned"))?;
            if state.shutting_down {
                return Err(hook_write_lifecycle_error(
                    "hook config writer is shutting down",
                ));
            }
            enqueue_explicit_hook_write(
                &mut state,
                ExplicitHookWriteRequest {
                    config_directory,
                    tool,
                    cancellation: Arc::clone(&cancellation),
                    response,
                },
            )?;
            wake.notify_one();
        }

        await_explicit_hook_write_response(
            &mut receiver,
            cancellation,
            response_deadline,
            cancellation_deadline,
        )
        .await
    }

    /// 停止并有界回收 worker；重复调用保持幂等。
    pub fn shutdown(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.store(true, Ordering::Release);
        }
        if let Some(shared) = &self.shared {
            let (state, wake) = &**shared;
            let mut state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            begin_hook_writer_shutdown(&mut state);
            wake.notify_one();
        }
        let worker_handle = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(worker_handle) = worker_handle {
            if let Err(worker_handle) =
                join_worker_until(worker_handle, HOOK_WRITER_SHUTDOWN_TIMEOUT)
            {
                retained_hook_writer_owner().retain(worker_handle);
                tracing::warn!(
                    timeout_ms = HOOK_WRITER_SHUTDOWN_TIMEOUT.as_millis(),
                    "hook config writer exceeded shutdown deadline and was transferred to the retained owner"
                );
            }
        }
        retained_hook_writer_owner().reap_finished();
        #[cfg(target_os = "windows")]
        super::wsl::shutdown_wsl_owners(HOOK_WRITER_SHUTDOWN_TIMEOUT);
    }

    /// 构造没有线程的生命周期占位，保证 GPUI 命令仍可安全取 State。
    pub(crate) fn disabled() -> Self {
        Self {
            shared: None,
            worker: Mutex::new(None),
            cancellation: None,
        }
    }
}

/// 在同一组绝对截止时间内等待显式写入；最终超时不放弃取消令牌。
async fn await_explicit_hook_write_response(
    receiver: &mut oneshot::Receiver<Result<HookConfigWriteResult, HookError>>,
    cancellation: Arc<AtomicBool>,
    response_deadline: tokio::time::Instant,
    cancellation_deadline: tokio::time::Instant,
) -> Result<HookConfigWriteResult, HookError> {
    let mut cancellation_guard = HookWriteCancellationGuard::new(cancellation);
    match tokio::time::timeout_at(response_deadline, &mut *receiver).await {
        Ok(Ok(result)) => {
            cancellation_guard.disarm();
            result
        }
        Ok(Err(_)) => Err(hook_write_lifecycle_error(
            "hook config writer stopped before returning a result",
        )),
        Err(_) => {
            cancellation_guard.cancel();
            match tokio::time::timeout_at(cancellation_deadline, receiver).await {
                Ok(Ok(result)) => {
                    cancellation_guard.disarm();
                    result
                }
                Ok(Err(_)) => Err(hook_write_lifecycle_error(
                    "hook config writer stopped while cancelling an overdue write",
                )),
                Err(_) => Err(hook_write_lifecycle_error(
                    HOOK_WRITE_CANCEL_DEADLINE_DETAIL,
                )),
            }
        }
    }
}

impl Drop for HookConfigWriter {
    /// 即使应用未显式调用 shutdown，也发布取消并把超时线程移交长期 owner。
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 返回跨 Hook writer 实例共享的超时线程 owner。
fn retained_hook_writer_owner() -> &'static RetainedThreadOwner {
    RETAINED_HOOK_WRITERS.get_or_init(|| RetainedThreadOwner::new("hook-writer"))
}

/// 只在具名 worker 已完成时 join；超时返回原句柄，保持真实所有权。
fn join_worker_until(
    worker: thread::JoinHandle<()>,
    timeout: Duration,
) -> Result<(), thread::JoinHandle<()>> {
    let started = Instant::now();
    while !worker.is_finished() {
        if started.elapsed() >= timeout {
            return Err(worker);
        }
        thread::sleep(HOOK_WRITER_SHUTDOWN_POLL.min(timeout.saturating_sub(started.elapsed())));
    }
    let _ = worker.join();
    Ok(())
}

/// 构造不会暴露配置路径或内容的 Hook writer 生命周期错误。
fn hook_write_lifecycle_error(detail: &'static str) -> HookError {
    HookError::new("error.hooks.writeFailed").param("detail", detail)
}

/// 回收已取消请求后在硬容量内入队，满队列立即向调用方施加背压。
fn enqueue_explicit_hook_write(
    state: &mut HookWriterState,
    request: ExplicitHookWriteRequest,
) -> Result<(), HookError> {
    let mut retained = VecDeque::with_capacity(state.explicit_requests.len());
    while let Some(queued) = state.explicit_requests.pop_front() {
        if queued.cancellation.load(Ordering::Acquire) {
            let _ = queued
                .response
                .send(Err(hook_write_lifecycle_error(HOOK_WRITE_CANCELLED_DETAIL)));
        } else {
            retained.push_back(queued);
        }
    }
    state.explicit_requests = retained;
    if state.explicit_requests.len() >= MAX_EXPLICIT_HOOK_WRITE_QUEUE {
        return Err(hook_write_lifecycle_error(HOOK_WRITE_QUEUE_FULL_DETAIL));
    }
    state.explicit_requests.push_back(request);
    Ok(())
}

/// 关闭 writer 状态并取消当前及排队显式写入，不把接收端留到 owner 销毁才唤醒。
fn begin_hook_writer_shutdown(state: &mut HookWriterState) {
    state.settings = None;
    state.repair_requested = false;
    state.shutting_down = true;
    if let Some(cancellation) = &state.active_explicit_cancellation {
        cancellation.store(true, Ordering::Release);
    }
    for request in state.explicit_requests.drain(..) {
        request.cancellation.store(true, Ordering::Release);
        let _ = request.response.send(Err(hook_write_lifecycle_error(
            "hook config writer stopped before executing the request",
        )));
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - store_config_writer.rs：消费自动补写请求，直到应用生命周期明确关闭。；在具名 worker 中读取设置并完成显式写入，不占用 GPUI 异步执行线程。；串行化一次显式或后台写入；后台路径额外携带应用退出取消令牌。
include!("store_config_writer.rs");
