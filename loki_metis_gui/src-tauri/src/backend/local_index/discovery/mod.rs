//! 在 GUI backend 内实现默认、环境、自定义与用户主动全设备的数据根发现；
//! 具体流程按职责收在 [`full_device`]、[`quick`]、[`inspection`] 三个子模块内，
//! 本文件只保留跨流程共享的公开类型。路径键与稳定 ID 生成
//! （原 `path_keys` 子模块）随存储层一并迁入了
//! `loki_metis_core`，供未来其他 adapter 复用。

mod full_device;
mod inspection;
mod quick;

use std::path::PathBuf;

use loki_metis_core::CoverageReport;

use super::{DiscoveryMethod, RegisteredRoot};

pub use full_device::discover_full_device_with_progress;
pub(crate) use inspection::{
    RootInspection, SignatureProbeContext, inspect_root, metadata_is_link_like,
    registered_path_matches_candidate, validate_local_plain_directory,
    walk_ancestors_for_link_component,
};
pub(crate) use loki_metis_core::{path_key, stable_id};
pub use quick::discover_quick;

/// 汇总快速发现所需的显式输入，测试可完全避开真实用户环境。
#[derive(Debug, Clone, Default)]
pub struct DiscoveryInputs {
    /// 当前用户主目录；存在时只检查其 `.codex` 直接候选。
    pub home_dir: Option<PathBuf>,
    /// 当前进程生效的 `CODEX_HOME`；不会读取其中的认证文件。
    pub codex_home: Option<PathBuf>,
    /// 用户已经明确登记的数据根。
    pub registered_roots: Vec<RegisteredRoot>,
}

impl DiscoveryInputs {
    /// 从当前进程环境构造快速发现输入，不执行目录遍历。
    pub fn from_process_environment(registered_roots: Vec<RegisteredRoot>) -> Self {
        let home_dir = super::current_user_home();
        let codex_home = std::env::var_os("CODEX_HOME").map(PathBuf::from);
        Self {
            home_dir,
            codex_home,
            registered_roots,
        }
    }
}

/// 描述一个通过只读结构签名确认的数据根。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredRoot {
    /// adapter 内部用于后续只读扫描的路径。
    pub path: PathBuf,
    /// 内容无关、按规范化访问位置生成的稳定根 ID。
    pub root_id: String,
    /// 默认供 GUI 展示的安全别名。
    pub alias: String,
    /// 根来自默认、环境、用户登记或主动全设备发现。
    pub discovery_method: DiscoveryMethod,
    // has_sessions/sessions_inspection_complete 这一对字段（以及下面归档区域
    // 的同款配对）体现了本文件反复使用的"三态"表达法：单独一个 bool
    // 只能表示"有/没有"，无法表示"不知道"；配上 `_inspection_complete`
    // 标志后就能区分「确认没有」（complete=true, has=false）和
    // 「因权限/I/O 问题没能确认」（complete=false）——后者绝不能被当作
    // "确认没有"处理，否则会把一个可能有效的数据根误判为无效。
    /// 根是否包含可读取且非符号链接的活动会话目录。
    pub has_sessions: bool,
    /// 是否已明确确认活动会话区域存在或不存在；I/O 不确定时为 false。
    pub sessions_inspection_complete: bool,
    /// 根是否包含可读取且非符号链接的归档会话目录。
    pub has_archived_sessions: bool,
    /// 是否已明确确认归档会话区域存在或不存在；I/O 不确定时为 false。
    pub archived_sessions_inspection_complete: bool,
}

/// 返回发现候选与不包含绝对路径的覆盖统计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResult {
    /// 已确认包含活动或归档会话目录的数据根。
    pub roots: Vec<DiscoveredRoot>,
    /// 已完整检查且确认不再符合 Codex rollout 签名的历史根精确 ID。
    pub confirmed_invalid_root_ids: Vec<String>,
    /// 因权限、I/O 或预算无法重新确认的历史根精确 ID。
    pub unconfirmed_root_ids: Vec<String>,
    /// 使用 core 稳定类型表达的覆盖结论。
    pub coverage: CoverageReport,
    /// 主动发现实际读取过目录项的目录数量。
    pub directories_scanned: u64,
    /// 因符号链接策略跳过的路径数量。
    pub symlink_skipped_count: u64,
    /// 因默认网络卷排除策略跳过的路径数量。
    pub network_skipped_count: u64,
}

/// 描述主动发现阶段可安全发布的轻量进度，不包含当前路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryProgress {
    /// 已实际检查的目录数量。
    pub directories_scanned: u64,
    /// 到当前进度点已确认的数据根数量。
    pub roots_discovered: u64,
}

/// 配置用户主动发起的全设备发现；调用方只应传当前用户获准访问的起点。
#[derive(Debug, Clone)]
pub struct FullDiscoveryOptions {
    /// 本次主动遍历的一个或多个显式起点。
    pub search_roots: Vec<PathBuf>,
    /// 调用方根据平台卷信息明确排除的路径前缀。
    pub excluded_roots: Vec<PathBuf>,
    /// 兼容测试注入的目录预算；生产默认不限数量。
    pub max_directories: u64,
    /// 兼容测试注入的目录项预算；生产默认不限数量。
    pub max_entries: u64,
    /// 兼容测试注入的遍历批次；生产默认单一不限量批次。
    pub max_traversal_batches: u64,
    /// 未知候选结构签名在整个任务中最多递归检查的目录数。
    pub max_signature_directories: u64,
    /// 未知候选结构签名在整个任务中最多处理的目录项数量。
    pub max_signature_entries: u64,
    /// 未知候选结构签名在整个任务中最多探测的 rollout 文件数。
    pub max_signature_files: u64,
    /// 未知候选结构签名在整个任务中最多读取的文件前缀字节数。
    pub max_signature_bytes: u64,
    /// 默认关闭；开启前调用方必须确认目标不是网络卷。
    pub allow_network_like_paths: bool,
    /// 卷枚举阶段已经明确跳过的网络卷数量。
    pub preflight_network_skipped_count: u64,
    /// 卷枚举阶段因未知、离线或失败而跳过的卷数量。
    pub preflight_other_skipped_count: u64,
}

impl Default for FullDiscoveryOptions {
    /// 返回保守空配置；空起点不会隐式遍历磁盘。
    fn default() -> Self {
        Self {
            search_roots: Vec::new(),
            excluded_roots: Vec::new(),
            max_directories: u64::MAX,
            max_entries: u64::MAX,
            max_traversal_batches: 1,
            max_signature_directories: u64::MAX,
            max_signature_entries: u64::MAX,
            max_signature_files: u64::MAX,
            max_signature_bytes: u64::MAX,
            allow_network_like_paths: false,
            preflight_network_skipped_count: 0,
            preflight_other_skipped_count: 0,
        }
    }
}
