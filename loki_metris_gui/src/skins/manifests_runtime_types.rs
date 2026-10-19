// 本文件由 skins/mod.rs include! 并入同一模块，无需额外导入。
// 收纳实例运行态、导入暂存批次、目录缓存指纹与注入汇总报告。

/// 单个宿主实例的换肤运行态：生效皮肤、页面监视任务与兼容结果。
struct InstanceRuntime {
    /// 当前生效皮肤。
    active: Option<SkinDescriptor>,
    /// 页面监视任务。
    task: Option<WatchTask>,
    /// 兼容结果。
    compatibility: Option<SkinCompatibilityStatus>,
}

#[derive(Default)]
/// 全部实例的运行态，以及每种宿主最近一次操作的目标实例。
struct RuntimeState {
    /// 按实例 ID 索引的运行态。
    instances: HashMap<String, InstanceRuntime>,
    /// 各宿主最近一次操作的目标实例，未指定实例时沿用。
    last_targets: HashMap<SkinHostKind, String>,
}

/// 导入预检中暂存的一个归档：暂存目录与候选描述。
struct PendingImportItem {
    /// 条目 ID。
    item_id: String,
    /// 归档文件名。
    archive_name: String,
    /// 暂存目录。
    staging: PathBuf,
    /// 候选皮肤描述。
    candidate: SkinDescriptor,
}

/// 一次导入预检的暂存批次，确认或取消时整体回收暂存根目录。
struct PendingImportBatch {
    /// 批次令牌。
    token: String,
    /// 批次暂存根目录，确认或取消时整体删除。
    staging_root: PathBuf,
    /// 预检通过的条目。
    items: Vec<PendingImportItem>,
    /// 被跳过的归档。
    skipped: Vec<SkippedSkinImport>,
}

/// 正在进行的导入预检，以 token 标识并可协作取消。
struct ActiveImportPreparation {
    /// 预检令牌。
    token: String,
    /// 协作取消标志。
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
/// 皮肤目录扫描缓存；目录指纹不变时直接复用上次解析结果。
struct CachedCatalog {
    /// 扫描时的目录指纹。
    fingerprint: Vec<CatalogDirectoryFingerprint>,
    /// 解析出的皮肤。
    skins: Vec<SkinDescriptor>,
}

#[derive(Clone, PartialEq, Eq)]
/// 皮肤库中一个皮肤目录的指纹：名称、修改时间及子项列表。
struct CatalogDirectoryFingerprint {
    /// 所属来源。
    source: SkinSource,
    /// 目录名。
    name: OsString,
    /// 目录修改时间。
    modified: Option<SystemTime>,
    /// 子项可以列出。
    children_readable: bool,
    /// 子项指纹，已排序。
    children: Vec<CatalogChildFingerprint>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
/// 皮肤目录子项的指纹：名称、类型、长度与修改时间。
struct CatalogChildFingerprint {
    /// 子项名。
    name: OsString,
    /// 类型码：0 文件、1 目录、2 符号链接、3 其它、255 读取失败。
    kind: u8,
    /// 字节长度。
    length: u64,
    /// 修改时间。
    modified: Option<SystemTime>,
}

/// 目录扫描得到的皮肤及其修改时间，目录列表按修改时间从新到旧排列。
struct CatalogSkin {
    /// 皮肤描述。
    descriptor: SkinDescriptor,
    /// 目录修改时间，用于排序。
    modified: Option<SystemTime>,
}

#[derive(Default)]
/// 一次注入的汇总报告：验证、注入、失败的页面数，首个失败原因、事务目标与兼容规则。
struct InjectionReport {
    /// 已确认身份的页面数。
    verified_pages: usize,
    /// 注入成功的页面数。
    injected_pages: usize,
    /// 注入失败的页面数。
    failed_pages: usize,
    /// 首个失败原因。
    first_failure: Option<AppError>,
    /// 本次事务登记的页面 ID，回滚时据此清理。
    transaction_targets: BTreeSet<String>,
    /// 已应用的兼容规则。
    compatibility_applied_rules: BTreeSet<String>,
    /// 无法应用的兼容规则。
    compatibility_skipped_rules: BTreeSet<String>,
}

#[derive(Clone)]
/// 保存一次初始注入的随机所有权标记与已登记页面；外层取消 future 后仍可回滚。
struct InjectionTransaction {
    /// 随机所有权标记，写入页面以识别本次注入。
    id: Arc<str>,
    /// 已登记的页面 ID。
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
    /// 合并单个页面回报的兼容规则。
    fn include_compatibility(&mut self, report: CompatibilityPageReport) {
        self.compatibility_applied_rules
            .extend(report.applied_rules);
        self.compatibility_skipped_rules
            .extend(report.skipped_rules);
    }

    /// 按已应用与跳过的规则得出兼容模式。
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
