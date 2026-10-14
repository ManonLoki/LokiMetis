//! 统计页与图表页结果缓存：索引未变时复用且只刷新观测时间，索引改写后必须失效。

use super::source_tests::{fixture_root, register_fixture};
use super::*;
use loki_metris_core::*;

/// 以隔离目录建立只含一个已索引 Codex 根的服务，返回服务、来源根与根 ID。
fn indexed_service(temp: &std::path::Path) -> (Arc<UsageService>, std::path::PathBuf, String) {
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

/// 在服务 runtime 上执行一次与生产相同的统计页查询。
fn statistics(service: &UsageService) -> UsageStatisticsPage {
    let query = UsageQuery::Statistics(SourceClientKind::Codex, LocalUsageWindow::ThisMonth, UsageDimension::Model);
    let runtime = service.runtime.lock().unwrap();
    match runtime.as_ref().unwrap().block_on(super::query::load(service.context.clone(), query, ScanCancellation::new())) {
        Ok(UsageResponse::Statistics(page)) => page,
        _ => panic!("统计页查询应成功"),
    }
}

/// 把观测时间归零后比较，其余事实必须逐项相同。
fn without_observed(mut page: UsageStatisticsPage) -> UsageStatisticsPage {
    page.observed_at_epoch_ms = 0;
    page.fact.observed_at_epoch_ms = 0;
    page
}

/// 索引未变时第二次查询命中缓存：页面事实相同，缓存键保持不变。
#[test]
fn unchanged_index_reuses_statistics_page() {
    let temp = tempfile::tempdir().unwrap();
    let (service, _source, _id) = indexed_service(temp.path());
    let first = statistics(&service);
    let key = service.context.page_cache.cached_key().expect("首轮查询后应保存页面");
    assert_eq!(service.context.page_cache.hits(), 0);
    let second = statistics(&service);
    assert_eq!(service.context.page_cache.hits(), 1, "第二次查询必须命中缓存而不是重新计算");
    assert!(service.context.page_cache.cached_key() == Some(key), "未变化的索引不应替换缓存");
    assert_eq!(without_observed(first), without_observed(second));
    service.finish();
}

/// 扫描事务改写索引后，下一次查询必须以新的索引文件戳重新计算并替换缓存。
#[test]
fn rewritten_index_invalidates_statistics_page() {
    let temp = tempfile::tempdir().unwrap();
    let (service, source, id) = indexed_service(temp.path());
    statistics(&service);
    let before = service.context.page_cache.cached_key().expect("首轮查询后应保存页面");

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

    statistics(&service);
    assert_eq!(service.context.page_cache.hits(), 0, "索引改写后不得命中旧页面");
    let after = service.context.page_cache.cached_key().expect("重新计算后应保存页面");
    assert!(after != before, "索引改写后缓存键必须随文件戳变化");
    service.finish();
}
