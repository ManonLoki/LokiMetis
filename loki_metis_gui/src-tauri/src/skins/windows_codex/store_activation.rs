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
const ACTIVATION_PHASE_FINISHED: u8 = 2;

/// Store 激活调用已经返回时的可观察结果。
pub(super) type StoreActivationResult = Result<(), StoreActivationFailure>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 不向 IPC 暴露 HRESULT 的 Store 激活失败阶段。
pub(super) enum StoreActivationFailure {
    Cancelled,
    ComInitialization,
    CancellationSetup,
    ManagerCreation,
    Activation,
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
    Busy,
    ThreadStart,
    Timeout { cancellation_requested: bool },
}

#[derive(Default)]
/// 一次原生线程激活的协作取消与结果槽。
struct StoreActivationControl {
    cancelled: AtomicBool,
    phase: AtomicU8,
    thread_id: AtomicU32,
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

    /// 仅在线程已到达完成 phase 后取走一次终态结果。
    fn take_outcome(&self) -> Option<StoreActivationResult> {
        if self.phase.load(Ordering::Acquire) != ACTIVATION_PHASE_FINISHED {
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
    id: u64,
    _handle: JoinHandle<()>,
}

#[derive(Default)]
/// 保存当前唯一受管 Store 激活线程，确保句柄不会脱离 owner。
struct StoreActivationOwnerState {
    task: Option<OwnedStoreActivation>,
}

#[derive(Default)]
/// 每个进程唯一的 Store 激活 owner；同一时刻只允许一个有副作用的 COM 调用。
struct StoreActivationOwner {
    next_id: AtomicU64,
    state: Mutex<StoreActivationOwnerState>,
}

impl StoreActivationOwner {
    /// 在登记句柄后才放行线程，消除“线程先结束、句柄后登记”的竞态。
    fn start<Operation>(
        self: &Arc<Self>,
        operation: Operation,
    ) -> Result<StoreActivationLease, StoreActivationLifecycleFailure>
    where
        Operation: FnOnce(Arc<StoreActivationControl>) -> StoreActivationResult + Send + 'static,
    {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.task.is_some() {
            return Err(StoreActivationLifecycleFailure::Busy);
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let control = Arc::new(StoreActivationControl::default());
        let worker_control = Arc::clone(&control);
        let worker_owner = Arc::clone(self);
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
                worker_owner.finish_from_worker(id);
                worker_control
                    .phase
                    .store(ACTIVATION_PHASE_FINISHED, Ordering::Release);
            })
            .map_err(|_| StoreActivationLifecycleFailure::ThreadStart)?;
        state.task = Some(OwnedStoreActivation {
            id,
            _handle: handle,
        });
        registration.store(true, Ordering::Release);
        drop(state);
        Ok(StoreActivationLease {
            owner: Arc::clone(self),
            id,
            control,
            completed: false,
        })
    }

    /// 由工作线程在最后一步移除并关闭自身 JoinHandle；阻塞期间句柄始终归 owner 所有。
    fn finish_from_worker(&self, id: u64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.task.as_ref().is_some_and(|task| task.id == id) {
            state.task.take();
        }
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
    owner: Arc<StoreActivationOwner>,
    id: u64,
    control: Arc<StoreActivationControl>,
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

/// 在独立 STA 线程执行 COM 激活；所有成功初始化步骤都在同一线程对称清理。
fn activate_store_codex_with_arguments(
    arguments: &str,
    control: &StoreActivationControl,
) -> StoreActivationResult {
    // SAFETY: 当前函数只在新建专用线程调用，并在成功初始化后于同线程 CoUninitialize。
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|_| StoreActivationFailure::ComInitialization)?;

    let result = (|| {
        // SAFETY: None 对应 Win32 要求的保留空参数，并在本函数末尾对称禁用。
        unsafe { CoEnableCallCancellation(None) }
            .map_err(|_| StoreActivationFailure::CancellationSetup)?;
        control
            .thread_id
            .store(unsafe { GetCurrentThreadId() }, Ordering::Release);
        control
            .phase
            .store(ACTIVATION_PHASE_COM_CALL, Ordering::Release);
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }

        // SAFETY: COM 已在当前专用 STA 线程初始化，接口只在该线程内使用。
        let manager: IApplicationActivationManager = unsafe {
            CoCreateInstance(
                &ApplicationActivationManager,
                None::<&windows::core::IUnknown>,
                CLSCTX_LOCAL_SERVER,
            )
        }
        .map_err(|_| control.failure_or_cancelled(StoreActivationFailure::ManagerCreation))?;
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }
        // SAFETY: manager 是当前线程拥有的有效 COM 接口。
        if unsafe { CoAllowSetForegroundWindow(&manager, None) }.is_err() {
            tracing::warn!("Windows declined foreground permission for Store activation");
        }
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }

        let arguments = arguments
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }
        // SAFETY: AUMID 固定；arguments 在同步调用完成前保持 NUL 结尾且有效。
        unsafe {
            manager.ActivateApplication(
                w!("OpenAI.Codex_2p2nqsd0c76g0!App"),
                PCWSTR(arguments.as_ptr()),
                AO_NONE,
            )
        }
        .map(|_| ())
        .map_err(|_| control.failure_or_cancelled(StoreActivationFailure::Activation))
    })();

    // SAFETY: 仅在 CoEnableCallCancellation 成功后才可能进入 COM_CALL phase。
    if control.phase.load(Ordering::Acquire) == ACTIVATION_PHASE_COM_CALL
        && unsafe { CoDisableCallCancellation(None) }.is_err()
    {
        tracing::warn!("Windows failed to disable COM call cancellation during worker cleanup");
    }
    // SAFETY: 与本线程开头成功的 CoInitializeEx 配对。
    unsafe { CoUninitialize() };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    #[tokio::test]
    /// 工作线程返回的业务失败必须原样送达等待方并完成回收。
    async fn worker_failure_is_returned_to_the_caller() {
        let owner = Arc::new(StoreActivationOwner::default());
        let lease = owner
            .start(|_| Err(StoreActivationFailure::ManagerCreation))
            .expect("worker starts");

        let outcome = lease
            .wait(Duration::from_secs(1), Duration::from_millis(50))
            .await
            .expect("lifecycle completes");

        assert_eq!(outcome, Err(StoreActivationFailure::ManagerCreation));
        assert_eq!(owner.owned_task_count(), 0);
    }

    #[tokio::test]
    /// 工作线程 panic 必须转成可观察失败，且句柄最终由 owner 回收。
    async fn worker_panic_is_observable_and_reaped() {
        let owner = Arc::new(StoreActivationOwner::default());
        let lease = owner
            .start(|_| panic!("simulated activation panic"))
            .expect("worker starts");

        let outcome = lease
            .wait(Duration::from_secs(1), Duration::from_millis(50))
            .await
            .expect("lifecycle completes");

        assert_eq!(outcome, Err(StoreActivationFailure::WorkerPanicked));
        assert_eq!(owner.owned_task_count(), 0);
    }

    #[tokio::test]
    /// 等待方被丢弃时应取消协作式工作线程并等待 owner 清空。
    async fn dropping_the_waiter_cancels_and_reaps_a_cooperative_worker() {
        let owner = Arc::new(StoreActivationOwner::default());
        let lease = owner
            .start(|control| {
                while !control.is_cancelled() {
                    thread::park_timeout(Duration::from_millis(1));
                }
                Err(StoreActivationFailure::Cancelled)
            })
            .expect("worker starts");
        assert_eq!(owner.owned_task_count(), 1);

        drop(lease);
        let deadline = Instant::now() + Duration::from_secs(1);
        while owner.owned_task_count() != 0 && Instant::now() < deadline {
            thread::park_timeout(Duration::from_millis(1));
        }

        assert_eq!(owner.owned_task_count(), 0);
    }

    #[tokio::test]
    /// 超时应保持有界，同时在工作线程终止前持续保留其所有权。
    async fn timeout_is_bounded_and_owner_retains_worker_until_terminal() {
        let owner = Arc::new(StoreActivationOwner::default());
        let (release_tx, release_rx) = mpsc::channel();
        let lease = owner
            .start(move |_| {
                let _ = release_rx.recv();
                Err(StoreActivationFailure::Cancelled)
            })
            .expect("worker starts");
        let started_at = Instant::now();

        let result = lease
            .wait(Duration::from_millis(30), Duration::from_millis(10))
            .await;

        assert!(matches!(
            result,
            Err(StoreActivationLifecycleFailure::Timeout { .. })
        ));
        assert!(started_at.elapsed() < Duration::from_secs(1));
        assert_eq!(owner.owned_task_count(), 1);

        release_tx.send(()).expect("release worker");
        let deadline = Instant::now() + Duration::from_secs(1);
        while owner.owned_task_count() != 0 && Instant::now() < deadline {
            thread::park_timeout(Duration::from_millis(1));
        }
        assert_eq!(owner.owned_task_count(), 0);
    }

    #[test]
    /// 已有副作用激活运行时必须拒绝并发启动第二个激活。
    fn concurrent_side_effecting_activation_is_rejected() {
        let owner = Arc::new(StoreActivationOwner::default());
        let (release_tx, release_rx) = mpsc::channel();
        let first = owner
            .start(move |_| {
                let _ = release_rx.recv();
                Ok(())
            })
            .expect("first worker starts");

        assert!(matches!(
            owner.start(|_| Ok(())),
            Err(StoreActivationLifecycleFailure::Busy)
        ));

        release_tx.send(()).expect("release first worker");
        drop(first);
        let deadline = Instant::now() + Duration::from_secs(1);
        while owner.owned_task_count() != 0 && Instant::now() < deadline {
            thread::park_timeout(Duration::from_millis(1));
        }
        assert_eq!(owner.owned_task_count(), 0);
    }
}
