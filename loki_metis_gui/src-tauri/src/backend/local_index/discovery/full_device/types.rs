//! 全设备发现的内部任务、预算与 worker 结果类型。

use std::fs;
use std::path::{Path, PathBuf};

use super::super::FullDiscoveryOptions;

/// 保存一个待验证目录及其所属卷起点；路径只在 backend 内部流转。
pub(super) struct TraversalItem {
    /// 本轮准备检查的目录候选。
    pub(super) path: PathBuf,
    /// 不做 canonicalize 的同层词法路径，只用于卷排除与跨根策略比较。
    pub(super) policy_path: PathBuf,
    /// 候选最初来自的词法本地卷起点，用于排除跨卷下钻。
    pub(super) traversal_root: PathBuf,
    /// 是否为调用方显式卷起点；该事实不能依赖原路径与 canonical 路径字节相等。
    pub(super) is_traversal_root: bool,
}

/// 保存一个可跨 ticket 继续消费的 `read_dir` 游标，不重复打开已经开始的目录。
pub(super) struct DirectoryCursor {
    /// 已通过预检并完成根签名判断的规范化目录。
    pub(super) path: PathBuf,
    /// 目录所属的本地卷起点。
    pub(super) traversal_root: PathBuf,
    /// 与当前目录对应的词法策略路径；派生子项时只追加 `file_name`。
    pub(super) policy_path: PathBuf,
    /// 首个 ticket 前为空；首次打开后由后续 ticket 继续持有。
    pub(super) entries: Option<fs::ReadDir>,
    /// 上一 ticket 只为区分“额度恰好耗尽”与“仍有 N+1 项”而预读的目录项。
    pub(super) buffered_entry: Option<std::io::Result<fs::DirEntry>>,
    /// 当前目录跨多个 ticket 累积的普通子目录；只在 EOF 后统一稳定排序并提交。
    pub(super) children: Vec<TraversalItem>,
}

/// 表示协调器尚未派发的两类确定性工作。
pub(super) enum PendingTask {
    /// 验证一个路径仍是普通本地目录并取得规范化位置。
    Preflight(TraversalItem),
    /// 在固定目录项配额内继续枚举一个已确认非 Codex 根的目录。
    Enumerate(Box<DirectoryCursor>),
}

impl PendingTask {
    /// 返回任务内部路径，只供测试观察并发，不发布到 GUI 或日志。
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Preflight(item) => &item.path,
            Self::Enumerate(cursor) => &cursor.path,
        }
    }
}

/// 保存已经取得目录项额度的 worker 任务。
pub(super) struct WorkerTask {
    /// 轮内确定性 ticket；结果必须按该值提交。
    pub(super) order: usize,
    /// 待执行的目录工作。
    pub(super) pending: PendingTask,
    /// 枚举任务本 ticket 最多处理的目录项；预检任务恒为零。
    pub(super) entry_quota: u64,
}

/// 区分目录预检的可提交结果，避免 worker 直接修改全局覆盖事实。
pub(super) enum PreflightOutcome {
    /// 候选仍是普通目录，并携带规范化访问位置及所属遍历卷。
    Ready {
        /// 去除可解析别名后的目录访问位置。
        normalized: PathBuf,
        /// 本次完全扫描显式确认的本地持久卷起点。
        traversal_root: PathBuf,
        /// 不访问文件系统的词法策略路径。
        policy_path: PathBuf,
    },
    /// 候选或卷起点包含链接类组件。
    Symlink,
    /// 读取候选元数据被操作系统拒绝。
    PermissionDenied,
    /// 其他 I/O 不确定状态；调用方只能记为跳过。
    Skipped,
    /// 候选不是目录，不进入根识别或下钻。
    NotDirectory,
    /// 用户已请求取消，协调器不得提交后续 ticket。
    Cancelled,
}

/// 保存一个目录枚举 ticket 的全部脱敏结果和可选继续游标。
pub(super) struct EnumerationOutcome {
    /// 本 ticket 实际消费的目录项数量；协调器据此扣减唯一共享预算。
    pub(super) entries_consumed: u64,
    /// 当前 ticket 发现的普通子目录，尚未执行根签名判断。
    pub(super) children: Vec<TraversalItem>,
    /// 目录尚未读完时保留游标；读完或打开失败时为空。
    pub(super) continuation: Option<DirectoryCursor>,
    /// `read_dir` 本身被权限拒绝的次数。
    pub(super) permission_denied_count: u64,
    /// 目录项错误、链接或其他无法继续读取项的总跳过数。
    pub(super) skipped_count: u64,
    /// 跳过项中属于链接或 reparse point 的数量。
    pub(super) symlink_skipped_count: u64,
    /// 用户是否在该 ticket 的安全边界请求取消。
    pub(super) cancelled: bool,
}

/// 保存 worker 返回的两类结果；全局状态仍只由协调器提交。
pub(super) enum WorkerOutcome {
    /// 一个路径预检完成。
    Preflight(PreflightOutcome),
    /// 一个有界目录项片段枚举完成。
    Enumerated(Box<EnumerationOutcome>),
    /// worker 在局部任务中异常退出；协调器必须保守终止而不能永久等待。
    Failed,
}

/// 将 worker 结果绑定回轮内确定性 ticket。
pub(super) struct WorkerResult {
    /// 与派发任务相同的轮内顺序。
    pub(super) order: usize,
    /// worker 的无全局副作用结果。
    pub(super) outcome: WorkerOutcome,
}

/// 保存 32 批次共享遍历额度；克隆只用于派发前模拟，不会复制生产预算事实。
#[derive(Clone)]
pub(super) struct TraversalBudget {
    /// 单批最多确认的普通目录数。
    pub(super) max_directories: u64,
    /// 单批最多处理的目录项数。
    pub(super) max_entries: u64,
    /// 总批次数上限。
    pub(super) max_batches: u64,
    /// 当前批次，从一开始计数。
    pub(super) current_batch: u64,
    /// 当前批次已经确认的普通目录数。
    pub(super) directories_in_batch: u64,
    /// 当前批次尚可处理的目录项数。
    pub(super) remaining_entries: u64,
}

impl TraversalBudget {
    /// 从公开扫描选项建立唯一真实遍历预算。
    pub(super) fn new(options: &FullDiscoveryOptions) -> Self {
        Self {
            max_directories: options.max_directories,
            max_entries: options.max_entries,
            max_batches: options.max_traversal_batches,
            current_batch: 1,
            directories_in_batch: 0,
            remaining_entries: options.max_entries,
        }
    }

    /// 确认当前批次仍可接纳目录；必要时只推进批次，不提前消费目录名额。
    pub(super) fn ensure_directory_capacity(&mut self) -> bool {
        if self.max_directories == 0 || self.max_batches == 0 {
            return false;
        }
        if self.directories_in_batch >= self.max_directories && !self.advance_batch() {
            return false;
        }
        true
    }

    /// 为一个已经通过范围过滤且尚未访问的规范化目录扣减额度。
    pub(super) fn claim_directory(&mut self) -> bool {
        if !self.ensure_directory_capacity() {
            return false;
        }
        self.directories_in_batch = self.directories_in_batch.saturating_add(1);
        true
    }

    /// 为一个即将实际读取的目录项扣减额度。
    pub(super) fn claim_entry(&mut self) -> bool {
        if self.max_directories == 0 || self.max_entries == 0 || self.max_batches == 0 {
            return false;
        }
        if self.remaining_entries == 0 && !self.advance_batch() {
            return false;
        }
        self.remaining_entries = self.remaining_entries.saturating_sub(1);
        true
    }

    /// 同时重置目录和目录项余额，保持原 32 个连续批次的耦合语义。
    pub(super) fn advance_batch(&mut self) -> bool {
        if self.current_batch >= self.max_batches {
            return false;
        }
        self.current_batch = self.current_batch.saturating_add(1);
        self.directories_in_batch = 0;
        self.remaining_entries = self.max_entries;
        true
    }
}
