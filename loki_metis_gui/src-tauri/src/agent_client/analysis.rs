//! 本机索引的客户端绑定：parser 身份与索引清理。
//! 只读窗口/调用/统计由 command 直调 view loader，不再经本层转发。

use std::path::{Path, PathBuf};

use crate::backend::local_index::LocalIndex;
use crate::local_view::local_read_error;
use loki_metis_core::{LocalIndexState, ProviderKind, SourceClientKind};
use tauri::async_runtime::spawn_blocking;

/// 绑定某一 Source Client 的本机索引路径与 parser 身份。
pub(crate) struct LocalIndexBinding {
    /// 本机索引所在的 app-data 目录。
    app_data_dir: PathBuf,
    /// 该客户端本机来源的 provider 类型。
    local_provider: ProviderKind,
    /// 该客户端固定的 parser generation。
    parser_version: u32,
    /// 展示用的解析来源标签，随 parser 版本变化。
    parser_source_label: String,
}

impl LocalIndexBinding {
    /// 为 Codex rollout 私有索引绑定。
    pub(crate) fn for_codex(app_data_dir: PathBuf) -> Self {
        let parser_version = SourceClientKind::Codex.parser_version();
        Self {
            app_data_dir,
            local_provider: ProviderKind::RolloutJsonl,
            parser_version,
            parser_source_label: ProviderKind::RolloutJsonl.parser_source_label(parser_version),
        }
    }

    /// 为 Claude Code 私有索引绑定 transcript provider。
    pub(crate) fn for_claude_code(app_data_dir: PathBuf) -> Self {
        let parser_version = SourceClientKind::ClaudeCode.parser_version();
        Self {
            app_data_dir,
            local_provider: ProviderKind::ClaudeTranscriptJsonl,
            parser_version,
            parser_source_label: ProviderKind::ClaudeTranscriptJsonl
                .parser_source_label(parser_version),
        }
    }

    /// 为 Grok Build CLI 私有索引绑定会话用量 provider。
    pub(crate) fn for_grok_build_cli(app_data_dir: PathBuf) -> Self {
        let parser_version = SourceClientKind::GrokBuildCli.parser_version();
        Self {
            app_data_dir,
            local_provider: ProviderKind::GrokSessionJsonl,
            parser_version,
            parser_source_label: ProviderKind::GrokSessionJsonl.parser_source_label(parser_version),
        }
    }

    /// 本产品客户端专属 app-data 目录。
    pub(crate) fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }

    /// 当前 parser generation。
    pub(crate) fn parser_version(&self) -> u32 {
        self.parser_version
    }

    /// 数据根摘要用的环境标签。
    pub(crate) fn source_environment_label(&self) -> &'static str {
        self.local_provider.source_environment_label()
    }

    /// 返回当前客户端固定的 provider 类型与非敏感解析器版本标签。
    pub(crate) fn identity(&self) -> (ProviderKind, String) {
        (self.local_provider, self.parser_source_label.clone())
    }

    /// 按当前绑定的客户端 app-data 目录与 parser generation 打开索引。
    async fn open_index(&self) -> Result<LocalIndex, String> {
        LocalIndex::open_in_app_data(&self.app_data_dir, self.parser_version)
            .await
            .map_err(|_| local_read_error())
    }

    /// 只清理本产品派生索引，保留数据根登记与原始 Agent 文件。
    pub(crate) async fn clear_index(&self) -> Result<(), String> {
        let mut index = self.open_index().await?;
        index.clear_index().await.map_err(|_| local_read_error())
    }

    /// 打开并迁移数据库后，返回当前 parser generation 的安全索引状态。
    pub(crate) async fn index_state(&self) -> Result<LocalIndexState, String> {
        let index = self.open_index().await?;
        index.index_state().await.map_err(|_| local_read_error())
    }

    /// 返回本产品索引文件大小；无法安全取得时返回空。
    pub(crate) async fn index_size_bytes(&self) -> Option<u64> {
        let index_path = self.app_data_dir.join("usage-index.sqlite3");
        spawn_blocking(move || {
            std::fs::metadata(index_path)
                .ok()
                .map(|metadata| metadata.len())
        })
        .await
        .ok()
        .flatten()
    }
}
