//! 实例运行态、导入暂存批次、目录缓存指纹与注入汇总报告。

use super::*;

/// 单个宿主实例的换肤运行态：生效皮肤、页面监视任务与兼容结果。
pub(super) struct InstanceRuntime {
    /// 当前生效皮肤。
    pub(super) active: Option<SkinDescriptor>,
    /// 页面监视任务。
    pub(super) task: Option<WatchTask>,
    /// 兼容结果。
    pub(super) compatibility: Option<SkinCompatibilityStatus>,
}

#[derive(Default)]
/// 全部实例的运行态，以及每种宿主最近一次操作的目标实例。
pub(super) struct RuntimeState {
    /// 按实例 ID 索引的运行态。
    pub(super) instances: HashMap<String, InstanceRuntime>,
    /// 各宿主最近一次操作的目标实例，未指定实例时沿用。
    pub(super) last_targets: HashMap<SkinHostKind, String>,
}

/// 导入预检中暂存的一个归档：暂存目录与候选描述。
pub(super) struct PendingImportItem {
    /// 条目 ID。
    pub(super) item_id: String,
    /// 归档文件名。
    pub(super) archive_name: String,
    /// 暂存目录。
    pub(super) staging: PathBuf,
    /// 候选皮肤描述。
    pub(super) candidate: SkinDescriptor,
}

/// 一次导入预检的暂存批次，确认或取消时整体回收暂存根目录。
pub(super) struct PendingImportBatch {
    /// 批次令牌。
    pub(super) token: String,
    /// 批次暂存根目录，确认或取消时整体删除。
    pub(super) staging_root: PathBuf,
    /// 预检通过的条目。
    pub(super) items: Vec<PendingImportItem>,
    /// 被跳过的归档。
    pub(super) skipped: Vec<SkippedSkinImport>,
}

/// 正在进行的导入预检，以 token 标识并可协作取消。
pub(super) struct ActiveImportPreparation {
    /// 预检令牌。
    pub(super) token: String,
    /// 协作取消标志。
    pub(super) cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
/// 皮肤目录扫描缓存；目录指纹不变时直接复用上次解析结果。
pub(super) struct CachedCatalog {
    /// 扫描时的目录指纹。
    pub(super) fingerprint: Vec<CatalogDirectoryFingerprint>,
    /// 解析出的皮肤。
    pub(super) skins: Vec<SkinDescriptor>,
}

#[derive(Clone, PartialEq, Eq)]
/// 皮肤库中一个皮肤目录的指纹：名称、修改时间及子项列表。
pub(super) struct CatalogDirectoryFingerprint {
    /// 所属来源。
    pub(super) source: SkinSource,
    /// 目录名。
    pub(super) name: OsString,
    /// 目录修改时间。
    pub(super) modified: Option<SystemTime>,
    /// 子项可以列出。
    pub(super) children_readable: bool,
    /// 子项指纹，已排序。
    pub(super) children: Vec<CatalogChildFingerprint>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
/// 皮肤目录子项的指纹：名称、类型、长度与修改时间。
pub(super) struct CatalogChildFingerprint {
    /// 子项名。
    pub(super) name: OsString,
    /// 类型码：0 文件、1 目录、2 符号链接、3 其它、255 读取失败。
    pub(super) kind: u8,
    /// 字节长度。
    pub(super) length: u64,
    /// 修改时间。
    pub(super) modified: Option<SystemTime>,
}

/// 目录扫描得到的皮肤及其修改时间，目录列表按修改时间从新到旧排列。
pub(super) struct CatalogSkin {
    /// 皮肤描述。
    pub(super) descriptor: SkinDescriptor,
    /// 目录修改时间，用于排序。
    pub(super) modified: Option<SystemTime>,
}

#[derive(Default)]
/// 一次注入的汇总报告：验证、注入、失败的页面数，首个失败原因、事务目标与兼容规则。
pub(super) struct InjectionReport {
    /// 已确认身份的页面数。
    pub(super) verified_pages: usize,
    /// 注入成功的页面数。
    pub(super) injected_pages: usize,
    /// 注入失败的页面数。
    pub(super) failed_pages: usize,
    /// 首个失败原因。
    pub(super) first_failure: Option<AppError>,
    /// 本次事务登记的页面 ID，回滚时据此清理。
    pub(super) transaction_targets: BTreeSet<String>,
    /// 已应用的兼容规则。
    pub(super) compatibility_applied_rules: BTreeSet<String>,
    /// 无法应用的兼容规则。
    pub(super) compatibility_skipped_rules: BTreeSet<String>,
}

#[derive(Clone)]
/// 保存一次初始注入的随机所有权标记与已登记页面；外层取消 future 后仍可回滚。
pub(super) struct InjectionTransaction {
    /// 随机所有权标记，写入页面以识别本次注入。
    pub(super) id: Arc<str>,
    /// 已登记的页面 ID。
    targets: Arc<StdMutex<BTreeSet<String>>>,
}

impl InjectionTransaction {
    /// 创建尚未登记页面的注入事务，并保存不可变所有权标记。
    pub(super) fn new(id: &str) -> Self {
        Self {
            id: Arc::from(id),
            targets: Arc::new(StdMutex::new(BTreeSet::new())),
        }
    }

    /// 返回用于页面 DOM marker 的本次事务标识。
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    /// 必须在写 DOM marker 前登记；锁中毒时保留集合并继续完成补偿路径。
    pub(super) fn track(&self, target_id: String) {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target_id);
    }

    /// 取得已登记页面的快照，供提交或补偿阶段稳定遍历。
    pub(super) fn tracked_targets(&self) -> BTreeSet<String> {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl InjectionReport {
    /// 合并单个页面回报的兼容规则。
    pub(super) fn include_compatibility(&mut self, report: CompatibilityPageReport) {
        self.compatibility_applied_rules
            .extend(report.applied_rules);
        self.compatibility_skipped_rules
            .extend(report.skipped_rules);
    }

    /// 按已应用与跳过的规则得出兼容模式。
    pub(super) fn compatibility_status(&self) -> SkinCompatibilityStatus {
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
