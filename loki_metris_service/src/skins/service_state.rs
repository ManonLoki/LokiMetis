//! 换肤服务 `SkinService` 的状态定义。

use super::*;

/// 换肤服务：持有皮肤库、导入暂存、宿主操作代次与取消、账户资料与端点缓存、监视任务回收器；`operation` 串行化所有改动宿主的操作，`runtime` 保存各实例运行态。
pub struct SkinService {
    /// 随应用分发的内置皮肤目录，只读。
    pub(super) builtin_root: PathBuf,
    /// 用户导入皮肤的目录。
    pub(super) user_root: PathBuf,
    /// 皮肤目录扫描缓存；目录指纹不变时复用。
    pub(super) catalog: StdRwLock<Option<CachedCatalog>>,
    /// 已完成预检、等待确认安装的导入批次。
    pub(super) pending_import: StdMutex<Option<PendingImportBatch>>,
    /// 进行中的导入预检。
    pub(super) import_preparation: StdMutex<Option<ActiveImportPreparation>>,
    /// 下一次宿主操作的 ID，单调递增。
    pub(super) next_codex_operation_id: AtomicU64,
    /// 当前在途的宿主操作登记；退出或停止监视时据此发出取消。
    pub(super) codex_operation: StdMutex<Option<CodexOperation>>,
    /// Codex 运行代次；变更期间为奇数，结束后恢复为新的偶数。
    pub(super) codex_runtime_generation: AtomicU64,
    /// WorkBuddy 运行代次，规则同上。
    pub(super) workbuddy_runtime_generation: AtomicU64,
    /// 按实例 ID 缓存的账户资料探测结果，同一实例只探测一次。
    pub(super) account_profile_probes: StdMutex<HashMap<String, Arc<OnceCell<Option<AccountProfile>>>>>,
    /// 各宿主最近一次通过宿主页探针的 CDP 端点，仅作提示；使用前仍会重新验证。
    pub(super) verified_endpoint_hints: StdMutex<HashMap<SkinHostKind, CdpEndpoint>>,
    /// 页面监视任务的回收器。
    pub(super) watch_task_reaper: Arc<StdMutex<WatchTaskReaper>>,
    /// 串行化所有改动宿主的操作。
    pub(super) operation: Mutex<()>,
    /// 各实例的换肤运行态。
    pub(super) runtime: Mutex<RuntimeState>,
}
