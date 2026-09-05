//! 与 GUI 及未来 CLI/MCP 共享的客户端能力端口定义。
//! 该模块不依赖任意 UI 框架、文件系统路径语义、进程管理或协议细节。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    CoverageReport, ScanKind, ScanStartOrigin, SourceClientKind, SourceRootReindexRequest,
    SourceRootSummary,
};

/// 本机扫描能力的 object-safe async 返回值别名。
pub type LocalScanFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 固定客户端槽位的轻量 registry，避免基于 HashMap 的缺失路径风险。
pub struct AgentClientRegistry<T> {
    codex: T,
    claude_code: T,
    grok_build_cli: T,
}

impl<T> AgentClientRegistry<T> {
    /// 构造完整槽位 registry。
    pub const fn new(codex: T, claude_code: T, grok_build_cli: T) -> Self {
        Self {
            codex,
            claude_code,
            grok_build_cli,
        }
    }

    /// 按强类型客户端返回对应插槽；WorkBuddy 不是物理扫描客户端，不占用槽位，
    /// 调用方必须先经 `AgentClientKindDto` 过滤，不应把它传入本 registry。
    pub const fn get(&self, client: SourceClientKind) -> &T {
        match client {
            SourceClientKind::Codex => &self.codex,
            SourceClientKind::ClaudeCode => &self.claude_code,
            SourceClientKind::GrokBuildCli => &self.grok_build_cli,
            SourceClientKind::WorkBuddy => unreachable!(),
        }
    }

    /// 返回可变槽位，供装配阶段替换或测试注入使用；跨 crate 可见，
    /// 因为 `#[cfg(test)]` 门禁不会在依赖此 crate 的下游 crate 测试中生效。
    pub fn get_mut(&mut self, client: SourceClientKind) -> &mut T {
        match client {
            SourceClientKind::Codex => &mut self.codex,
            SourceClientKind::ClaudeCode => &mut self.claude_code,
            SourceClientKind::GrokBuildCli => &mut self.grok_build_cli,
            SourceClientKind::WorkBuddy => unreachable!(),
        }
    }
}

/// 每个本机 Agent 用量索引的固定文件名；不同 Agent 不得共用同一文件。
pub const USAGE_INDEX_FILE_NAME: &str = "usage-index.sqlite3";

/// 返回固定客户端私有 app-data 根目录。
///
/// 这是「一 Agent 一物理用量库」的目录槽位：新增 Agent 必须增加独立
/// `SourceClientKind` 变体与本函数的新分支，不得复用已有 Codex / Claude
/// Code / Grok Build CLI 目录。Codex 保留产品 app-data 根以免迁移旧库。
pub fn source_client_app_data_dir(app_data_dir: &Path, client: SourceClientKind) -> PathBuf {
    match client {
        SourceClientKind::Codex => app_data_dir.to_path_buf(),
        SourceClientKind::ClaudeCode => app_data_dir.join("clients").join("claude-code"),
        SourceClientKind::GrokBuildCli => app_data_dir.join("clients").join("grok-build-cli"),
        SourceClientKind::WorkBuddy => app_data_dir.join("clients").join("workbuddy"),
    }
}

/// 返回指定 Agent 独占的物理用量索引路径。
///
/// 打开、扫描、写入或清理任一 Agent 都必须经过此映射；新 Agent 只能
/// 增加新槽位，不能把数据并入既有文件。
pub fn source_client_usage_index_path(app_data_dir: &Path, client: SourceClientKind) -> PathBuf {
    source_client_app_data_dir(app_data_dir, client).join(USAGE_INDEX_FILE_NAME)
}

/// 表示可取消的扫描取消语义，不包含实现细节。
#[derive(Debug, Clone, Default)]
pub struct ScanCancellation {
    cancelled: Arc<AtomicBool>,
}

impl ScanCancellation {
    /// 构造未取消的信号句柄。
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求扫描在下一个安全边界退出。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// 返回当前是否已请求取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// 定义本机扫描可跨 adapter 共享的阶段进度。
#[derive(Debug, Clone)]
pub enum LocalScanProgress {
    /// 本次发现阶段。
    Discovering {
        /// 已遍历目录数。
        directories_scanned: u64,
        /// 已确认根数。
        roots_discovered: u64,
        /// 本次发现允许遍历的目录上限。
        max_directories: u64,
    },
    /// 发现阶段完成。
    DiscoveryFinished {
        /// 发现阶段总计遍历的目录数。
        directories_scanned: u64,
        /// 发现阶段总计确认的根数。
        roots_discovered: u64,
    },
    /// 索引阶段。
    Indexing {
        /// 当前扫描模式。
        kind: ScanKind,
        /// 当前正在索引的数据根稳定 ID；不包含绝对路径。
        current_root_id: Option<String>,
        /// 已完成索引的根数。
        roots_completed: u64,
        /// 待索引根总数。
        roots_total: u64,
        /// 已扫描文件数。
        files_scanned: u64,
        /// 本轮新增 canonical 调用数。
        calls_added: u64,
    },
}

/// 本机扫描完成后可回传的统一无副作用结果。
#[derive(Debug, Clone)]
pub struct LocalScanOutput {
    /// 已扫描文件数。
    pub files_scanned: u64,
    /// 本轮新增 canonical 调用。
    pub call_count: u64,
    /// 覆盖报告。
    pub coverage: CoverageReport,
    /// 用于回写来源快照的去标识根摘要。
    pub roots: Vec<SourceRootSummary>,
}

/// 定义无副作用索引 adapter 的扫描能力；实现必须在 async 任务内直接
/// `.await` LocalIndex，不得在生产路径调用 `block_on`。
pub trait LocalUsageScanner: Send + Sync {
    /// 异步执行扫描；进度回调须为 `Send`，以便跨 await 边界持有。
    fn execute(
        &self,
        kind: ScanKind,
        origin: ScanStartOrigin,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>>;

    /// 异步重建一个已校验数据根；实现不得先清除旧 generation，只有完整
    /// 解析成功后才能原子切换到新 generation。
    fn reindex_source_root(
        &self,
        request: SourceRootReindexRequest,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证新建取消令牌默认处于未请求取消状态。
    fn cancellation_default_is_not_requested() {
        let cancellation = ScanCancellation::new();
        assert!(!cancellation.is_cancelled());
        cancellation.cancel();
        assert!(cancellation.is_cancelled());
    }

    #[test]
    /// 验证 Claude Code 的应用数据目录与 Codex 后端空间保持隔离。
    fn source_client_app_data_dir_separates_claude_backend_space() {
        let root = Path::new("/tmp/app-data");
        assert_eq!(
            source_client_app_data_dir(root, SourceClientKind::Codex),
            root
        );
        assert_eq!(
            source_client_app_data_dir(root, SourceClientKind::ClaudeCode),
            root.join("clients").join("claude-code")
        );
        assert_eq!(
            source_client_app_data_dir(root, SourceClientKind::GrokBuildCli),
            root.join("clients").join("grok-build-cli")
        );
    }

    /// 验证三个现有 Agent 的物理用量库路径互不相同，且新槽位只能走 match 新分支。
    #[test]
    fn source_client_usage_index_paths_are_exclusive() {
        let root = Path::new("/product-app-data");
        let clients = [
            SourceClientKind::Codex,
            SourceClientKind::ClaudeCode,
            SourceClientKind::GrokBuildCli,
        ];
        let paths: Vec<_> = clients
            .into_iter()
            .map(|client| source_client_usage_index_path(root, client))
            .collect();
        assert_eq!(paths[0], root.join(USAGE_INDEX_FILE_NAME));
        assert_eq!(
            paths[1],
            root.join("clients")
                .join("claude-code")
                .join(USAGE_INDEX_FILE_NAME)
        );
        assert_eq!(
            paths[2],
            root.join("clients")
                .join("grok-build-cli")
                .join(USAGE_INDEX_FILE_NAME)
        );
        let unique = paths.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(
            unique.len(),
            clients.len(),
            "每个 Agent 必须独占一个物理用量库文件"
        );
    }
}
