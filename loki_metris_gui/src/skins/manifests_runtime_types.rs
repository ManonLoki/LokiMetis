// 本文件是 manifests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：定义换皮宿主 `InstanceRuntime` 使用的内部数据。；定义换皮宿主 `RuntimeState` 使用的内部数据。；定义换皮宿主 `PendingImportItem` 使用的内部数据。

/// 定义换皮宿主 `InstanceRuntime` 使用的内部数据。
struct InstanceRuntime {
    active: Option<SkinDescriptor>,
    task: Option<WatchTask>,
    compatibility: Option<SkinCompatibilityStatus>,
}

#[derive(Default)]
/// 定义换皮宿主 `RuntimeState` 使用的内部数据。
struct RuntimeState {
    instances: HashMap<String, InstanceRuntime>,
    last_targets: HashMap<SkinHostKind, String>,
}

/// 定义换皮宿主 `PendingImportItem` 使用的内部数据。
struct PendingImportItem {
    item_id: String,
    archive_name: String,
    staging: PathBuf,
    candidate: SkinDescriptor,
}

/// 定义换皮宿主 `PendingImportBatch` 使用的内部数据。
struct PendingImportBatch {
    token: String,
    staging_root: PathBuf,
    items: Vec<PendingImportItem>,
    skipped: Vec<SkippedSkinImport>,
}

/// 定义换皮宿主 `ActiveImportPreparation` 使用的内部数据。
struct ActiveImportPreparation {
    token: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
/// 定义换皮宿主 `CachedCatalog` 使用的内部数据。
struct CachedCatalog {
    fingerprint: Vec<CatalogDirectoryFingerprint>,
    skins: Vec<SkinDescriptor>,
}

#[derive(Clone, PartialEq, Eq)]
/// 定义换皮宿主 `CatalogDirectoryFingerprint` 使用的内部数据。
struct CatalogDirectoryFingerprint {
    source: SkinSource,
    name: OsString,
    modified: Option<SystemTime>,
    children_readable: bool,
    children: Vec<CatalogChildFingerprint>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
/// 定义换皮宿主 `CatalogChildFingerprint` 使用的内部数据。
struct CatalogChildFingerprint {
    name: OsString,
    kind: u8,
    length: u64,
    modified: Option<SystemTime>,
}

/// 定义换皮宿主 `CatalogSkin` 使用的内部数据。
struct CatalogSkin {
    descriptor: SkinDescriptor,
    modified: Option<SystemTime>,
}

#[derive(Default)]
/// 定义换皮宿主 `InjectionReport` 使用的内部数据。
struct InjectionReport {
    verified_pages: usize,
    injected_pages: usize,
    failed_pages: usize,
    first_failure: Option<AppError>,
    transaction_targets: BTreeSet<String>,
    compatibility_applied_rules: BTreeSet<String>,
    compatibility_skipped_rules: BTreeSet<String>,
}

#[derive(Clone)]
/// 保存一次初始注入的随机所有权标记与已登记页面；外层取消 future 后仍可回滚。
struct InjectionTransaction {
    id: Arc<str>,
    targets: Arc<StdMutex<BTreeSet<String>>>,
}

impl InjectionTransaction {
    /// 创建尚未登记页面的注入事务，并保存不可变所有权标记。
    fn new(id: &str) -> Self {
        Self {
            id: Arc::from(id),
            targets: Arc::new(StdMutex::new(BTreeSet::new())),
        }
    }

    /// 返回用于页面 DOM marker 的本次事务标识。
    fn id(&self) -> &str {
        &self.id
    }

    /// 必须在写 DOM marker 前登记；锁中毒时保留集合并继续完成补偿路径。
    fn track(&self, target_id: String) {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target_id);
    }

    /// 取得已登记页面的快照，供提交或补偿阶段稳定遍历。
    fn tracked_targets(&self) -> BTreeSet<String> {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl InjectionReport {
    /// 执行换皮宿主内部的 `include_compatibility` 步骤。
    fn include_compatibility(&mut self, report: CompatibilityPageReport) {
        self.compatibility_applied_rules
            .extend(report.applied_rules);
        self.compatibility_skipped_rules
            .extend(report.skipped_rules);
    }

    /// 执行换皮宿主内部的 `compatibility_status` 步骤。
    fn compatibility_status(&self) -> SkinCompatibilityStatus {
        let mode = if self.compatibility_skipped_rules.is_empty() {
            if self.compatibility_applied_rules.is_empty() {
                SkinCompatibilityMode::Native
            } else {
                SkinCompatibilityMode::Adapted
            }
        } else {
            SkinCompatibilityMode::Partial
        };
        SkinCompatibilityStatus {
            version: HOST_COMPATIBILITY_VERSION.into(),
            mode,
            applied_rules: self.compatibility_applied_rules.iter().cloned().collect(),
            skipped_rules: self.compatibility_skipped_rules.iter().cloned().collect(),
        }
    }
}
