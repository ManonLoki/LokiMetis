//! 合成网页事件覆盖分项缺失、分页完整性和民用日边界。

use super::*;
use serde_json::{json, Value};

/// 固定 UTC 时间范围，避免测试依赖当前机器时区。
fn range() -> CursorUsageRange {
    CursorUsageRange { start_ms: 1_792_627_200_000, end_ms: 1_792_800_000_000 }
}

/// 构造无凭据或用户身份的逐事件协议夹具。
fn event(id: usize, tokens: Value) -> Value {
    json!({"usageEventId":format!("event-{id}"),"timestamp":range().start_ms.to_string(),
        "model":"model-a","tokenUsage":tokens})
}

/// 使用上游的真实顶层字段，不引入不存在的分页游标。
fn page(total: usize, events: Vec<Value>) -> String {
    json!({"totalUsageEventsCount":total,"usageEventsDisplay":events}).to_string()
}

/// 三页必须全部完成才有快照，重复页面与短页都不能少算后宣称成功。
#[test]
fn cursor_usage_requires_all_205_events() {
    let mut pages = CursorUsagePages::new(range()).unwrap();
    for n in 0..3 {
        let events = (n*100..((n+1)*100).min(205)).map(|id| event(id, json!({"inputTokens":1,"outputTokens":2}))).collect();
        pages.push(n+1, &page(205, events)).unwrap();
    }
    assert_eq!(pages.next_page(), None);
    let summary = pages.finish(range().end_ms, &TimeZone::UTC).unwrap();
    assert_eq!(summary.counts.events, 205);
    assert_eq!(summary.counts.input.tokens, 205);
    assert_eq!(summary.counts.cache_write.missing, 205);
    assert_eq!(summary.models[0].counts, summary.counts);
    assert_eq!(summary.daily.iter().map(|g|g.counts.input.tokens).sum::<u64>(), summary.counts.input.tokens);
    let mut unfinished = CursorUsagePages::new(range()).unwrap();
    unfinished.push(1, &page(205,(0..100).map(|id| event(id,json!({}))).collect())).unwrap();
    assert_eq!(unfinished.finish(0,&TimeZone::UTC), Err(CursorUsageError::Incomplete));
}

/// 缺失不补零，显式零仍计入已返回字段，缓存分项不重复加入输入。
#[test]
fn cursor_usage_preserves_missing_zero_and_independent_cache_fields() {
    let mut pages = CursorUsagePages::new(range()).unwrap();
    pages.push(1,&page(3,vec![event(1,json!({"inputTokens":0,"outputTokens":5,"cacheReadTokens":100})),
        event(2,json!({"inputTokens":7,"outputTokens":0,"cacheWriteTokens":20})),event(3,Value::Null)])).unwrap();
    let data=pages.finish(0,&TimeZone::UTC).unwrap();
    assert_eq!(data.counts.input,ReportedTokens{tokens:7,reported:2,missing:1});
    assert_eq!(data.counts.cache_read,ReportedTokens{tokens:100,reported:1,missing:2});
    assert_eq!(data.counts.cache_write,ReportedTokens{tokens:20,reported:1,missing:2});
    assert_eq!(data.counts.without_usage,1);
}

/// 总数漂移、重复ID、错误页序、短页、缺失ID与超限均失败关闭。
#[test]
fn cursor_usage_rejects_unreliable_pagination() {
    let first=page(101,(0..100).map(|id|event(id,json!({}))).collect());
    for second in [page(102,vec![event(100,json!({}))]),page(101,vec![]),page(101,vec![event(0,json!({}))])] {
        let mut pages=CursorUsagePages::new(range()).unwrap();pages.push(1,&first).unwrap();
        assert!(pages.push(2,&second).is_err());
    }
    let mut pages=CursorUsagePages::new(range()).unwrap();
    assert!(pages.push(2,&page(0,vec![])).is_err());
    assert!(pages.push(1,&page(MAX_EVENTS+1,vec![])).is_err());
    let mut bad=event(0,json!({}));bad.as_object_mut().unwrap().remove("usageEventId");
    assert!(pages.push(1,&page(1,vec![bad])).is_err());
    assert!(pages.push(1,&page(2,vec![event(1,json!({})),event(1,json!({}))])).is_err());
}

/// 非整数、负值和计数溢出不能被夹紧成合法数据。
#[test]
fn cursor_usage_rejects_bad_numbers_and_overflow() {
    for invalid in [json!(-1),json!(1.5),json!("9"),json!(true)] {
        let mut pages=CursorUsagePages::new(range()).unwrap();
        assert!(pages.push(1,&page(1,vec![event(1,json!({"inputTokens":invalid}))])).is_err());
    }
    let mut pages=CursorUsagePages::new(range()).unwrap();
    pages.push(1,&page(2,vec![event(1,json!({"inputTokens":u64::MAX})),event(2,json!({"inputTokens":1}))])).unwrap();
    assert_eq!(pages.finish(0,&TimeZone::UTC),Err(CursorUsageError::Overflow));
}

/// 日桶遵守所选时区，窗口请求半开边界不重复统计午夜。
#[test]
fn cursor_usage_calendar_boundaries_and_safe_model() {
    let now="2026-10-22T00:30:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    let zone=TimeZone::get("America/Los_Angeles").unwrap();
    let range=CursorUsageRange::for_window(LocalUsageWindow::Today,now,&zone).unwrap();
    assert_eq!(jiff::Timestamp::from_millisecond(range.start_ms).unwrap().to_string(),"2026-10-21T07:00:00Z");
    let mut pages=CursorUsagePages::new(range).unwrap();
    let mut row=event(1,json!({"inputTokens":1}));row["timestamp"]=json!(now);row["model"]=json!("/private/user/email");
    pages.push(1,&page(1,vec![row])).unwrap();
    let data=pages.finish(now,&zone).unwrap();
    assert_eq!(data.daily[0].label.as_deref(),Some("2026-10-21"));
    assert_eq!(data.models[0].label,None);
    let dst="2026-11-02T20:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    let yesterday=CursorUsageRange::for_window(LocalUsageWindow::Yesterday,dst,&zone).unwrap();
    assert_eq!(yesterday.end_ms-yesterday.start_ms,25*60*60*1000);
}

/// 空窗口只有明确返回零条且分页完成才能得到可信零，越界事件拒绝。
#[test]
fn cursor_usage_empty_is_explicit_and_out_of_range_is_rejected() {
    let mut pages=CursorUsagePages::new(range()).unwrap();pages.push(1,&page(0,vec![])).unwrap();
    let data=pages.finish(0,&TimeZone::UTC).unwrap();assert_eq!(data.counts.events,0);assert!(!data.daily.is_empty());
    let mut pages=CursorUsagePages::new(range()).unwrap();let mut row=event(1,json!({}));row["timestamp"]=json!(range().end_ms);
    assert!(pages.push(1,&page(1,vec![row])).is_err());
}

/// 真实空响应省略默认字段仍是可信零；未知正文及后续空页不能冒充完整窗口。
#[test]
fn cursor_usage_accepts_omitted_zero_defaults_only_for_empty_windows() {
    for body in ["{}", r#"{"totalUsageEventsCount":0}"#, r#"{"usageEventsDisplay":[]}"#] {
        let mut pages = CursorUsagePages::new(range()).unwrap();
        pages.push(1, body).unwrap();
        let data = pages.finish(0, &TimeZone::UTC).unwrap();
        assert_eq!(data.counts.events, 0);
        assert_eq!(data.counts.input.tokens, 0);
        assert!(data.daily.iter().all(|day| day.counts.events == 0));
    }
    for body in [r#"{"error":"unavailable"}"#, r#"{"totalUsageEventsCount":null}"#,
        r#"{"usageEventsDisplay":null}"#, "null", "[]", ""] {
        assert!(CursorUsagePages::new(range()).unwrap().push(1, body).is_err());
    }
    let mut pages = CursorUsagePages::new(range()).unwrap();
    let row = event(1, json!({"inputTokens":1}));
    assert_eq!(pages.push(1, &json!({"usageEventsDisplay":[row]}).to_string()), Err(CursorUsageError::Incomplete));
    pages.push(1, &page(101, (0..100).map(|id|event(id,json!({}))).collect())).unwrap();
    assert_eq!(pages.push(2, "{}"), Err(CursorUsageError::Incomplete));
    assert_eq!(pages.finish(0, &TimeZone::UTC), Err(CursorUsageError::Incomplete));
}

/// 费用对象、空对象和全 null 不能被标为有 Token 明细。
#[test]
fn cursor_usage_empty_token_objects_remain_unknown() {
    let mut pages=CursorUsagePages::new(range()).unwrap();
    pages.push(1,&page(3,vec![event(1,json!({})),event(2,json!({"totalCents":2})),
        event(3,json!({"inputTokens":null,"cacheReadTokens":null}))])).unwrap();
    let data=pages.finish(0,&TimeZone::UTC).unwrap();
    assert_eq!(data.counts.without_usage,3);
    assert_eq!(data.counts.input.reported,0);
    assert_eq!(data.counts.input.missing,3);
}
