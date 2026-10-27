//! 独立 All 摘要读取，不进入会取消页面请求的查询邮箱，也不触发扫描/迁移。

use super::{ServiceContext, UsageData, query, today_cache};
use loki_metris_host::workbuddy::{self, WorkBuddyReadError};
use jiff::tz::TimeZone;
use loki_metris_core::*;
use std::{sync::Arc, sync::atomic::Ordering};

/// 今日查询与 All 概览共用同一聚合，now/时区/来源在本轮捕获一次。
pub(super) async fn load(
    context: Arc<ServiceContext>,
    now: i64,
    zone: TimeZone,
    cancellation: ScanCancellation,
) -> Result<(UsageData, TodayUsageSnapshot), &'static str> {
    load_with_components(context, now, zone, cancellation).await.map(|(data, all, _)| (data, all))
}

/// 终端消费同轮总量与分量，页面和通知继续沿用原返回形状。
pub(super) async fn load_with_components(context: Arc<ServiceContext>, now: i64, zone: TimeZone,
    cancellation: ScanCancellation) -> Result<(UsageData, TodayUsageSnapshot, Vec<(AiTool, TodayUsageSnapshot)>), &'static str> {
    let day = UsageDay::capture(now, &zone).ok_or("usage_time_unavailable")?;
    let generation = context.notification_source_epoch.load(Ordering::Acquire);
    let selection = context.agents.dashboard_selection().await?;
    if selection.0.is_empty() && !selection.1 {
        return Err("usage_agents_disabled");
    }
    let clients: Vec<_> = selection.0.to_vec();
    // 读取前取键；索引文件、覆盖与民用日都未变时直接复用上轮索引侧汇总。
    let key = today_cache::key_for(&context, today_cache::Scope::Combined, &clients, &day).await;
    let cached = key.as_ref().and_then(|key| context.today_cache.lookup(key));
    let mut signature = vec![];
    let mut raw_inputs = vec![];
    let mut components = vec![];
    let mut all_ready = true;
    if let Some(cached) = &cached {
        signature = cached.signature.clone();
        all_ready = cached.all_ready;
        components = cached.components.clone();
        for (_, records, _) in &mut components { today_cache::restamp(records, now); }
    } else {
        for client in &selection.0 {
            let snapshot = query::read_snapshot(&context.directory, *client, now, &zone).await?;
            let ready = snapshot.read_ready && matches!(snapshot.index_state,
                LocalIndexState::Ready | LocalIndexState::ReadyNoCalls) && snapshot.roots.iter().any(|r| r.enabled);
            all_ready &= ready;
            let coverage = query::coverage(&context, *client);
            let label = client.provider().parser_source_label(client.parser_version());
            signature.push(format!("{client:?}:{}", client.parser_version()));
            signature.extend(
                snapshot
                    .roots
                    .iter()
                    .map(|r| format!("{client:?}:{}:{}", r.root_id, r.enabled)),
            );
            raw_inputs.push((*client, snapshot, coverage, label, ready));
        }
    }
    // 保存用的签名与就绪度须在合并 WorkBuddy 之前取得，它们只描述索引侧。
    let index_signature = signature.clone();
    let index_ready = all_ready;
    let mut workbuddy = None;
    let mut failed = false;
    let mut absent = false;
    if selection.1 {
        let homes = workbuddy::resolve_workbuddy_homes();
        signature.push("workbuddy".into());
        signature.extend(homes.iter().map(|p| stable_id("wb", &path_key(p))));
        match workbuddy::read_workbuddy_usage_snapshot_in_zone(
            &homes,
            now,
            TimeStandard::Local,
            &zone,
        )
        .await
        {
            Ok(value) => workbuddy = Some(value),
            Err(WorkBuddyReadError::SourceUnavailable) => {
                absent = true;
                all_ready = false;
            }
            Err(WorkBuddyReadError::Read) => {
                failed = true;
                all_ready = false;
            }
        }
    }
    if cancellation.is_cancelled() {
        return Err("usage_scan_cancelled");
    }
    signature.sort();
    let source = stable_id("selection", &signature.join("|"));
    let zone_for_aggregate = zone.clone();
    let cached_records = cached.map(|cached| {
        let mut records = cached.records.clone();
        today_cache::restamp(&mut records, now);
        records
    });
    let day_for_cache = day.clone();
    let permit = context.today_aggregate_gate.clone().acquire_owned().await.map_err(|_| "usage_aggregate_failed")?;
    let work = move || {
        // 命中缓存时跳过联合快照构造；新算出的索引侧汇总随结果返回，供读后校验保存。
        let mut inputs = Vec::new();
        for (client, snapshot, coverage, label, ready) in raw_inputs {
            let effective = snapshot.effective_coverage(coverage.clone());
            let records = build_local_windows_with_standard(&snapshot.canonical, &effective, snapshot.index_state,
                now, client.provider(), Some(&label), TimeStandard::Local, &zone_for_aggregate)
                .map_err(|_| "usage_aggregate_failed")?;
            components.push((client, records, ready));
            inputs.push(AgentUsageSnapshot::new(client, snapshot, coverage, Some(label)));
        }
        let components_for_cache = components.clone();
        let mut index_records = None;
        let base = match cached_records {
            Some(records) => Some(records),
            None => match combine_agent_usage_snapshots(inputs) {
                Ok(combined) => {
                    let records = build_combined_local_windows_with_standard(
                        &combined,
                        now,
                        TimeStandard::Local,
                        &zone_for_aggregate,
                    )
                    .map_err(|_| "usage_aggregate_failed")?;
                    index_records = Some(records.clone());
                    Some(records)
                }
                Err(UsageViewError::NoEnabledAgents | UsageViewError::NoConfiguredDataSources) => {
                    None
                }
                Err(_) => return Err("usage_aggregate_failed"),
            },
        };
        let records = match base {
            Some(records) => {
                if let Some(snapshot) = &workbuddy {
                    merge_workbuddy_into_local_windows(
                        records,
                        &snapshot.windows,
                        &snapshot.coverage,
                    )
                } else if failed {
                    mark_workbuddy_unavailable_in_local_windows(records)
                } else {
                    records
                }
            }
            None => {
                let wb = workbuddy.as_ref().ok_or(if failed {
                    "usage_read_failed"
                } else if absent {
                    "usage_workbuddy_absent"
                } else {
                    "usage_sources_not_configured"
                })?;
                build_workbuddy_only_local_windows(
                    &wb.windows,
                    &wb.coverage,
                    now,
                    ProviderKind::CombinedLocalAgents,
                )
            }
        };
        let mut agents = Vec::new();
        for (client, records, ready) in components {
            let agent = match client { SourceClientKind::Codex => AiTool::Codex,
                SourceClientKind::ClaudeCode => AiTool::ClaudeCode, SourceClientKind::Grok => AiTool::Grok, SourceClientKind::WorkBuddy => AiTool::WorkBuddy };
            let value = TodayUsageSnapshot::from_records(day.clone(), source.clone(), &records, now, ready, false)
                .ok_or("usage_aggregate_failed")?;
            agents.push((agent, value));
        }
        if selection.1 {
            let value = if let Some(wb) = &workbuddy {
                let records = build_workbuddy_only_local_windows(&wb.windows, &wb.coverage, now, ProviderKind::WorkBuddyProjectJsonl);
                TodayUsageSnapshot::from_records(day.clone(), source.clone(), &records, now, true, false).ok_or("usage_aggregate_failed")?
            } else { TodayUsageSnapshot { day: day.clone(), source: source.clone(), total_tokens: 0, observed_ms: now,
                availability: if failed { TodayAvailability::Unavailable } else { TodayAvailability::NoRecords },
                completeness: Completeness::Unknown, confidence: Confidence::Exact } };
            agents.push((AiTool::WorkBuddy, value));
        }
        let today = TodayUsageSnapshot::from_records(day, source, &records, now, all_ready, failed)
            .and_then(|value| value.with_available_components(&agents))
            .ok_or("usage_aggregate_failed")?;
        Ok((
            UsageData {
                records,
                workbuddy,
                workbuddy_unavailable: failed,
            },
            today,
            index_records,
            agents,
            components_for_cache,
        ))
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let aggregate_cancel = cancellation.clone();
    context.background.spawn_blocking("today-aggregate", move |shutdown| {
        let _permit = permit;
        let result = if shutdown.is_cancelled() || aggregate_cancel.is_cancelled() { Err("usage_scan_cancelled") } else { work() };
        let _ = tx.send(result);
    })?;
    let result = rx.await.map_err(|_| "usage_aggregate_failed")??;
    if cancellation.is_cancelled() { return Err("usage_scan_cancelled"); }
    let (data, today, index_records, agents, components_for_cache) = result;
    if selection != context.agents.dashboard_selection().await?
        || generation != context.notification_source_epoch.load(Ordering::Acquire)
    {
        return Err("usage_snapshot_superseded");
    }
    // 读取期间索引文件未变化才保存；并发写入中途读到的数据不能当作稳定结果。
    if let (Some(key), Some(records)) = (key, index_records) {
        let after = today_cache::key_for(&context, today_cache::Scope::Combined, &clients, &day_for_cache).await;
        if after.as_ref() == Some(&key) {
            context.today_cache.store_components(key, index_signature, index_ready, records, components_for_cache);
        }
    }
    Ok((data, today, agents))
}
