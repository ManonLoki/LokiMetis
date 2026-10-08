//! 应用拥有的异步索引服务；扫描只读原始会话，写入独立 LokiMetris 派生库。

mod queue;

use crate::backend::local_index::*;
use queue::{Intent, Queue};
use crate::backend::workbuddy::{self, WorkbuddyReadError};
use jiff::tz::TimeZone;
use loki_metris_core::{AgentUsageSnapshot, CoverageReport, LocalRecordsSummary, ProviderKind,
    RetentionDays, RootRecord, ScanStartOrigin, SourceClientKind, TimeStandard, UsageSnapshot,
    build_combined_local_windows_with_standard, build_local_windows_with_standard,
    combine_agent_usage_snapshots, empty_coverage, local_index_scan_policy_with_retention,
    WorkbuddyStatisticsSnapshot, build_workbuddy_only_local_windows, merge_workbuddy_into_local_windows,
    mark_workbuddy_unavailable_in_local_windows,
    merge_coverage_reports, overview_snapshot_start_epoch_ms, source_client_app_data_dir};
use std::{path::PathBuf, sync::{Arc, Mutex}, time::Duration};
use tokio::{runtime::Runtime, sync::oneshot};

/// 一次查询的不可变领域快照；它只作展示，不成为另一份权威业务状态。
#[derive(Clone)]
pub struct UsageData {
    pub records: LocalRecordsSummary,
    pub roots: Vec<RootRecord>,
    pub observed_at_epoch_ms: i64,
    pub workbuddy: Option<WorkbuddyStatisticsSnapshot>,
    pub workbuddy_read_failed: bool,
}

/// 服务拥有唯一 runtime、writer 协调器及关闭门禁；界面只提交有界请求。
pub struct UsageService {
    runtime: Mutex<Option<Runtime>>,
    queue: Arc<Queue>,
    progress: Arc<Mutex<Option<ScanProgress>>>,
}

impl UsageService {
    /// 事件循环前建立运行时；16MiB worker 栈保留历史异步深链所需余量。
    pub fn new() -> Result<Arc<Self>, &'static str> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2)
            .thread_name("loki-metris-usage").thread_stack_size(16 * 1024 * 1024)
            .enable_all().build().map_err(|_| "usage_runtime_unavailable")?;
        #[cfg(target_os = "macos")]
        let directory = current_user_home().map(|home| home.join("Library/Application Support/app-loki-metris/data"));
        #[cfg(target_os = "windows")]
        let directory = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|root| root.join("app-loki-metris/data"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let directory = current_user_home().map(|home| home.join(".local/share/app-loki-metris"));
        let directory = std::env::var_os("LOKI_METRIS_DATA_DIR").map(PathBuf::from).or(directory)
            .filter(|path| path.is_absolute()).ok_or("usage_directory_unavailable")?;
        let queue = Arc::new(Queue::default());
        let worker = queue.clone();
        let progress = Arc::new(Mutex::new(None));
        let worker_progress = progress.clone();
        runtime.spawn(async move {
            worker.run(move |intent, cancellation| {
                let directory = directory.clone();
                let progress = worker_progress.clone();
                async move { Self::load(&directory, intent.client, intent.scan, cancellation, progress).await }
            }).await;
        });
        Ok(Arc::new(Self { runtime: Mutex::new(Some(runtime)), queue, progress }))
    }

    /// 有界邮箱保留一个活动请求和一个最新意图；前台只持有接收端。
    pub fn request(&self, client: Option<SourceClientKind>, scan: bool)
        -> Result<oneshot::Receiver<Result<UsageData, &'static str>>, &'static str>
    {
        self.queue.submit(Intent { client, scan })
    }

    /// 可读进度不含原始文件路径，前台轮询不会访问数据库。
    pub fn progress(&self) -> Option<ScanProgress> {
        self.progress.lock().ok().and_then(|progress| progress.clone())
    }

    /// 取消当前扫描与尚未执行的意图，不触碰原始会话文件。
    pub fn cancel(&self) { self.queue.cancel(); }

    /// 先合作取消并有界等待索引收尾，然后关闭运行时。
    pub fn finish(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            if let Some(runtime) = runtime.take() {
                if !self.queue.stop(Duration::from_secs(10)) {
                    tracing::warn!(event = "usage_shutdown_drain_timeout");
                }
                runtime.shutdown_timeout(Duration::from_secs(1));
            }
        }
    }

    /// 从各客户端一致快照构造概览；联合视图使用 core 的跨 Agent 命名空间规则。
    async fn load(directory: &std::path::Path, selected: Option<SourceClientKind>, scan: bool, cancellation: CancellationToken, progress: Arc<Mutex<Option<ScanProgress>>>) -> Result<UsageData, &'static str> {
        let now = current_epoch_ms();
        let zone = TimeZone::system();
        if let Ok(mut progress) = progress.lock() { *progress = None; }
        if selected == Some(SourceClientKind::WorkBuddy) {
            let homes = workbuddy::resolve_workbuddy_homes();
            let snapshot = workbuddy::read_workbuddy_statistics(&homes, now, TimeStandard::Local).await
                .map_err(|error| if error == WorkbuddyReadError::SourceUnavailable { "usage_workbuddy_absent" } else { "usage_read_failed" })?;
            if cancellation.is_cancelled() { return Err("usage_scan_cancelled") }
            let records = build_workbuddy_only_local_windows(&snapshot.windows, &snapshot.coverage, now, ProviderKind::WorkbuddyProjectJsonl);
            return Ok(UsageData { records, roots: Vec::new(), observed_at_epoch_ms: now,
                workbuddy: Some(snapshot), workbuddy_read_failed: false });
        }
        let clients = selected.map(|client| vec![client]).unwrap_or_else(|| vec![
            SourceClientKind::Codex, SourceClientKind::ClaudeCode, SourceClientKind::GrokBuildCli]);
        let mut inputs = Vec::new();
        let mut roots = Vec::new();
        let mut single = None;
        for client in clients {
            if cancellation.is_cancelled() { return Err("usage_scan_cancelled"); }
            let directory = source_client_app_data_dir(directory, client);
            let mut index = LocalIndex::open_in_app_data(&directory, client.parser_version()).await.map_err(|_| "usage_index_unavailable")?;
            let coverage = if scan {
                scan_client(&mut index, client, &cancellation, now, &zone, progress.clone()).await?
            } else { empty_coverage() };
            if cancellation.is_cancelled() { return Err("usage_scan_cancelled") }
            let cutoff = overview_snapshot_start_epoch_ms(now, &TimeStandard::Local, &zone).map_err(|_| "usage_time_unavailable")?;
            let snapshot = index.usage_view_snapshot_since(cutoff).await.map_err(|_| "usage_read_failed")?;
            roots.extend(snapshot.roots.clone());
            if selected.is_some() {
                single = Some(build_records(&snapshot, &coverage, client, now, &zone)?);
            }
            inputs.push(AgentUsageSnapshot::new(client, snapshot, coverage, Some(format!("parser-v{}", client.parser_version()))));
        }
        let mut workbuddy_read_failed = false;
        let mut workbuddy = None;
        if selected.is_none() {
            match workbuddy::read_workbuddy_usage_snapshot(&workbuddy::resolve_workbuddy_homes(), now, TimeStandard::Local).await {
                Ok(snapshot) => workbuddy = Some(snapshot),
                Err(WorkbuddyReadError::SourceUnavailable) => {},
                Err(WorkbuddyReadError::Read) => workbuddy_read_failed = true,
            }
        }
        if cancellation.is_cancelled() { return Err("usage_scan_cancelled") }
        let records = if let Some(records) = single { records } else {
            match combine_agent_usage_snapshots(inputs) {
                Ok(combined) => {
                    let records = build_combined_local_windows_with_standard(&combined, now, TimeStandard::Local, &zone)
                        .map_err(|_| "usage_aggregate_failed")?;
                    if let Some(snapshot) = &workbuddy {
                        merge_workbuddy_into_local_windows(records, &snapshot.windows, &snapshot.coverage)
                    } else if workbuddy_read_failed { mark_workbuddy_unavailable_in_local_windows(records) }
                    else { records }
                }
                Err(_) => {
                    let snapshot = workbuddy.as_ref().ok_or(if workbuddy_read_failed { "usage_read_failed" } else { "usage_sources_not_configured" })?;
                    build_workbuddy_only_local_windows(&snapshot.windows, &snapshot.coverage, now, ProviderKind::CombinedLocalAgents)
                }
            }
        };
        Ok(UsageData { records, roots, observed_at_epoch_ms: now, workbuddy, workbuddy_read_failed })
    }
}

/// 将物理数据提供者映射为 core 的唯一 Token 核算口径。
fn build_records(snapshot: &UsageSnapshot, coverage: &CoverageReport, client: SourceClientKind, now: i64, zone: &TimeZone)
    -> Result<LocalRecordsSummary, &'static str>
{
    let provider = match client {
        SourceClientKind::Codex => ProviderKind::RolloutJsonl,
        SourceClientKind::ClaudeCode => ProviderKind::ClaudeTranscriptJsonl,
        SourceClientKind::GrokBuildCli => ProviderKind::GrokSessionJsonl,
        SourceClientKind::WorkBuddy => return Err("usage_client_unsupported"),
    };
    build_local_windows_with_standard(&snapshot.canonical, coverage, snapshot.index_state, now,
        provider, Some(&provider.parser_source_label(client.parser_version())), TimeStandard::Local, zone)
        .map_err(|_| "usage_aggregate_failed")
}

/// OS 发现与解析在 blocking 岛；保留历史扫描下界、保留窗口及联合覆盖规则。
async fn scan_client(index: &mut LocalIndex, client: SourceClientKind, cancel: &CancellationToken, now: i64, zone: &TimeZone, progress: Arc<Mutex<Option<ScanProgress>>>)
    -> Result<CoverageReport, &'static str>
{
    let policy = local_index_scan_policy_with_retention(ScanStartOrigin::ExplicitUser,
        index.has_current_parser_usage().await.map_err(|_| "usage_read_failed")?, now,
        RetentionDays::default(), TimeStandard::Local, zone);
    let config = ScanConfig { scan_since_epoch_ms: policy.scan_since_epoch_ms,
        ingest_since_epoch_ms: policy.retention_since_epoch_ms, started_at_epoch_ms: Some(now), ..Default::default() };
    let known = index.known_roots().await.map_err(|_| "usage_read_failed")?;
    let token = cancel.clone();
    let (discovery, scan) = match client {
        SourceClientKind::Codex => {
            let inputs = DiscoveryInputs::from_process_environment(known);
            let discovery = tokio::task::spawn_blocking(move || discover_quick(&inputs, &token)).await.map_err(|_| "usage_discovery_failed")?;
            let summary = scan_discovered_roots(index, &discovery.roots, config, cancel, |value| { if let Ok(mut slot) = progress.lock() { *slot = Some(value); } }).await.map_err(|_| "usage_scan_failed")?;
            (discovery.coverage, summary.coverage)
        }
        SourceClientKind::ClaudeCode => {
            let inputs = ClaudeDiscoveryInputs::from_process_environment(known);
            let discovery = tokio::task::spawn_blocking(move || discover_claude_quick(&inputs, &token)).await.map_err(|_| "usage_discovery_failed")?;
            let summary = scan_claude_discovered_roots(index, &discovery.roots, config, cancel, |value| { if let Ok(mut slot) = progress.lock() { *slot = Some(value); } }).await.map_err(|_| "usage_scan_failed")?;
            (discovery.coverage, summary.coverage)
        }
        SourceClientKind::GrokBuildCli => {
            let inputs = GrokDiscoveryInputs::from_process_environment(known);
            let discovery = tokio::task::spawn_blocking(move || discover_grok_quick(&inputs, &token)).await.map_err(|_| "usage_discovery_failed")?;
            let summary = scan_grok_discovered_roots(index, &discovery.roots, config, cancel, |value| { if let Ok(mut slot) = progress.lock() { *slot = Some(value); } }).await.map_err(|_| "usage_scan_failed")?;
            (discovery.coverage, summary.coverage)
        }
        SourceClientKind::WorkBuddy => return Err("usage_client_unsupported"),
    };
    Ok(merge_coverage_reports(discovery, &scan))
}
