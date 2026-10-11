//! 独立 All 摘要读取，不进入会取消页面请求的查询邮箱，也不触发扫描/迁移。

use super::{ServiceContext, UsageData, query, today_cache};
use crate::backend::workbuddy::{self, WorkbuddyReadError};
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
    let day = UsageDay::capture(now, &zone).ok_or("usage_time_unavailable")?;
    let generation = context.notification_source_epoch.load(Ordering::Acquire);
    let selection = context.agents.dashboard_selection().await?;
    if selection.0.is_empty() && !selection.1 {
        return Err("usage_agents_disabled");
    }
    let clients: Vec<_> = selection.0.iter().copied().collect();
    // 读取前取键；索引文件、覆盖与民用日都未变时直接复用上轮索引侧汇总。
    let key = today_cache::key_for(&context, today_cache::Scope::Combined, &clients, &day).await;
    let cached = key.as_ref().and_then(|key| context.today_cache.lookup(key));
    let mut signature = vec![];
    let mut inputs = vec![];
    let mut all_ready = true;
    if let Some(cached) = &cached {
        signature = cached.signature.clone();
        all_ready = cached.all_ready;
    } else {
        for client in &selection.0 {
            let snapshot = query::read_snapshot(&context.directory, *client, now, &zone).await?;
            all_ready &= snapshot.read_ready && matches!(
                snapshot.index_state,
                LocalIndexState::Ready | LocalIndexState::ReadyNoCalls
            ) && snapshot.roots.iter().any(|r| r.enabled);
            signature.push(format!("{client:?}:{}", client.parser_version()));
            signature.extend(
                snapshot
                    .roots
                    .iter()
                    .map(|r| format!("{client:?}:{}:{}", r.root_id, r.enabled)),
            );
            inputs.push(AgentUsageSnapshot::new(
                *client,
                snapshot,
                query::coverage(&context, *client),
                Some(query::provider(*client).parser_source_label(client.parser_version())),
            ));
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
            Err(WorkbuddyReadError::SourceUnavailable) => {
                absent = true;
                all_ready = false;
            }
            Err(WorkbuddyReadError::Read) => {
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
    let result = tokio::task::spawn_blocking(move || {
        // 命中缓存时跳过联合快照构造；新算出的索引侧汇总随结果返回，供读后校验保存。
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
        let today = TodayUsageSnapshot::from_records(day, source, &records, now, all_ready, failed)
            .ok_or("usage_aggregate_failed")?;
        Ok((
            UsageData {
                records,
                workbuddy,
            },
            today,
            index_records,
        ))
    })
    .await
    .map_err(|_| "usage_aggregate_failed")??;
    let (data, today, index_records) = result;
    let index_ready_fresh = index_records.is_some();
    if selection != context.agents.dashboard_selection().await?
        || generation != context.notification_source_epoch.load(Ordering::Acquire)
    {
        return Err("usage_snapshot_superseded");
    }
    // 读取期间索引文件未变化才保存；并发写入中途读到的数据不能当作稳定结果。
    if let (Some(key), Some(records)) = (key, index_records) {
        let after = today_cache::key_for(&context, today_cache::Scope::Combined, &clients, &day_for_cache).await;
        if after.as_ref() == Some(&key) {
            context.today_cache.store(key, index_signature, index_ready, records);
        }
    }
    // 后台摘要不经查询邮箱；全量读取之后同样归还分配器空闲页。
    if index_ready_fresh {
        crate::native::memory::release_free_pages();
    }
    Ok((data, today))
}
