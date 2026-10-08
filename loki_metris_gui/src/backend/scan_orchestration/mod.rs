//! 单 writer 调用的历史扫描编排；不绑定 GUI、IPC 或另一套运行时。

mod client;
mod adapters;
mod support;
mod run;

use loki_metris_core::{LocalScanOutput, LocalScanProgress, SourceClientKind, ScanCancellation,
    ScanKind, ScanStartOrigin, RetentionDays, SourceRootSummary, build_indexed_source_roots};
use crate::backend::local_index::LocalIndex;

/// 显式注入保留策略和许可令牌，保留停用根、别名与来源 generation 的历史语义。
pub(crate) async fn scan(directory: std::path::PathBuf, client: SourceClientKind,
    retention: RetentionDays, origin: ScanStartOrigin, cancellation: ScanCancellation,
    progress: Box<dyn FnMut(LocalScanProgress) + Send>) -> Result<LocalScanOutput, String>
{
    match client {
        SourceClientKind::Codex => run::run_scan::<client::CodexClient>(directory, retention, None, ScanKind::Quick, origin, cancellation, progress).await,
        SourceClientKind::ClaudeCode => run::run_scan::<client::ClaudeClient>(directory, retention, None, ScanKind::Quick, origin, cancellation, progress).await,
        SourceClientKind::GrokBuildCli => run::run_scan::<client::GrokClient>(directory, retention, None, ScanKind::Quick, origin, cancellation, progress).await,
        SourceClientKind::WorkBuddy => Err("usage_client_unsupported".into()),
    }
}

/// 来源摘要只读取有界 SQL 计数，不为根列表装载完整调用。
async fn load_source_root_summaries_for_parser(directory: &std::path::Path, parser: u32,
    environment: &'static str) -> Result<Vec<SourceRootSummary>, String>
{
    let index = LocalIndex::open_read_only_in_app_data(directory, parser).await.map_err(|_| "usage_read_failed")?;
    let records = index.root_usage_summary_records().await.map_err(|_| "usage_read_failed")?;
    Ok(build_indexed_source_roots(&records, environment))
}
