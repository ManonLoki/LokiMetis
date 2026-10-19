//! 以显式所有者运行 Windows Store COM 激活，并对调用与取消使用同一个总期限。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoAllowSetForegroundWindow, CoCancelCall,
    CoCreateInstance, CoDisableCallCancellation, CoEnableCallCancellation, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{
    AO_NONE, ApplicationActivationManager, IApplicationActivationManager,
};
use windows::core::{PCWSTR, w};

use super::super::CODEX_PAGE_POLL_INTERVAL;
use super::AppError;

const STORE_ACTIVATION_TOTAL_TIMEOUT: Duration = Duration::from_secs(15);
const STORE_ACTIVATION_FINAL_CANCEL_BUDGET: Duration = Duration::from_secs(2);
const ACTIVATION_PHASE_COM_CALL: u8 = 1;
const ACTIVATION_PHASE_OUTCOME_READY: u8 = 2;

/// Store 激活调用已经返回时的可观察结果。
pub(super) type StoreActivationResult = Result<(), StoreActivationFailure>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 不向 IPC 暴露 HRESULT 的 Store 激活失败阶段。
pub(super) enum StoreActivationFailure {
    /// 调用方已取消。
    Cancelled,
    /// COM 初始化失败。
    ComInitialization,
    /// 无法建立 COM 取消。
    CancellationSetup,
    /// 无法创建应用激活管理器。
    ManagerCreation,
    /// 激活调用失败。
    Activation,
    /// 激活线程 panic。
    WorkerPanicked,
}

impl StoreActivationFailure {
    /// 返回可用于脱敏诊断的稳定阶段名。
    pub(super) const fn reason(self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::ComInitialization => "com-initialization",
            Self::CancellationSetup => "cancellation-setup",
            Self::ManagerCreation => "manager-creation",
            Self::Activation => "activation",
            Self::WorkerPanicked => "worker-panicked",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// owner 无法开始或在总期限内回收一次激活的原因。
enum StoreActivationLifecycleFailure {
    /// 已有激活在进行。
    Busy,
    /// 无法启动激活线程。
    ThreadStart,
    /// 总期限内没有回收；附是否已请求 COM 取消。
    Timeout { cancellation_requested: bool },
}

#[derive(Default)]
/// 一次原生线程激活的协作取消与结果槽。
struct StoreActivationControl {
    /// 协作取消标志。
    cancelled: AtomicBool,
    /// 激活阶段，只有处于 COM 调用阶段时才发出 COM 取消。
    phase: AtomicU8,
    /// 激活线程的系统线程 ID，用于 COM 取消。
    thread_id: AtomicU32,
    /// 激活结果槽。
    outcome: Mutex<Option<StoreActivationResult>>,
}

impl StoreActivationControl {
    /// 发布取消并在 COM 调用已经开始时请求系统取消该同步调用。
    fn request_cancel(&self) -> bool {
        self.cancelled.store(true, Ordering::Release);
        if self.phase.load(Ordering::Acquire) != ACTIVATION_PHASE_COM_CALL {
            return false;
        }
        let thread_id = self.thread_id.load(Ordering::Acquire);
        if thread_id == 0 {
            return false;
        }
        // SAFETY: thread_id 由本次仍受 owner 持有的激活线程发布；零秒参数不会在此额外等待。
        unsafe { CoCancelCall(thread_id, 0) }.is_ok()
    }

    /// 返回 owner 或调用方是否已经请求停止本次 Store 激活。
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// 在取消已发布时优先返回取消，否则保留底层激活失败。
    fn failure_or_cancelled(&self, failure: StoreActivationFailure) -> StoreActivationFailure {
        if self.is_cancelled() {
            StoreActivationFailure::Cancelled
        } else {
            failure
        }
    }

    /// 在线程发布完成 phase 前保存唯一的终态结果。
    fn store_outcome(&self, outcome: StoreActivationResult) {
        *self
            .outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
    }

    /// 仅在线程发布结果 phase 后取走一次终态结果；该 phase 不代表原生线程已退出。
    fn take_outcome(&self) -> Option<StoreActivationResult> {
        if self.phase.load(Ordering::Acquire) != ACTIVATION_PHASE_OUTCOME_READY {
            return None;
        }
        self.outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// owner 持有仍可能位于 COM 内的原生线程句柄。
struct OwnedStoreActivation {
    /// 激活 ID。
    id: u64,
    /// 原生线程句柄。
    handle: JoinHandle<()>,
}

#[derive(Default)]
/// 保存当前唯一受管 Store 激活线程，确保句柄不会脱离 owner。
struct StoreActivationOwnerState {
    /// 当前唯一受管激活线程。
    task: Option<OwnedStoreActivation>,
}

#[derive(Default)]
/// 每个进程唯一的 Store 激活 owner；同一时刻只允许一个有副作用的 COM 调用。
struct StoreActivationOwner {
    /// 下一个激活 ID。
    next_id: AtomicU64,
    /// 当前受管激活。
    state: Mutex<StoreActivationOwnerState>,
}

impl StoreActivationOwner {
    /// 只在目标线程已经终止后从 owner 取出并 join，绝不等待仍在运行的线程。
    fn reap_finished_locked(
        state: &mut StoreActivationOwnerState,
        expected_id: Option<u64>,
    ) -> bool {
        let Some(task) = state.task.as_ref() else {
            return false;
        };
        if expected_id.is_some_and(|expected_id| task.id != expected_id)
            || !task.handle.is_finished()
        {
            return false;
        }

        let task = state
            .task
            .take()
            .expect("finished Store activation remains registered");
        if task.handle.join().is_err() {
            tracing::warn!(
                activation_id = task.id,
                "Store activation thread panicked outside its guarded operation"
            );
        }
        true
    }

    /// 从外部调用线程回收一个已经终止的指定工作线程。
    fn reap_finished(&self, expected_id: u64) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::reap_finished_locked(&mut state, Some(expected_id))
    }

    /// 在登记句柄后才放行线程，消除“线程先结束、句柄后登记”的竞态。
    fn start<Operation>(
        self: &Arc<Self>,
        operation: Operation,
    ) -> Result<StoreActivationLease, StoreActivationLifecycleFailure>
    where
        Operation: FnOnce(Arc<StoreActivationControl>) -> StoreActivationResult + Send + 'static,
    {
        self.start_inner(operation, || {})
    }

    #[cfg(test)]
    /// 为回归测试在结果发布与原生线程退出之间插入可控尾段。
    fn start_with_worker_tail<Operation, WorkerTail>(
        self: &Arc<Self>,
        operation: Operation,
        worker_tail: WorkerTail,
    ) -> Result<StoreActivationLease, StoreActivationLifecycleFailure>
    where
        Operation: FnOnce(Arc<StoreActivationControl>) -> StoreActivationResult + Send + 'static,
        WorkerTail: FnOnce() + Send + 'static,
    {
        self.start_inner(operation, worker_tail)
    }

    /// 创建受管工作线程，并在结果发布后运行仅由调用方提供的有界尾段。
    fn start_inner<Operation, WorkerTail>(
        self: &Arc<Self>,
        operation: Operation,
        worker_tail: WorkerTail,
    ) -> Result<StoreActivationLease, StoreActivationLifecycleFailure>
    where
        Operation: FnOnce(Arc<StoreActivationControl>) -> StoreActivationResult + Send + 'static,
        WorkerTail: FnOnce() + Send + 'static,
    {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::reap_finished_locked(&mut state, None);
        if state.task.is_some() {
            return Err(StoreActivationLifecycleFailure::Busy);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let control = Arc::new(StoreActivationControl::default());
        let worker_control = Arc::clone(&control);
        let registration = Arc::new(AtomicBool::new(false));
        let worker_registration = Arc::clone(&registration);
        let handle = thread::Builder::new()
            .name("loki-store-activation".to_owned())
            .spawn(move || {
                while !worker_registration.load(Ordering::Acquire) {
                    thread::yield_now();
                }
                let outcome =
                    catch_unwind(AssertUnwindSafe(|| operation(Arc::clone(&worker_control))))
                        .unwrap_or(Err(StoreActivationFailure::WorkerPanicked));
                worker_control.store_outcome(outcome);
                worker_control
                    .phase
                    .store(ACTIVATION_PHASE_OUTCOME_READY, Ordering::Release);
                worker_tail();
            })
            .map_err(|_| StoreActivationLifecycleFailure::ThreadStart)?;
        state.task = Some(OwnedStoreActivation { id, handle });
        registration.store(true, Ordering::Release);
        drop(state);
        Ok(StoreActivationLease {
            owner: Arc::clone(self),
            id,
            control,
            completed: false,
        })
    }

    #[cfg(test)]
    /// 返回测试 owner 当前持有的工作线程数量。
    fn owned_task_count(&self) -> usize {
        usize::from(
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .task
                .is_some(),
        )
    }
}

/// 调用方 lease 在 future 被取消或 runtime 关闭而销毁时仍会请求 COM 取消。
struct StoreActivationLease {
    /// 进程级激活 owner。
    owner: Arc<StoreActivationOwner>,
    /// 激活 ID。
    id: u64,
    /// 取消与结果控制。
    control: Arc<StoreActivationControl>,
    /// 已取得结果；为 false 时析构会请求取消。
    completed: bool,
}

impl StoreActivationLease {
    /// 在总期限内等待激活终态，并在末段预算中持续请求 COM 取消。
    async fn wait(
        mut self,
        total_timeout: Duration,
        final_cancel_budget: Duration,
    ) -> Result<StoreActivationResult, StoreActivationLifecycleFailure> {
        let started_at = tokio::time::Instant::now();
        let final_deadline = started_at + total_timeout;
        let cancel_deadline = final_deadline
            .checked_sub(final_cancel_budget.min(total_timeout))
            .unwrap_or(started_at);

        loop {
            if let Some(outcome) = self.control.take_outcome() {
                self.completed = true;
                self.owner.reap_finished(self.id);
                return Ok(outcome);
            }
            if tokio::time::Instant::now() >= cancel_deadline {
                break;
            }
            tokio::time::sleep(
                CODEX_PAGE_POLL_INTERVAL
                    .min(cancel_deadline.saturating_duration_since(tokio::time::Instant::now())),
            )
            .await;
        }

        let mut cancellation_requested = self.control.request_cancel();
        loop {
            if let Some(outcome) = self.control.take_outcome() {
                self.completed = true;
                self.owner.reap_finished(self.id);
                return Ok(outcome);
            }
            if tokio::time::Instant::now() >= final_deadline {
                return Err(StoreActivationLifecycleFailure::Timeout {
                    cancellation_requested,
                });
            }
            tokio::time::sleep(
                CODEX_PAGE_POLL_INTERVAL
                    .min(final_deadline.saturating_duration_since(tokio::time::Instant::now())),
            )
            .await;
            // 覆盖 phase 切换与 COM cancel object 建立之间的短竞态。
            cancellation_requested |= self.control.request_cancel();
        }
    }
}

impl Drop for StoreActivationLease {
    /// 调用方提前离开时请求取消，同时让全局 owner 继续持有工作线程。
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.control.request_cancel();
            tracing::warn!(
                activation_id = self.id,
                owned = Arc::strong_count(&self.owner) > 1,
                "Store activation caller ended before the owned worker reached a terminal state"
            );
        }
    }
}

static STORE_ACTIVATION_OWNER: OnceLock<Arc<StoreActivationOwner>> = OnceLock::new();

/// 运行一次有总期限、有取消路径且不会随 Tokio blocking pool 丢失所有权的 Store 激活。
pub(super) async fn activate_store_codex_owned(
    arguments: String,
) -> Result<StoreActivationResult, AppError> {
    let owner = Arc::clone(
        STORE_ACTIVATION_OWNER.get_or_init(|| Arc::new(StoreActivationOwner::default())),
    );
    let lease = owner
        .start(move |control| activate_store_codex_with_arguments(&arguments, &control))
        .map_err(store_activation_lifecycle_error)?;
    let outcome = lease
        .wait(
            STORE_ACTIVATION_TOTAL_TIMEOUT,
            STORE_ACTIVATION_FINAL_CANCEL_BUDGET,
        )
        .await
        .map_err(store_activation_lifecycle_error)?;
    if outcome == Err(StoreActivationFailure::Cancelled) {
        return Err(AppError::new(
            "skin.codex_launch_timeout",
            "Codex Store 激活已被取消，未启动其它替代实例。",
        ));
    }
    Ok(outcome)
}

/// 把 Store 激活 owner 的生命周期失败映射为稳定、脱敏的应用错误。
fn store_activation_lifecycle_error(error: StoreActivationLifecycleFailure) -> AppError {
    match error {
        StoreActivationLifecycleFailure::Busy => AppError::new(
            "skin.codex_launch_busy",
            "上一次 Codex Store 激活仍在收尾，请稍后重试。",
        ),
        StoreActivationLifecycleFailure::ThreadStart => AppError::new(
            "skin.codex_launch_failed",
            "无法启动受控的 Codex Store 激活任务。",
        ),
        StoreActivationLifecycleFailure::Timeout {
            cancellation_requested,
        } => AppError::with_details(
            "skin.codex_launch_timeout",
            "Codex Store 激活未在限定时间内结束；已请求取消，未启动其它替代实例。",
            vec![format!(
                "com_cancel_request={}",
                if cancellation_requested {
                    "accepted"
                } else {
                    "unavailable"
                }
            )],
        ),
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - store_activation_call.rs：带参数的商店版 Codex 激活调用，以及本模块单元测试
include!("store_activation_call.rs");
