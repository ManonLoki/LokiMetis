//! 今日摘要索引侧缓存：未变化时结果与重算一致，索引或民用日变化时必须失效。

use super::source_tests::{fixture_root, register_fixture};
use super::*;
use loki_metris_core::*;

/// 以隔离目录建立只含一个 Codex 根的服务，返回服务、来源根与根 ID。
pub(super) fn indexed_service(temp: &std::path::Path) -> (Arc<UsageService>, std::path::PathBuf, String) {
    let source = fixture_root(temp);
    let service = UsageService::in_directory(temp.join("data")).unwrap();
    let id = register_fixture(&service, &source);
    service.save_enabled_ai_tools(vec![AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
    service
        .mutate_source(SourceClientKind::Codex, SourceOperation::Reindex { root_id: id.clone() })
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    (service, source, id)
}

/// 在服务 runtime 上执行一次与生产相同的今日摘要读取。
fn load(service: &UsageService, now: i64) -> (UsageData, TodayUsageSnapshot) {
    let runtime = service.runtime.lock().unwrap();
    runtime
        .as_ref()
        .unwrap()
        .block_on(super::today::load(service.context.clone(), now, jiff::tz::TimeZone::UTC, ScanCancellation::new()))
        .unwrap()
}

/// 把观测时间归零后比较，其余事实必须逐项相同。
fn without_observed(mut data: UsageData) -> UsageData {
    super::today_cache::restamp(&mut data.records, 0);
    data
}

/// 同一天索引未变时，缓存命中与首次计算给出完全相同的事实，且只刷新观测时间。
#[test]
fn unchanged_index_reuses_summary_with_identical_facts() {
    let temp = tempfile::tempdir().unwrap();
    let (service, _source, _id) = indexed_service(temp.path());
    let first_now = "2026-10-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    let later = first_now + 30_000;

    let (first_data, first) = load(&service, first_now);
    assert_eq!(first.total_tokens, 110);
    assert!(service.context.today_cache.is_populated(), "首轮读取后应保存索引侧汇总");
    let (second_data, second) = load(&service, later);

    assert_eq!(first.total_tokens, second.total_tokens);
    assert_eq!(first.source, second.source);
    assert_eq!(first.availability, second.availability);
    assert!(second_data.records.windows.iter().all(|w| w.fact.observed_at_epoch_ms == later), "命中后观测时间必须是本轮时刻");
    assert_eq!(without_observed(first_data).records, without_observed(second_data).records);
    service.finish();
}

/// 索引被扫描事务改写后，缓存必须失效并读到新总量。
#[test]
fn rewritten_index_invalidates_cached_summary() {
    let temp = tempfile::tempdir().unwrap();
    let (service, source, id) = indexed_service(temp.path());
    let now = "2026-10-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    assert_eq!(load(&service, now).1.total_tokens, 110);
    assert_eq!(load(&service, now + 1).1.total_tokens, 110, "第二轮命中缓存");

    let file = source.join("sessions/rollout-source.jsonl");
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("{\"timestamp\":\"2026-10-09T00:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":200,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":200}}}}\n");
    std::fs::write(&file, text).unwrap();
    service
        .mutate_source(SourceClientKind::Codex, SourceOperation::Reindex { root_id: id })
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();

    assert_eq!(load(&service, now + 2).1.total_tokens, 310, "索引变化后必须重新读取");
    service.finish();
}

/// 跨过民用日后窗口锚点改变，不能继续返回前一天的"今日"总量。
#[test]
fn new_civil_day_invalidates_cached_summary() {
    let temp = tempfile::tempdir().unwrap();
    let (service, _source, _id) = indexed_service(temp.path());
    let today = "2026-10-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    let tomorrow = "2026-10-10T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    assert_eq!(load(&service, today).1.total_tokens, 110);

    let (data, next_day) = load(&service, tomorrow);
    assert_eq!(next_day.total_tokens, 0, "新的一天今日总量应重新计算");
    let yesterday = data.records.windows.iter().find(|w| w.window == LocalUsageWindow::Yesterday).unwrap();
    assert_eq!(yesterday.fact.value.tokens.total_tokens, 110);
    service.finish();
}

/// 单个 Agent 的概览页走同一缓存：未变时事实相同，索引改写后立即读到新总量。
#[test]
fn single_agent_overview_reuses_summary_until_index_changes() {
    let temp = tempfile::tempdir().unwrap();
    let (service, source, id) = indexed_service(temp.path());
    let overview = |service: &UsageService| match service
        .query(UsageQuery::Overview(Some(SourceClientKind::Codex)))
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap()
    {
        UsageResponse::Overview(data) => data,
        _ => panic!("概览查询必须返回概览"),
    };
    // 夹具数据固定在 2026-10-09，而查询使用真实当前时间，窗口归属随运行日期变化；
    // 因此以全部窗口总量之和作为"是否读到新数据"的稳定指标。
    let sum = |data: &UsageData| data.records.windows.iter().map(|w| w.fact.value.tokens.total_tokens).sum::<u64>();
    let first = overview(&service);
    assert!(service.context.today_cache.is_populated());
    let second = overview(&service);
    assert_eq!(without_observed(first).records, without_observed(second.clone()).records);

    let file = source.join("sessions/rollout-source.jsonl");
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("{\"timestamp\":\"2026-10-09T00:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":200,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":200}}}}\n");
    std::fs::write(&file, text).unwrap();
    service
        .mutate_source(SourceClientKind::Codex, SourceOperation::Reindex { root_id: id })
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    let third = overview(&service);
    assert_ne!(sum(&second), sum(&third), "索引改写后概览必须反映新数据");
    service.finish();
}

/// 总量和各Agent分量来自同轮输入；缓存命中、索引更新、跨日均保持一致且不伪造Cursor统计。
#[test]
fn terminal_token_components_follow_same_observation_and_cache_generation() {
    let temp = tempfile::tempdir().unwrap();
    let (service, source, id) = indexed_service(temp.path());
    let now = "2026-10-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    let read = |at| {
        let runtime = service.runtime.lock().unwrap();
        runtime.as_ref().unwrap().block_on(super::today::load_with_components(service.context.clone(), at, jiff::tz::TimeZone::UTC, ScanCancellation::new())).unwrap()
    };
    for at in [now, now + 30_000] {
        let (_, all, agents) = read(at);
        assert_eq!(agents.len(),1); assert_eq!(agents[0].0,AiTool::Codex);
        assert_eq!(agents[0].1.total_tokens,all.total_tokens); assert_eq!(all.total_tokens,110);
        assert_eq!(agents[0].1.observed_ms,at); assert_eq!(agents[0].1.day,all.day);
    }
    let file=source.join("sessions/rollout-source.jsonl"); let mut raw=std::fs::read_to_string(&file).unwrap();
    raw.push_str("{\"timestamp\":\"2026-10-09T00:02:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":200,\"cached_input_tokens\":0,\"output_tokens\":0,\"total_tokens\":200}}}}\n");
    std::fs::write(&file,raw).unwrap();
    service.mutate_source(SourceClientKind::Codex,SourceOperation::Reindex { root_id:id }).unwrap().blocking_recv().unwrap().unwrap();
    let (_,all,agents)=read(now+60_000); assert_eq!(all.total_tokens,310);assert_eq!(agents[0].1.total_tokens,310);
    let (_,all,agents)=read(now+86_400_000); assert_eq!(all.total_tokens,0);assert_eq!(agents[0].1.total_tokens,0);assert_eq!(agents[0].1.day,all.day);
    service.finish();
}

/// Claude 已登记却没有可解释记录时，真实联合读取及缓存仍须保留 Codex 的部分总量。
#[test]
fn terminal_total_keeps_indexed_usage_when_claude_is_unscanned() {
    let temp = tempfile::tempdir().unwrap();
    let (service, _, _) = indexed_service(temp.path());
    let root = temp.path().join("claude");
    std::fs::create_dir_all(root.join("projects")).unwrap();
    let runtime = service.runtime.lock().unwrap().as_ref().unwrap().handle().clone();
    runtime.block_on(async {
        let directory = source_client_app_data_dir(&service.context.directory, SourceClientKind::ClaudeCode);
        let mut index = loki_metris_host::local_index::LocalIndex::open_in_app_data(
            &directory, SourceClientKind::ClaudeCode.parser_version()).await.unwrap();
        index.register_confirmed_root_fields(&stable_id("claude-root", "empty-claude"), &root, "空 Claude",
            loki_metris_host::local_index::DiscoveryMethod::Registered).await.unwrap();
    });
    service.save_enabled_ai_tools(vec![AiTool::Codex, AiTool::ClaudeCode]).unwrap().blocking_recv().unwrap().unwrap();
    let now = "2026-10-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    for at in [now, now + 30_000] {
        let (_, all, agents) = runtime.block_on(super::today::load_with_components(
            service.context.clone(), at, jiff::tz::TimeZone::UTC, ScanCancellation::new())).unwrap();
        let wire = loki_metris_core::devices::link::data::TokenSnapshot::from_observation(&all, &agents).unwrap();
        assert_eq!(wire.all.total.as_deref(), Some("110"));
        assert_eq!(wire.all.availability, "available");
        assert_eq!(wire.all.completeness, Completeness::Unknown, "未扫描联合来源原有 Unknown 不能升级为 Partial");
        let claude = wire.agents.iter().find(|agent| agent.agent == AiTool::ClaudeCode).unwrap();
        assert_eq!(claude.amount.total, None);
        assert_eq!(claude.amount.availability, "noRecords");
        assert!(!all.eligible(at, &all.day, &all.source));
        assert_eq!(wire.observed_ms, at);
    }
    service.finish();
}
