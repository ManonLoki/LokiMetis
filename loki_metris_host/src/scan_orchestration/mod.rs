//! 单 writer 调用的历史扫描编排；不绑定 GUI、IPC 或另一套运行时。

mod client;
mod adapters;
mod support;
mod run;

use loki_metris_core::{LocalScanOutput, LocalScanProgress, SourceClientKind, ScanCancellation,
    ScanKind, ScanStartOrigin, RetentionDays, SourceRootSummary, build_indexed_source_roots};
use crate::local_index::LocalIndex;

/// 读取单一物理来源的有界摘要；全新安装不创建或迁移派生库。
pub async fn sources(directory:&std::path::Path,client:SourceClientKind)->Result<Vec<SourceRootSummary>,String> {
    if client==SourceClientKind::WorkBuddy {return Err("usage_client_unsupported".into())}
    let file=directory.join(loki_metris_core::USAGE_INDEX_FILE_NAME);
    let exists=tokio::task::spawn_blocking(move||file.try_exists()).await.map_err(|_|"usage_read_failed")?
        .map_err(|_|"usage_read_failed")?;
    if !exists {return Ok(Vec::new())}
    load_source_root_summaries_for_parser(directory,client.parser_version(),client.provider().source_environment_label()).await
}

/// 单根重建共享既有强制 generation 流程，旧代只在新代提交后替换。
pub async fn reindex(directory:std::path::PathBuf,client:SourceClientKind,retention:RetentionDays,
    request:loki_metris_core::SourceRootReindexRequest,cancellation:ScanCancellation,
    progress:Box<dyn FnMut(LocalScanProgress)+Send>)->Result<LocalScanOutput,String> {
    match client {
        SourceClientKind::Codex=>run::run_reindex::<client::CodexClient>(directory,retention,None,request,cancellation,progress).await,
        SourceClientKind::ClaudeCode=>run::run_reindex::<client::ClaudeClient>(directory,retention,None,request,cancellation,progress).await,
        SourceClientKind::Grok=>run::run_reindex::<client::GrokClient>(directory,retention,None,request,cancellation,progress).await,
        SourceClientKind::WorkBuddy=>Err("usage_client_unsupported".into()),
    }
}

/// 显式注入保留策略和许可令牌，保留停用根、别名与来源 generation 的历史语义。
pub async fn scan(directory: std::path::PathBuf, client: SourceClientKind,
    retention: RetentionDays, origin: ScanStartOrigin, cancellation: ScanCancellation,
    progress: Box<dyn FnMut(LocalScanProgress) + Send>) -> Result<LocalScanOutput, String>
{
    match client {
        SourceClientKind::Codex => run::run_scan::<client::CodexClient>(directory, retention, None, ScanKind::Quick, origin, cancellation, progress).await,
        SourceClientKind::ClaudeCode => run::run_scan::<client::ClaudeClient>(directory, retention, None, ScanKind::Quick, origin, cancellation, progress).await,
        SourceClientKind::Grok => run::run_scan::<client::GrokClient>(directory, retention, None, ScanKind::Quick, origin, cancellation, progress).await,
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
