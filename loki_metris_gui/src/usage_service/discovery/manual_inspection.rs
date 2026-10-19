//! 手动所选目录的有界签名核对、取消票据与后台任务所有权。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use loki_metris_core::{
    DiscoveryMethod, ManualSourceInspectOutcome, RootDiscoveryCoordinator, SourceClientKind,
    source_root_alias_from_path,
};

use crate::backend::local_index::{
    CancellationToken, ClaudeRootInspection, ClaudeSignatureBudget, FullDiscoveryOptions,
    RootInspection, SignatureProbeContext, inspect_claude_root, inspect_root,
    validate_local_plain_directory,
};
use super::super::background::BackgroundTaskOwner;

/// 所选目录首次结构签名检查的总墙钟时限。
pub(super) const MANUAL_INSPECTION_TIMEOUT: Duration = Duration::from_secs(5);
/// 等待检查结果时复核协调器取消状态的短轮询间隔。
const MANUAL_INSPECTION_CANCEL_POLL: Duration = Duration::from_millis(20);
/// 单次首次签名检查允许递归进入的目录上限。
const MANUAL_SIGNATURE_DIRECTORY_LIMIT: u64 = 8_192;
/// 单次首次签名检查允许读取的目录项上限。
const MANUAL_SIGNATURE_ENTRY_LIMIT: u64 = 100_000;
/// 单次首次签名检查允许打开的候选签名文件上限。
const MANUAL_SIGNATURE_FILE_LIMIT: u64 = 256;
/// 单次首次签名检查允许读取的文件前缀总字节上限。
const MANUAL_SIGNATURE_BYTE_LIMIT: u64 = 16 * 1024 * 1024;

/// 单次结构探测的结果：核对结论与（找到时）实际根路径。
pub(super) struct InspectBundle {
    /// core 归约核对结论所需的输入。
    pub(super) decision_input: ManualSourceInspectOutcome,
    /// 探测命中时的实际根路径。
    pub(super) found_path: Option<PathBuf>,
}

impl InspectBundle {
    /// 构造探测确认命中的结果。
    fn found(path: PathBuf) -> Self {
        Self {
            decision_input: ManualSourceInspectOutcome::Found,
            found_path: Some(path),
        }
    }

    /// 构造未能确认为合格根的结果。
    fn unverified(decision_input: ManualSourceInspectOutcome) -> Self {
        Self {
            decision_input,
            found_path: None,
        }
    }
}

/// 首次签名检查在命令等待边界产生的终态。
pub(super) enum ManualInspectionCompletion {
    /// 阻塞检查在时限内正常返回。
    Finished(io::Result<InspectBundle>),
    /// 用户、协调器或应用关闭请求了取消。
    Cancelled,
    /// 检查超过固定总墙钟时限。
    DeadlineExceeded,
    /// 后台 owner 拒绝任务或 worker 未能交付结果。
    WorkerUnavailable,
}

/// 命令仅持有结果票据与共享取消句柄；真正的 blocking JoinHandle 留在 owner。
struct ManualInspectionRequest {
    /// blocking 检查结果的接收端。
    receiver: tokio::sync::oneshot::Receiver<io::Result<InspectBundle>>,
    /// 与检查 worker 共享的取消句柄。
    cancellation: CancellationToken,
    /// 发现协调器，用于观察用户的显式取消。
    coordinator: Arc<RootDiscoveryCoordinator>,
    /// 尚未得到终态；为 true 时被丢弃会立即传播取消。
    armed: bool,
}

impl ManualInspectionRequest {
    /// 在固定总时限内等待结果，并周期复核显式协调器取消。
    async fn wait(mut self, timeout: Duration) -> ManualInspectionCompletion {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self.cancellation.is_cancelled() || self.coordinator.is_cancel_requested() {
                self.cancel();
                self.armed = false;
                return ManualInspectionCompletion::Cancelled;
            }
            tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => {
                    self.cancel();
                    self.armed = false;
                    return ManualInspectionCompletion::DeadlineExceeded;
                }
                result = &mut self.receiver => {
                    if tokio::time::Instant::now() >= deadline {
                        self.cancel();
                        self.armed = false;
                        return ManualInspectionCompletion::DeadlineExceeded;
                    }
                    if self.cancellation.is_cancelled()
                        || self.coordinator.is_cancel_requested()
                    {
                        self.cancel();
                        self.armed = false;
                        return ManualInspectionCompletion::Cancelled;
                    }
                    self.armed = false;
                    return result.map_or(
                        ManualInspectionCompletion::WorkerUnavailable,
                        ManualInspectionCompletion::Finished,
                    );
                }
                () = tokio::time::sleep(MANUAL_INSPECTION_CANCEL_POLL) => {}
            }
        }
    }

    /// 同时取消签名令牌与本轮协调器状态，重复调用保持幂等。
    fn cancel(&self) {
        self.cancellation.cancel();
        let _ = self.coordinator.request_cancel();
    }
}

impl Drop for ManualInspectionRequest {
    /// 调用方 future 被撤销时也立即传播取消；worker 句柄仍由后台 owner 持有。
    fn drop(&mut self) {
        if self.armed {
            self.cancel();
        }
    }
}

/// 把首次签名检查从创建起登记到应用后台 owner，并等待到取消或总 deadline。
pub(super) async fn inspect_selected_path_owned<Lease>(
    owner: &BackgroundTaskOwner,
    coordinator: Arc<RootDiscoveryCoordinator>,
    client: SourceClientKind,
    path: PathBuf,
    reservation: Arc<Lease>,
    timeout: Duration,
) -> ManualInspectionCompletion
where
    Lease: Send + Sync + 'static,
{
    let cancellation = CancellationToken::new();
    let options = manual_signature_options();
    let request = match spawn_owned_manual_inspection(
        owner,
        Arc::clone(&coordinator),
        cancellation.clone(),
        reservation,
        move |cancellation| {
            validate_local_plain_directory(&path)
                .map(|()| inspect_selected_path(client, &path, &options, cancellation))
        },
    ) {
        Ok(request) => request,
        Err(()) => return ManualInspectionCompletion::WorkerUnavailable,
    };
    request.wait(timeout).await
}

/// 为所选目录首次核对建立固定、有限且不随目录规模增长的签名预算。
fn manual_signature_options() -> FullDiscoveryOptions {
    FullDiscoveryOptions {
        max_signature_directories: MANUAL_SIGNATURE_DIRECTORY_LIMIT,
        max_signature_entries: MANUAL_SIGNATURE_ENTRY_LIMIT,
        max_signature_files: MANUAL_SIGNATURE_FILE_LIMIT,
        max_signature_bytes: MANUAL_SIGNATURE_BYTE_LIMIT,
        ..FullDiscoveryOptions::default()
    }
}

/// 登记一项可向命令回传结果的阻塞检查；超时后句柄仍始终归 owner 所有。
fn spawn_owned_manual_inspection<Lease, Inspect>(
    owner: &BackgroundTaskOwner,
    coordinator: Arc<RootDiscoveryCoordinator>,
    cancellation: CancellationToken,
    reservation: Arc<Lease>,
    inspect: Inspect,
) -> Result<ManualInspectionRequest, ()>
where
    Lease: Send + Sync + 'static,
    Inspect: FnOnce(&CancellationToken) -> io::Result<InspectBundle> + Send + 'static,
{
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let shutdown_cancellation = cancellation.clone();
    let task_cancellation = cancellation.clone();
    let task_coordinator = Arc::clone(&coordinator);
    owner
        .spawn_blocking_cancelable(
            "manual-source-inspection",
            move || shutdown_cancellation.cancel(),
            move |shutdown| {
                let _reservation = reservation;
                if shutdown.is_cancelled() || task_coordinator.is_cancel_requested() {
                    task_cancellation.cancel();
                }
                let result = if task_cancellation.is_cancelled() {
                    Ok(InspectBundle::unverified(
                        ManualSourceInspectOutcome::Cancelled,
                    ))
                } else {
                    inspect(&task_cancellation)
                };
                if shutdown.is_cancelled() || task_coordinator.is_cancel_requested() {
                    task_cancellation.cancel();
                }
                let result = if task_cancellation.is_cancelled() {
                    Ok(InspectBundle::unverified(
                        ManualSourceInspectOutcome::Cancelled,
                    ))
                } else {
                    result
                };
                let _ = sender.send(result);
            },
        )
        .map_err(|_| ())?;
    Ok(ManualInspectionRequest {
        receiver,
        cancellation,
        coordinator,
        armed: true,
    })
}

/// 按客户端类型选择结构签名探测策略，核对用户所选路径。
fn inspect_selected_path(
    client: SourceClientKind,
    path: &Path,
    options: &FullDiscoveryOptions,
    cancellation: &CancellationToken,
) -> InspectBundle {
    match client {
        SourceClientKind::Codex => {
            let alias = source_root_alias_from_path(path, client);
            let mut probe = SignatureProbeContext::new(options, cancellation);
            classify_codex_inspection(inspect_root(
                path,
                alias,
                DiscoveryMethod::Registered,
                None,
                Some(&mut probe),
            ))
        }
        SourceClientKind::ClaudeCode => {
            let mut budget = ClaudeSignatureBudget::new(options, cancellation);
            classify_claude_inspection(path, inspect_claude_root(path, &mut budget))
        }
        SourceClientKind::GrokBuildCli => {
            let mut budget =
                crate::backend::local_index::GrokSignatureBudget::new(options, cancellation);
            classify_grok_inspection(
                path,
                crate::backend::local_index::inspect_grok_root(path, &mut budget),
            )
        }
        SourceClientKind::WorkBuddy => {
            unreachable!("手动添加入口只接受 AgentClientKindDto 的三个批准客户端")
        }
    }
}

/// 把 Codex 结构探测结论映射为统一的核对结果。
fn classify_codex_inspection(inspection: RootInspection) -> InspectBundle {
    match inspection {
        RootInspection::Found(root) => InspectBundle::found(root.path),
        RootInspection::NotRoot => InspectBundle::unverified(ManualSourceInspectOutcome::NotRoot),
        RootInspection::RejectedSignature => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Rejected)
        }
        RootInspection::Indeterminate => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Indeterminate)
        }
        RootInspection::Cancelled => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Cancelled)
        }
        RootInspection::BudgetExhausted => {
            InspectBundle::unverified(ManualSourceInspectOutcome::BudgetExhausted)
        }
    }
}

/// 把 Grok 结构探测结论映射为统一的核对结果。
fn classify_grok_inspection(
    path: &Path,
    inspection: crate::backend::local_index::GrokRootInspection,
) -> InspectBundle {
    use crate::backend::local_index::GrokRootInspection;
    match inspection {
        GrokRootInspection::Found(_) => InspectBundle::found(path.to_path_buf()),
        GrokRootInspection::NotRoot => {
            InspectBundle::unverified(ManualSourceInspectOutcome::NotRoot)
        }
        GrokRootInspection::Rejected => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Rejected)
        }
        GrokRootInspection::Indeterminate => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Indeterminate)
        }
        GrokRootInspection::Cancelled => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Cancelled)
        }
        GrokRootInspection::BudgetExhausted => {
            InspectBundle::unverified(ManualSourceInspectOutcome::BudgetExhausted)
        }
    }
}

/// 把 Claude 结构探测结论映射为统一的核对结果。
fn classify_claude_inspection(path: &Path, inspection: ClaudeRootInspection) -> InspectBundle {
    match inspection {
        ClaudeRootInspection::Found(_) => InspectBundle::found(path.to_path_buf()),
        ClaudeRootInspection::NotRoot => {
            InspectBundle::unverified(ManualSourceInspectOutcome::NotRoot)
        }
        ClaudeRootInspection::Rejected => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Rejected)
        }
        ClaudeRootInspection::Indeterminate => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Indeterminate)
        }
        ClaudeRootInspection::Cancelled => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Cancelled)
        }
        ClaudeRootInspection::BudgetExhausted => {
            InspectBundle::unverified(ManualSourceInspectOutcome::BudgetExhausted)
        }
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - manual_inspection_tests.rs：本模块的单元测试
include!("manual_inspection_tests.rs");
