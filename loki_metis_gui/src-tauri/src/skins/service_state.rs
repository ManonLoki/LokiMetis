/// 定义换皮宿主 `SkinService` 使用的内部数据。
pub struct SkinService {
    builtin_root: PathBuf,
    user_root: PathBuf,
    catalog: StdRwLock<Option<CachedCatalog>>,
    pending_import: StdMutex<Option<PendingImportBatch>>,
    import_preparation: StdMutex<Option<ActiveImportPreparation>>,
    next_codex_operation_id: AtomicU64,
    codex_operation: StdMutex<Option<CodexOperation>>,
    account_profile_probes: StdMutex<HashMap<String, Arc<OnceCell<Option<AccountProfile>>>>>,
    operation: Mutex<()>,
    runtime: Mutex<RuntimeState>,
}
