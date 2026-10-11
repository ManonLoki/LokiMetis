//! 只读 loader 直接返回 core 页面模型；CPU 聚合在 blocking 岛，绝不初始化或迁移库。

use super::{ServiceContext, UsageData};
use crate::backend::{local_index::current_epoch_ms, workbuddy::{self, WorkbuddyReadError}};
use jiff::tz::TimeZone;
use loki_metris_core::*;
use std::{collections::BTreeMap, sync::Arc};

/// 查询输入只包含业务筛选；All 不拥有另一个物理存储。
#[derive(Clone)]
pub enum UsageQuery {
    Overview(Option<SourceClientKind>),
    Statistics(SourceClientKind, LocalUsageWindow, UsageDimension),
    Chart(Option<SourceClientKind>, LocalUsageWindow, UsageChartDimension),
    Calls(Option<SourceClientKind>, UsageCallsQuery),
    Sources(SourceClientKind),
}

/// 类型化页面响应避免从概览聚合值反推调用或图表。
pub enum UsageResponse {
    Overview(UsageData),
    Statistics(UsageStatisticsPage),
    Chart(UsageChartPage),
    Calls(UsageCallsPage),
    Sources(Vec<SourceRootSummary>),
    Workbuddy(WorkbuddyStatisticsSnapshot),
    WorkbuddyDetails(UsageStatisticsPage, WorkbuddyModelUsageWindow),
}

/// 为所选页面取得同一次时间观测和不可变读取快照。
pub(super) async fn load(context: Arc<ServiceContext>, query: UsageQuery, cancellation: ScanCancellation)
    -> Result<UsageResponse, &'static str>
{
    if matches!(query, UsageQuery::Overview(None)) {
        return super::today::load(context, current_epoch_ms(), TimeZone::system(), cancellation).await
            .map(|(data,_)| UsageResponse::Overview(data));
    }
    let (enabled_clients, workbuddy_enabled) = context.agents.dashboard_selection().await?;
    let requested_client = match &query {
        UsageQuery::Overview(client) | UsageQuery::Chart(client, ..) | UsageQuery::Calls(client, ..) => *client,
        UsageQuery::Statistics(client, ..) | UsageQuery::Sources(client) => Some(*client),
    };
    // 参考宿主允许显式读取停用物理索引；WorkBuddy 停用则完全不探测本机文件。
    if requested_client == Some(SourceClientKind::WorkBuddy) && !workbuddy_enabled { return Err("usage_agents_disabled") }
    if requested_client.is_none() && enabled_clients.is_empty() && !workbuddy_enabled { return Err("usage_agents_disabled") }
    if let UsageQuery::Sources(client)=&query {
        let client=*client;
        let roots=if client==SourceClientKind::WorkBuddy {
            tokio::task::spawn_blocking(super::sources::workbuddy_sources).await.map_err(|_|"usage_read_failed")?
        } else {
            crate::backend::scan_orchestration::sources(&source_client_app_data_dir(&context.directory,client),client).await.map_err(|_|"usage_read_failed")?
        };
        check_cancel(&cancellation)?;
        return Ok(UsageResponse::Sources(roots));
    }
    let selected = match &query {
        UsageQuery::Overview(client) | UsageQuery::Chart(client, ..) | UsageQuery::Calls(client, ..) => *client,
        UsageQuery::Statistics(client, ..)|UsageQuery::Sources(client) => Some(*client),
    };
    let now = current_epoch_ms();
    let zone = TimeZone::system();
    if selected == Some(SourceClientKind::WorkBuddy) {
        if matches!(query, UsageQuery::Calls(..)) { return Err("usage_client_unsupported") }
        if let UsageQuery::Statistics(_, window, dimension) = query {
            let details = workbuddy::read_workbuddy_usage_details(&workbuddy::resolve_workbuddy_homes(), window, dimension, now, TimeStandard::Local).await.map_err(workbuddy_error)?;
            check_cancel(&cancellation)?;
            return Ok(UsageResponse::WorkbuddyDetails(details.statistics, details.model_usage));
        }
        let snapshot = workbuddy::read_workbuddy_statistics(&workbuddy::resolve_workbuddy_homes(), now, TimeStandard::Local).await
            .map_err(workbuddy_error)?;
        check_cancel(&cancellation)?;
        return if matches!(query, UsageQuery::Overview(..)) {
            let records = build_workbuddy_only_local_windows(&snapshot.windows, &snapshot.coverage, now, ProviderKind::WorkbuddyProjectJsonl);
            Ok(UsageResponse::Overview(UsageData { records, workbuddy: Some(snapshot) }))
        } else { Ok(UsageResponse::Workbuddy(snapshot)) };
    }
    if let Some(client) = selected {
        // 概览每 10 秒由页面刷新；索引与覆盖未变且仍是同一民用日时直接复用窗口汇总。
        let day = if matches!(query, UsageQuery::Overview(_)) { UsageDay::capture(now, &zone) } else { None };
        let key = match &day {
            Some(day) => super::today_cache::key_for(&context, super::today_cache::Scope::Single, &[client], day).await,
            None => None,
        };
        if let Some(cached) = key.as_ref().and_then(|key| context.today_cache.lookup(key)) {
            let mut records = cached.records.clone();
            super::today_cache::restamp(&mut records, now);
            return Ok(UsageResponse::Overview(UsageData { records, workbuddy: None }));
        }
        let snapshot = read_snapshot(&context.directory, client, now, &zone).await?;
        let coverage = coverage(&context, client);
        check_cancel(&cancellation)?;
        let response = tokio::task::spawn_blocking(move || build_single(query, snapshot, coverage, client, now, &zone))
            .await.map_err(|_| "usage_aggregate_failed")??;
        // 读取期间索引文件未变化才保存，避免把并发写入中途的数据当作稳定结果。
        if let (Some(key), Some(day), UsageResponse::Overview(data)) = (key, day, &response) {
            let after = super::today_cache::key_for(&context, super::today_cache::Scope::Single, &[client], &day).await;
            if after.as_ref() == Some(&key) {
                context.today_cache.store(key, Vec::new(), false, data.records.clone());
            }
        }
        return Ok(response);
    }
    let mut jobs = tokio::task::JoinSet::new();
    for client in enabled_clients {
        let context = context.clone();
        let zone = zone.clone();
        jobs.spawn(async move {
            let snapshot = read_snapshot(&context.directory, client, now, &zone).await?;
            let report = coverage(&context, client);
            Ok::<_, &'static str>(AgentUsageSnapshot::new(client, snapshot, report,
                Some(provider(client).parser_source_label(client.parser_version()))))
        });
    }
    let mut inputs = Vec::new();
    while let Some(result) = jobs.join_next().await { inputs.push(result.map_err(|_| "usage_read_failed")??); }
    check_cancel(&cancellation)?;
    let mut workbuddy = None;
    let mut failed = false;
    let mut absent = false;
    if workbuddy_enabled && matches!(query, UsageQuery::Overview(..)) {
        match workbuddy::read_workbuddy_usage_snapshot(&workbuddy::resolve_workbuddy_homes(), now, TimeStandard::Local).await {
            Ok(snapshot) => workbuddy = Some(snapshot),
            Err(WorkbuddyReadError::SourceUnavailable) => absent = true,
            Err(WorkbuddyReadError::Read) => failed = true,
        }
    }
    check_cancel(&cancellation)?;
    tokio::task::spawn_blocking(move || {
        let combined = combine_agent_usage_snapshots(inputs);
        match query {
            UsageQuery::Overview(..) => {
                let records = match combined {
                    Ok(combined) => {
                        let records = build_combined_local_windows_with_standard(&combined, now, TimeStandard::Local, &zone)
                            .map_err(|_| "usage_aggregate_failed")?;
                        if let Some(snapshot) = &workbuddy { merge_workbuddy_into_local_windows(records, &snapshot.windows, &snapshot.coverage) }
                        else if failed { mark_workbuddy_unavailable_in_local_windows(records) } else { records }
                    }
                    Err(UsageViewError::NoEnabledAgents | UsageViewError::NoConfiguredDataSources) => {
                        let snapshot = workbuddy.as_ref().ok_or(if failed { "usage_read_failed" } else if absent { "usage_workbuddy_absent" } else { "usage_sources_not_configured" })?;
                        build_workbuddy_only_local_windows(&snapshot.windows, &snapshot.coverage, now, ProviderKind::CombinedLocalAgents)
                    }
                    Err(_) => return Err("usage_aggregate_failed"),
                };
                Ok(UsageResponse::Overview(UsageData { records, workbuddy }))
            }
            UsageQuery::Chart(_, window, dimension) => Ok(UsageResponse::Chart(build_combined_usage_chart_with_standard(
                &combined.map_err(combined_error)?, window, dimension, now, TimeStandard::Local, &zone).map_err(|_| "usage_aggregate_failed")?)),
            UsageQuery::Calls(_, query) => Ok(UsageResponse::Calls(build_combined_usage_calls_page(
                &combined.map_err(combined_error)?, &query, now).map_err(|_| "usage_query_failed")?)),
            UsageQuery::Statistics(..)|UsageQuery::Sources(..) => Err("usage_client_unsupported"),
        }
    }).await.map_err(|_| "usage_aggregate_failed")?
}

/// 用户未配置来源与装配故障保持不同分类，不能用空页面掩盖失败。
fn combined_error(error:UsageViewError)->&'static str {
    match error {UsageViewError::NoEnabledAgents=>"usage_agents_disabled",UsageViewError::NoConfiguredDataSources=>"usage_sources_not_configured",_=>"usage_aggregate_failed"}
}

/// 未有派生文件是未扫描；其余打开/读取失败不能伪装可信空值。
pub(super) async fn read_snapshot(directory: &std::path::Path, client: SourceClientKind, now: i64, zone: &TimeZone) -> Result<UsageSnapshot, &'static str> {
    let directory = source_client_app_data_dir(directory, client);
    let file = directory.join(USAGE_INDEX_FILE_NAME);
    let exists = tokio::task::spawn_blocking(move || file.try_exists()).await.map_err(|_| "usage_read_failed")?
        .map_err(|_| "usage_read_failed")?;
    if !exists { return Ok(UsageSnapshot { canonical: canonicalize_usage_calls(Vec::new()), index_state: LocalIndexState::NotScanned, read_ready: false, roots: Vec::new() }) }
    let mut index = LocalIndex::open_read_only_in_app_data(&directory, client.parser_version()).await.map_err(|_| "usage_index_unavailable")?;
    let cutoff = overview_snapshot_start_epoch_ms(now, &TimeStandard::Local, zone).map_err(|_| "usage_time_unavailable")?;
    index.usage_view_snapshot_since(cutoff).await.map_err(|_| "usage_read_failed")
}

/// 物理页面直接使用 core 的窗口、分组和游标合同。
fn build_single(query: UsageQuery, snapshot: UsageSnapshot, coverage: CoverageReport,
    client: SourceClientKind, now: i64, zone: &TimeZone) -> Result<UsageResponse, &'static str>
{
    let coverage = snapshot.effective_coverage(coverage);
    let provider = provider(client);
    let source = provider.parser_source_label(client.parser_version());
    let aliases: BTreeMap<_, _> = snapshot.roots.iter().map(|root| (root.root_id.clone(), root.alias.clone())).collect();
    match query {
        UsageQuery::Overview(..) => {
            let records = build_local_windows_with_standard(&snapshot.canonical, &coverage, snapshot.index_state, now,
                provider, Some(&source), TimeStandard::Local, zone).map_err(|_| "usage_aggregate_failed")?;
            Ok(UsageResponse::Overview(UsageData { records, workbuddy: None }))
        }
        UsageQuery::Statistics(_, window, dimension) => Ok(UsageResponse::Statistics(build_usage_statistics_with_standard(
            &snapshot.canonical, &aliases, snapshot.index_state, &coverage, window, dimension, now,
            provider, Some(&source), TimeStandard::Local, zone).map_err(|_| "usage_aggregate_failed")?)),
        UsageQuery::Chart(_, window, dimension) => Ok(UsageResponse::Chart(build_usage_chart_with_standard(
            &snapshot.canonical, &aliases, snapshot.index_state, &coverage, window, dimension, now,
            provider, Some(&source), TimeStandard::Local, zone).map_err(|_| "usage_aggregate_failed")?)),
        UsageQuery::Calls(_, query) => Ok(UsageResponse::Calls(build_usage_calls_page(&snapshot.canonical.calls, snapshot.index_state,
            &aliases, &query, now, client, provider, Some(&source)).map_err(|_| "usage_query_failed")?)),
        UsageQuery::Sources(..)=>Err("usage_client_unsupported"),
    }
}

/// 覆盖状态来自上一轮已提交扫描，普通读请求不会把它清空。
pub(super) fn coverage(context: &ServiceContext, client: SourceClientKind) -> CoverageReport {
    context.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&client).cloned().unwrap_or_else(empty_coverage)
}

/// 物理存储只对应三个支持的 SQLite Agent；WorkBuddy 始终走独立 JSONL 读模型。
pub(super) fn physical_clients() -> [SourceClientKind; 3] {
    [SourceClientKind::Codex, SourceClientKind::ClaudeCode, SourceClientKind::GrokBuildCli]
}

/// provider 映射是源身份，不能以统一 parser-vN 代替。
pub(super) fn provider(client: SourceClientKind) -> ProviderKind {
    match client {
        SourceClientKind::Codex => ProviderKind::RolloutJsonl,
        SourceClientKind::ClaudeCode => ProviderKind::ClaudeTranscriptJsonl,
        SourceClientKind::GrokBuildCli => ProviderKind::GrokSessionJsonl,
        SourceClientKind::WorkBuddy => ProviderKind::WorkbuddyProjectJsonl,
    }
}

/// 取消发生在读取边界；SQLite 和 blocking future 始终被拥有者 await。
fn check_cancel(token: &ScanCancellation) -> Result<(), &'static str> {
    if token.is_cancelled() { Err("usage_scan_cancelled") } else { Ok(()) }
}

/// 固定来源不存在与实际读取失败保持不同错误。
fn workbuddy_error(error: WorkbuddyReadError) -> &'static str {
    if error == WorkbuddyReadError::SourceUnavailable { "usage_workbuddy_absent" } else { "usage_read_failed" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未扫描的只读查询不创建产品目录、数据库或扫描任务。
    #[test]
    fn read_missing_index_does_not_create_or_scan() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("not-created");
        let service = super::super::UsageService::in_directory(directory.clone()).unwrap();
        let receiver = service.query(UsageQuery::Overview(Some(SourceClientKind::Codex))).unwrap();
        let result = service.runtime.lock().unwrap().as_ref().unwrap().block_on(receiver).unwrap().unwrap();
        let UsageResponse::Overview(data) = result else { panic!("wrong response") };
        assert_eq!(data.records.index_state, LocalIndexState::NotScanned);
        assert!(!directory.exists());
        assert!(!service.scan_status().running);
        service.finish();
    }

    /// 损坏的已存在文件必须失败，不能降为可信零值或被迁移覆盖。
    #[test]
    fn corrupt_index_is_error_and_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join(USAGE_INDEX_FILE_NAME);
        let bytes = b"this is not sqlite";
        std::fs::write(&file, bytes).unwrap();
        let service = super::super::UsageService::in_directory(temp.path().to_owned()).unwrap();
        let receiver = service.query(UsageQuery::Overview(Some(SourceClientKind::Codex))).unwrap();
        let result = service.runtime.lock().unwrap().as_ref().unwrap().block_on(receiver).unwrap();
        assert!(result.is_err());
        service.finish();
        assert_eq!(std::fs::read(file).unwrap(), bytes);
    }

    /// 页面读请求在 writer 被另一操作持有时仍完成，并且不取消后台扫描意图。
    #[test]
    fn query_does_not_wait_for_or_cancel_writer() {
        let temp = tempfile::tempdir().unwrap();
        let service = super::super::UsageService::in_directory(temp.path().to_owned()).unwrap();
        let permit = service.context.writer.try_start().unwrap();
        let token = permit.cancellation_token();
        service.request_scan(Some(SourceClientKind::WorkBuddy), ScanStartOrigin::ExplicitUser).unwrap();
        let receiver = service.query(UsageQuery::Statistics(SourceClientKind::Codex, LocalUsageWindow::Today, UsageDimension::Model)).unwrap();
        let result = service.runtime.lock().unwrap().as_ref().unwrap().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), receiver).await.unwrap().unwrap().unwrap()
        });
        assert!(matches!(result, UsageResponse::Statistics(_)));
        assert!(!token.is_cancelled());
        drop(permit);
        service.finish();
    }

    /// 全部调用和图表没有任何已配置来源时返回失败，且只读路径不创建数据库。
    #[test]
    fn all_without_sources_is_not_empty_success() {
        let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("absent");
        let service=super::super::UsageService::in_directory(directory.clone()).unwrap();
        for query in [UsageQuery::Calls(None,UsageCallsQuery::default()),UsageQuery::Chart(None,LocalUsageWindow::Today,UsageChartDimension::Agent)] {
            let receiver=service.query(query).unwrap();
            let result=service.runtime.lock().unwrap().as_ref().unwrap().block_on(receiver).unwrap();
            assert!(matches!(result,Err("usage_sources_not_configured")));
        }
        service.finish();assert!(!directory.exists());
    }
}
