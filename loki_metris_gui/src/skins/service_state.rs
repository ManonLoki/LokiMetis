// 本文件收纳：换肤服务 `SkinService` 的状态定义。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

/// 换肤服务：持有皮肤库、导入暂存、宿主操作代次与取消、账户资料与端点缓存、监视任务回收器；`operation` 串行化所有改动宿主的操作，`runtime` 保存各实例运行态。
pub struct SkinService {
    builtin_root: PathBuf,
    user_root: PathBuf,
    catalog: StdRwLock<Option<CachedCatalog>>,
    pending_import: StdMutex<Option<PendingImportBatch>>,
    import_preparation: StdMutex<Option<ActiveImportPreparation>>,
    next_codex_operation_id: AtomicU64,
    codex_operation: StdMutex<Option<CodexOperation>>,
    codex_runtime_generation: AtomicU64,
    workbuddy_runtime_generation: AtomicU64,
    account_profile_probes: StdMutex<HashMap<String, Arc<OnceCell<Option<AccountProfile>>>>>,
    verified_endpoint_hints: StdMutex<HashMap<SkinHostKind, CdpEndpoint>>,
    watch_task_reaper: Arc<StdMutex<WatchTaskReaper>>,
    operation: Mutex<()>,
    runtime: Mutex<RuntimeState>,
}
