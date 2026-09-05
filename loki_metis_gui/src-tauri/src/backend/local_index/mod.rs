//! GUI backend 内的本机 Codex 数据根发现、rollout JSONL 解析与增量索引。
//! 这是整个仓库体量最大的模块；按数据流顺序理解会更容易：
//! `volume_roots`/`discovery` 先在磁盘上找到合法数据根 ->
//! `jsonl`/`claude::jsonl` 把单个文件解析成结构化调用记录 ->
//! `scan` 编排一次完整/快速扫描的进度与取消 ->
//! `rollout_ingest`/`claude::index` 负责调用
//! `loki_metis_core::LocalIndex` 把结果写入并读回 ->
//! `snapshot`（core 侧）组装最终提供给 GUI 视图层的数据形状。
//!
//! 物理存储层（schema、迁移、批次写入、数据根 registry、只读快照）已经
//! 随 SeaORM 迁移一并挪进了 core（见 [`loki_metis_core`]
//! 的 `local_index` 模块），本模块只保留操作系统数据源相关、天然与
//! GUI/Codex/Claude 客户端绑定的发现与解析逻辑，通过 core 暴露的最小
//! 异步 checkpoint 原语落盘。
//!
//! `LocalIndex` 的公开方法是真正的 `async fn`（core 内部用 SeaORM
//! 驱动 SQLite）。扫描编排在 async 任务内直接 `.await` 写库；目录枚举、
//! JSONL 解析与 Codex 全盘发现的 OS worker 仍放进受控 `spawn_blocking`
//! 岛。生产路径不得再使用 `block_on` 桥接 LocalIndex。

mod claude; // Claude Code 专属的发现、扫描与 transcript 解析（内部再拆分子模块）
mod discovery; // Codex rollout 数据根发现（默认路径/环境变量/用户登记/全设备）
mod file_source; // 文件系统层的只读会话文件抽象
mod grok; // Grok Build CLI 专属的发现、扫描与 updates.jsonl 解析
mod jsonl; // Codex rollout JSONL 的流式解析器
pub(crate) mod metadata_discovery;
mod rollout_ingest; // 增量索引单个已验证 rollout 文件，编排解析与 core checkpoint 写入
mod root_registration; // 数据根登记的 GUI 扩展方法：把 Discovered*Root 适配到 core 的存储原语
mod scan; // 扫描编排：协调发现、解析、写入与可取消进度上报
mod user_paths; // Win/macOS 默认数据根由 OS 用户目录 API 解析，不直接信任 HOME/USERPROFILE
mod volume_roots; // 跨平台本地卷枚举，供“全设备发现”使用 // 不读取 JSONL 的元数据候选发现

use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
pub(crate) use claude::CLAUDE_PARSER_VERSION;
pub use claude::{
    ClaudeDiscoveredRoot, ClaudeDiscoveryInputs, ClaudeDiscoveryResult,
    discover_claude_full_device_with_progress, discover_claude_quick, scan_claude_discovered_roots,
};
pub(crate) use claude::{ClaudeRootInspection, ClaudeSignatureBudget, inspect_claude_root};
pub use discovery::{
    DiscoveredRoot, DiscoveryInputs, DiscoveryProgress, DiscoveryResult, FullDiscoveryOptions,
    discover_full_device_with_progress, discover_quick,
};
pub(crate) use discovery::{
    RootInspection, SignatureProbeContext, inspect_root, metadata_is_link_like,
    registered_path_matches_candidate, validate_local_plain_directory,
    walk_ancestors_for_link_component,
};
pub(crate) use file_source::same_opened_file;
pub use grok::{
    GrokDiscoveredRoot, GrokDiscoveryInputs, GrokDiscoveryResult,
    discover_grok_full_device_with_progress, discover_grok_quick, scan_grok_discovered_roots,
};
pub(crate) use grok::{GrokRootInspection, GrokSignatureBudget, inspect_grok_root};
#[cfg(test)]
pub(crate) use grok::{
    PRODUCTION_GROK_SESSION_ENVELOPE_JSONL, SYNTHETIC_GROK_UPDATES_JSONL,
    sum_completed_usage_from_fixture,
};
pub use jsonl::DEFAULT_MAX_JSONL_LINE_BYTES;
#[cfg(test)]
pub use jsonl::PARSER_VERSION;
pub(crate) use root_registration::RegisterDiscoveredRoot;
pub use scan::{
    CancellationToken, ScanConfig, ScanCoordinator, ScanMode, ScanPermit, ScanProgress,
    ScanSummary, scan_discovered_roots,
};
pub(crate) use user_paths::current_user_home;
pub(crate) use volume_roots::{LocalPathStatus, classify_local_path, is_obviously_network_path};
pub use volume_roots::{LocalVolumeRoots, enumerate_local_volume_roots};

// 存储层的稳定类型统一从 core 重新导出，让 GUI 内其余模块继续用
// `crate::backend::local_index::{LocalIndex, LocalError, ...}` 这条既有
// 路径引用，不需要为了这次迁移改写一大批 `use` 声明。
pub use loki_metis_core::{
    DiscoveryMethod, LocalError, LocalErrorKind, LocalIndex, RegisteredRoot, RootRecord,
};

/// scan_runs 记录的开始/结束时间戳；由 Codex 与 Claude 扫描共用。
pub(crate) fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;

