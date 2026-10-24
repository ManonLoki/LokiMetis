//! 统一 Codex / Claude / Grok 三条本机索引扫描管线的差异点；WorkBuddy 独立读取。
//!
//! 三条客户端管线（打开索引 -> 快速重验证已知根 -> 可选全设备发现 ->
//! 合并发现结果 -> 登记根 -> 增量扫描 -> 合并覆盖 -> 读回来源摘要）结构
//! 完全相同，只在“怎么发现”“怎么登记”“怎么扫描”“用哪个 parser
//! generation/provider”上不同；这个 trait 把这几个差异点收拢成关联类型
//! 与方法，交给 [`super::run_scan`] 驱动同一份骨架。

use loki_metris_core::{DiscoveredRootIdentity, DiscoveryOutcome, ProviderKind, SourceClientKind};

use crate::local_index::{
    CancellationToken, DiscoveryProgress, LocalError, LocalIndex, RegisteredRoot, ScanConfig,
    ScanProgress, ScanSummary,
};

/// 单一客户端（Codex、Claude 或 Grok）的本机索引扫描能力集合。
pub(super) trait ScanClient {
    /// 该客户端结构签名确认的发现根类型。
    type DiscoveredRoot: DiscoveredRootIdentity + Clone + Send + 'static;

    /// 该管线对应的物理 Agent；parser generation 与 provider 均由 core 从它推导。
    const CLIENT: SourceClientKind;

    /// 当前客户端固定的 parser generation，唯一来源为 core。
    fn parser_version() -> u32 {
        Self::CLIENT.parser_version()
    }
    /// 来源摘要读取用的 provider 类型，唯一来源为 core。
    fn provider_kind() -> ProviderKind {
        Self::CLIENT.provider()
    }
    /// 全设备发现进度上限，各客户端的预算换算方式见各自实现。
    fn full_device_max_directories() -> u64;

    /// 把本机索引层错误映射为面向用户的稳定文案；保留各客户端原有的
    /// 通用提示或错误类别映射，不在统一编排骨架时改变提示粒度。
    fn map_local_error(error: LocalError) -> String;

    /// 阻塞岛内对已知根做一次快速结构签名重验证。
    fn discover_quick(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot>;

    /// 只重验证调用方传入的登记根，不追加默认目录或进程环境候选。
    fn discover_registered(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot>;

    /// 阻塞岛内做一次用户主动发起的全设备发现，通过回调上报进度。
    fn discover_full_device_with_progress<F>(
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot>
    where
        F: FnMut(DiscoveryProgress);

    /// 把已确认数据根登记到该客户端的存储层。
    fn register_root(
        index: &mut LocalIndex,
        root: &Self::DiscoveredRoot,
    ) -> impl std::future::Future<Output = Result<(), LocalError>> + Send;

    /// 增量扫描一批已确认数据根。
    fn scan_discovered_roots<F>(
        index: &mut LocalIndex,
        roots: &[Self::DiscoveredRoot],
        config: ScanConfig,
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> impl std::future::Future<Output = Result<ScanSummary, LocalError>> + Send
    where
        F: FnMut(ScanProgress) + Send;
}

/// Codex rollout 客户端。
pub(super) struct CodexClient;

impl ScanClient for CodexClient {
    /// Codex rollout 发现产出的根类型。
    type DiscoveredRoot = crate::local_index::DiscoveredRoot;

    /// 对应的物理 Agent。
    const CLIENT: SourceClientKind = SourceClientKind::Codex;

    /// 返回 Codex 全设备发现的进度目录数上限。
    fn full_device_max_directories() -> u64 {
        super::support::codex_full_device_progress_max_directories()
    }

    /// 折叠为 Codex 沿用的通用扫描失败文案。
    fn map_local_error(_error: LocalError) -> String {
        super::support::local_scan_task_error_message()
    }

    /// 从进程继承的环境快速重验证 Codex 已知根。
    fn discover_quick(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot> {
        crate::local_index::discover_quick(
            &crate::local_index::DiscoveryInputs::from_process_environment(
                registered_roots,
            ),
            cancellation,
        )
    }

    /// 只用显式登记输入重验证 Codex 根，避免单根操作触碰其他候选。
    fn discover_registered(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot> {
        crate::local_index::discover_quick(
            &crate::local_index::DiscoveryInputs {
                home_dir: None,
                codex_home: None,
                registered_roots,
            },
            cancellation,
        )
    }

    /// 执行 Codex 全设备发现并通过回调上报进度。
    fn discover_full_device_with_progress<F>(
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot>
    where
        F: FnMut(DiscoveryProgress),
    {
        crate::local_index::discover_full_device_with_progress(
            &super::support::full_device_options(),
            cancellation,
            on_progress,
        )
    }

    /// 把已确认的 Codex 根登记进本机索引。
    async fn register_root(
        index: &mut LocalIndex,
        root: &Self::DiscoveredRoot,
    ) -> Result<(), LocalError> {
        use crate::local_index::RegisterDiscoveredRoot;
        index.register_root(root).await
    }

    /// 增量扫描一批已确认的 Codex 根。
    async fn scan_discovered_roots<F>(
        index: &mut LocalIndex,
        roots: &[Self::DiscoveredRoot],
        config: ScanConfig,
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> Result<ScanSummary, LocalError>
    where
        F: FnMut(ScanProgress) + Send,
    {
        crate::local_index::scan_discovered_roots(
            index,
            roots,
            config,
            cancellation,
            on_progress,
        )
        .await
    }
}

/// Claude Code transcript 客户端。
pub(super) struct ClaudeClient;

impl ScanClient for ClaudeClient {
    /// Claude transcript 发现产出的根类型。
    type DiscoveredRoot = crate::local_index::ClaudeDiscoveredRoot;

    /// 对应的物理 Agent。
    const CLIENT: SourceClientKind = SourceClientKind::ClaudeCode;

    /// 返回 Claude 全设备发现的进度目录数上限。
    fn full_device_max_directories() -> u64 {
        super::support::claude_full_device_progress_max_directories()
    }

    /// 保留 Claude 原有的按错误类别细分文案，不折叠为通用提示。
    fn map_local_error(error: LocalError) -> String {
        super::support::local_scan_error_message_from_local(error)
    }

    /// 从进程继承的环境快速重验证 Claude 已知根。
    fn discover_quick(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot> {
        crate::local_index::discover_claude_quick(
            &crate::local_index::ClaudeDiscoveryInputs::from_process_environment(
                registered_roots,
            ),
            cancellation,
        )
    }

    /// 只用显式登记输入重验证 Claude 根，避免单根操作触碰其他候选。
    fn discover_registered(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot> {
        crate::local_index::discover_claude_quick(
            &crate::local_index::ClaudeDiscoveryInputs {
                home_dir: None,
                claude_config_dir: None,
                registered_roots,
            },
            cancellation,
        )
    }

    /// 执行 Claude 全设备发现并通过回调上报进度。
    fn discover_full_device_with_progress<F>(
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot>
    where
        F: FnMut(DiscoveryProgress),
    {
        crate::local_index::discover_claude_full_device_with_progress(
            &super::support::full_device_options(),
            cancellation,
            on_progress,
        )
    }

    /// 把已确认的 Claude 根登记进本机索引。
    async fn register_root(
        index: &mut LocalIndex,
        root: &Self::DiscoveredRoot,
    ) -> Result<(), LocalError> {
        use crate::local_index::RegisterDiscoveredRoot;
        index.register_claude_root(root).await
    }

    /// 增量扫描一批已确认的 Claude 根。
    async fn scan_discovered_roots<F>(
        index: &mut LocalIndex,
        roots: &[Self::DiscoveredRoot],
        config: ScanConfig,
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> Result<ScanSummary, LocalError>
    where
        F: FnMut(ScanProgress) + Send,
    {
        crate::local_index::scan_claude_discovered_roots(
            index,
            roots,
            config,
            cancellation,
            on_progress,
        )
        .await
    }
}

/// Grok Build CLI 会话用量客户端。
pub(super) struct GrokClient;

impl ScanClient for GrokClient {
    /// Grok 会话发现产出的根类型。
    type DiscoveredRoot = crate::local_index::GrokDiscoveredRoot;

    /// 对应的物理 Agent。
    const CLIENT: SourceClientKind = SourceClientKind::Grok;

    /// 返回 Grok 全设备发现的进度目录数上限。
    fn full_device_max_directories() -> u64 {
        super::support::claude_full_device_progress_max_directories()
    }

    /// 保留按错误类别细分的扫描失败文案。
    fn map_local_error(error: LocalError) -> String {
        super::support::local_scan_error_message_from_local(error)
    }

    /// 从进程继承的环境快速重验证 Grok 已知根。
    fn discover_quick(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot> {
        crate::local_index::discover_grok_quick(
            &crate::local_index::GrokDiscoveryInputs::from_process_environment(
                registered_roots,
            ),
            cancellation,
        )
    }

    /// 只用显式登记输入重验证 Grok 根，避免单根操作触碰其他候选。
    fn discover_registered(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot> {
        crate::local_index::discover_grok_quick(
            &crate::local_index::GrokDiscoveryInputs {
                home_dir: None,
                grok_home: None,
                registered_roots,
            },
            cancellation,
        )
    }

    /// 执行 Grok 全设备发现并通过回调上报进度。
    fn discover_full_device_with_progress<F>(
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> DiscoveryOutcome<Self::DiscoveredRoot>
    where
        F: FnMut(DiscoveryProgress),
    {
        crate::local_index::discover_grok_full_device_with_progress(
            &super::support::full_device_options(),
            cancellation,
            on_progress,
        )
    }

    /// 把已确认的 Grok 根登记进本机索引。
    async fn register_root(
        index: &mut LocalIndex,
        root: &Self::DiscoveredRoot,
    ) -> Result<(), LocalError> {
        use crate::local_index::RegisterDiscoveredRoot;
        index.register_grok_root(root).await
    }

    /// 增量扫描一批已确认的 Grok 根。
    async fn scan_discovered_roots<F>(
        index: &mut LocalIndex,
        roots: &[Self::DiscoveredRoot],
        config: ScanConfig,
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> Result<ScanSummary, LocalError>
    where
        F: FnMut(ScanProgress) + Send,
    {
        crate::local_index::scan_grok_discovered_roots(
            index,
            roots,
            config,
            cancellation,
            on_progress,
        )
        .await
    }
}
