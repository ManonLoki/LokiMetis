//! 驱动 [`ScanClient`] 描述的一条本机扫描客户端管线的共同骨架：打开索引
//! -> 并发执行快速重验证已知根与（可选）全设备发现 -> 合并发现结果 ->
//! 登记根 -> 增量扫描 -> 合并覆盖 -> 读回来源摘要。全设备发现与快速
//! 重验证互不依赖（前者不读写索引），因此全设备遍历一开始就被派发到
//! 阻塞线程池，与快速校验及其后的数据库写入真正并发执行。Codex 与
//! Claude 只在 `execute_scan`/`execute_claude_scan` 里各自提供一个
//! `ScanClient` 实现，本函数不知道也不需要知道自己驱动的是哪一个。

use std::path::PathBuf;
use std::sync::Arc;

use loki_metis_core::{
    CoverageState, DiscoveredRootIdentity, LocalScanOutput, LocalScanProgress,
    RegisteredRootIdentity, ScanCancellation, ScanDiscoveryResult, ScanKind, ScanStartOrigin,
    SourceRootReindexRequest, confirmed_invalid_roots_to_remove,
    discovery_roots_for_active_registration, merge_coverage_reports, scan_discovery_for_scan_kind,
    source_root_not_found_message, source_root_reindex_requires_enabled_message,
    source_root_reindex_validation_failed_message,
};

use crate::backend::local_index::{
    LocalIndex, RegisteredRoot, ScanConfig, ScanMode, registered_path_matches_candidate,
};
use crate::local_view::load_source_root_summaries_for_parser;
use crate::runtime::now_epoch_ms;

use super::client::ScanClient;
use super::support::local_scan_task_error_message;

/// 从产品 app-data 根读取已保存天数，按设备当地民用日得到派生用量保留下界。
pub(super) fn scan_retention_policy(
    settings_app_data_dir: &std::path::Path,
    origin: ScanStartOrigin,
    has_current_usage: bool,
    observed_at_epoch_ms: i64,
) -> loki_metis_core::LocalIndexScanPolicy {
    let privacy = crate::privacy_store::load_settings(settings_app_data_dir)
        .unwrap_or_else(|_| crate::privacy_store::LocalPrivacySettings::default());
    loki_metis_core::local_index_scan_policy_with_retention(
        origin,
        has_current_usage,
        observed_at_epoch_ms,
        privacy.retention_days,
        loki_metis_core::TimeStandard::Local,
        &jiff::tz::TimeZone::system(),
    )
}

/// 打开索引、编排一次完整发现 -> 登记 -> 扫描流程，返回统一扫描输出。
/// `app_data_dir` 是该客户端独占的用量库目录；`settings_app_data_dir`
/// 必须是产品 app-data 根，才能读到用户已保存的保留天数。
/// `account_context_gate` 只有 Codex 客户端提供（登记/覆盖写入需要和
/// 主数据目录切换串行化）；Claude 客户端传 `None`，直接跳过加锁。
pub(super) async fn run_scan<C: ScanClient>(
    app_data_dir: PathBuf,
    settings_app_data_dir: PathBuf,
    account_context_gate: Option<Arc<tokio::sync::Mutex<()>>>,
    kind: ScanKind,
    origin: ScanStartOrigin,
    cancellation: ScanCancellation,
    mut on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
) -> Result<LocalScanOutput, String> {
    let mut index = LocalIndex::open_in_app_data(&app_data_dir, C::parser_version())
        .await
        .map_err(C::map_local_error)?;
    let started_at_epoch_ms = now_epoch_ms();
    let has_current_usage = index
        .has_current_parser_usage()
        .await
        .map_err(C::map_local_error)?;
    let scan_policy = scan_retention_policy(
        &settings_app_data_dir,
        origin,
        has_current_usage,
        started_at_epoch_ms,
    );
    index
        .prune_usage_before(scan_policy.retention_since_epoch_ms)
        .await
        .map_err(C::map_local_error)?;
    let mut background_index_root_ids = index
        .claim_background_index_roots()
        .await
        .map_err(C::map_local_error)?;
    let all_registered_roots = index.all_roots().await.map_err(C::map_local_error)?;
    // 周期任务和显式更新都只允许触碰已经完成首次验证的根，或本次显式
    // 置为 indexing 的根；confirmedUnindexed 不能因为“已启用”而被隐式读取。
    let registered_roots: Vec<RegisteredRoot> =
        index.known_roots().await.map_err(C::map_local_error)?;

    // 全设备发现是一次独立的文件系统遍历，不读写索引也不依赖已知根，
    // 因此在等待快速重验证结果之前就把它派发到阻塞线程池：两者在物理
    // 硬件上真正并发执行，全设备遍历（数量级更慢）无需再等快速校验和
    // 随后的数据库写入完成才开始。
    let full_device_task = if kind == ScanKind::FullDevice {
        let cancel_for_full = cancellation.clone();
        let (progress_tx, progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = tauri::async_runtime::spawn_blocking(move || {
            C::discover_full_device_with_progress(&cancel_for_full, |progress| {
                let _ = progress_tx.send(progress);
            })
        });
        Some((handle, progress_rx))
    } else {
        None
    };

    let cancel_for_quick = cancellation.clone();
    let known_validation = tauri::async_runtime::spawn_blocking(move || {
        C::discover_quick(registered_roots, &cancel_for_quick)
    })
    .await
    .map_err(|_| local_scan_task_error_message())?;
    let (
        known_roots,
        known_confirmed_invalid_root_ids,
        known_unconfirmed_root_ids,
        known_coverage,
        known_dirs,
        known_sym,
        known_net,
    ) = known_validation.into_parts();
    let removable_invalid_root_ids = confirmed_invalid_roots_to_remove(
        &known_confirmed_invalid_root_ids,
        &background_index_root_ids,
    );
    {
        let _account_context_guard = match &account_context_gate {
            Some(gate) => Some(gate.lock().await),
            None => None,
        };
        index
            .remove_roots(&removable_invalid_root_ids)
            .await
            .map_err(C::map_local_error)?;
        index
            .record_roots_coverage(&known_unconfirmed_root_ids, CoverageState::Partial)
            .await
            .map_err(C::map_local_error)?;
    }
    let known_validation = C::Discovery::from_parts(
        known_roots,
        known_confirmed_invalid_root_ids,
        known_unconfirmed_root_ids,
        known_coverage,
        known_dirs,
        known_sym,
        known_net,
    );

    let full_discovery = if let Some((discovery_task, mut progress_rx)) = full_device_task {
        let max_directories = C::full_device_max_directories();
        while let Some(progress) = progress_rx.recv().await {
            on_progress(LocalScanProgress::Discovering {
                directories_scanned: progress.directories_scanned,
                roots_discovered: progress.roots_discovered,
                max_directories,
            });
        }
        let full_discovery = discovery_task
            .await
            .map_err(|_| local_scan_task_error_message())?;
        let (f_roots, f_civ, f_unc, f_coverage, f_dirs, f_sym, f_net) = full_discovery.into_parts();
        on_progress(LocalScanProgress::DiscoveryFinished {
            directories_scanned: f_dirs,
            roots_discovered: f_coverage.roots_discovered,
        });
        Some(C::Discovery::from_parts(
            f_roots, f_civ, f_unc, f_coverage, f_dirs, f_sym, f_net,
        ))
    } else {
        None
    };

    let registered_for_merge = all_registered_roots.clone();
    let discovery = tauri::async_runtime::spawn_blocking(move || {
        scan_discovery_for_scan_kind(
            kind,
            known_validation,
            full_discovery,
            &registered_for_merge,
            |registered, root| {
                registered_path_matches_candidate(registered.path(), root.path()).unwrap_or(false)
            },
        )
    })
    .await
    .map_err(|_| local_scan_task_error_message())?;
    let (discovery_roots, _, _, discovery_coverage, _, _, _) = discovery.into_parts();

    {
        let _account_context_guard = match &account_context_gate {
            Some(gate) => Some(gate.lock().await),
            None => None,
        };
        for root in &discovery_roots {
            C::register_root(&mut index, root)
                .await
                .map_err(C::map_local_error)?;
        }
        let discovered_root_ids = discovery_roots
            .iter()
            .map(|root| root.root_id().to_owned())
            .collect::<Vec<_>>();
        let newly_claimed_root_ids = index
            .claim_discovered_roots_for_current_scan(&discovered_root_ids)
            .await
            .map_err(C::map_local_error)?;
        background_index_root_ids.extend(newly_claimed_root_ids);
    }
    let enabled_roots = index.known_roots().await.map_err(C::map_local_error)?;
    let roots_to_scan = discovery_roots_for_active_registration(&discovery_roots, &enabled_roots);
    let config = ScanConfig {
        mode: match kind {
            ScanKind::Quick => ScanMode::Quick,
            ScanKind::FullDevice => ScanMode::FullDevice,
        },
        scan_since_epoch_ms: scan_policy.scan_since_epoch_ms,
        ingest_since_epoch_ms: scan_policy.retention_since_epoch_ms,
        started_at_epoch_ms: Some(started_at_epoch_ms),
        ..ScanConfig::default()
    };
    let summary = C::scan_discovered_roots(
        &mut index,
        &roots_to_scan,
        config,
        &cancellation,
        |progress| {
            on_progress(LocalScanProgress::Indexing {
                kind,
                current_root_id: Some(progress.current_root_id),
                roots_completed: progress.roots_completed,
                roots_total: progress.roots_total,
                files_scanned: progress.files_scanned,
                calls_added: progress.calls_added,
            });
        },
    )
    .await
    .map_err(C::map_local_error)?;
    let merged_coverage = merge_coverage_reports(discovery_coverage, &summary.coverage);
    if merged_coverage.state != CoverageState::Cancelled {
        index
            .finish_background_index_roots(&background_index_root_ids)
            .await
            .map_err(C::map_local_error)?;
    }
    drop(index);
    let roots = load_source_root_summaries_for_parser(
        &app_data_dir,
        C::parser_version(),
        C::provider_kind().source_environment_label(),
    )
    .await?;
    Ok(LocalScanOutput {
        files_scanned: summary.files_scanned,
        call_count: summary.aggregate.call_count,
        coverage: merged_coverage,
        roots,
    })
}

/// 重新验证并强制重建一个已启用数据根。该流程不做默认/环境发现、不改变
/// 其他根，也不预先删除旧 generation；来源只有完整解析成功后才切换。
pub(super) async fn run_reindex<C: ScanClient>(
    app_data_dir: PathBuf,
    settings_app_data_dir: PathBuf,
    account_context_gate: Option<Arc<tokio::sync::Mutex<()>>>,
    request: SourceRootReindexRequest,
    cancellation: ScanCancellation,
    mut on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
) -> Result<LocalScanOutput, String> {
    let mut index = LocalIndex::open_in_app_data(&app_data_dir, C::parser_version())
        .await
        .map_err(C::map_local_error)?;
    let all_registered_roots = index.all_roots().await.map_err(C::map_local_error)?;
    let target = all_registered_roots
        .iter()
        .find(|root| root.root_id.as_deref() == Some(request.root_id()))
        .cloned()
        .ok_or_else(|| source_root_not_found_message().to_owned())?;
    if !target.enabled {
        return Err(source_root_reindex_requires_enabled_message().to_owned());
    }

    let cancellation_for_validation = cancellation.clone();
    let validation = tauri::async_runtime::spawn_blocking(move || {
        C::discover_registered(vec![target], &cancellation_for_validation)
    })
    .await
    .map_err(|_| local_scan_task_error_message())?;
    let validation = scan_discovery_for_scan_kind(
        ScanKind::Quick,
        validation,
        None,
        &all_registered_roots,
        |registered, root| {
            registered_path_matches_candidate(registered.path(), root.path()).unwrap_or(false)
        },
    );
    let (
        discovered_roots,
        confirmed_invalid_root_ids,
        unconfirmed_root_ids,
        mut discovery_coverage,
        _,
        _,
        _,
    ) = validation.into_parts();

    if cancellation.is_cancelled() || discovery_coverage.state == CoverageState::Cancelled {
        discovery_coverage.state = CoverageState::Cancelled;
        drop(index);
        let roots = load_source_root_summaries_for_parser(
            &app_data_dir,
            C::parser_version(),
            C::provider_kind().source_environment_label(),
        )
        .await?;
        return Ok(LocalScanOutput {
            files_scanned: 0,
            call_count: 0,
            coverage: discovery_coverage,
            roots,
        });
    }

    let roots_to_scan = discovered_roots
        .into_iter()
        .filter(|root| root.root_id() == request.root_id())
        .collect::<Vec<_>>();
    if roots_to_scan.len() != 1
        || confirmed_invalid_root_ids
            .iter()
            .any(|root_id| root_id == request.root_id())
        || unconfirmed_root_ids
            .iter()
            .any(|root_id| root_id == request.root_id())
    {
        return Err(source_root_reindex_validation_failed_message().to_owned());
    }

    {
        let _account_context_guard = match &account_context_gate {
            Some(gate) => Some(gate.lock().await),
            None => None,
        };
        C::register_root(&mut index, &roots_to_scan[0])
            .await
            .map_err(C::map_local_error)?;
    }
    let started_at_epoch_ms = now_epoch_ms();
    let has_current_usage = index
        .has_current_parser_usage()
        .await
        .map_err(C::map_local_error)?;
    let scan_policy = scan_retention_policy(
        &settings_app_data_dir,
        ScanStartOrigin::ExplicitUser,
        has_current_usage,
        started_at_epoch_ms,
    );
    let summary = C::scan_discovered_roots(
        &mut index,
        &roots_to_scan,
        ScanConfig {
            mode: ScanMode::Quick,
            scan_since_epoch_ms: scan_policy.retention_since_epoch_ms,
            ingest_since_epoch_ms: scan_policy.retention_since_epoch_ms,
            force_rebuild: true,
            started_at_epoch_ms: Some(started_at_epoch_ms),
            ..ScanConfig::default()
        },
        &cancellation,
        |progress| {
            on_progress(LocalScanProgress::Indexing {
                kind: ScanKind::Quick,
                current_root_id: Some(progress.current_root_id),
                roots_completed: progress.roots_completed,
                roots_total: progress.roots_total,
                files_scanned: progress.files_scanned,
                calls_added: progress.calls_added,
            });
        },
    )
    .await
    .map_err(C::map_local_error)?;
    let merged_coverage = merge_coverage_reports(discovery_coverage, &summary.coverage);
    drop(index);
    let roots = load_source_root_summaries_for_parser(
        &app_data_dir,
        C::parser_version(),
        C::provider_kind().source_environment_label(),
    )
    .await?;
    Ok(LocalScanOutput {
        files_scanned: summary.files_scanned,
        call_count: summary.aggregate.call_count,
        coverage: merged_coverage,
        roots,
    })
}
