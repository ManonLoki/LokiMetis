//! 实现看板查询、显式设置、刷新、扫描取消与本产品索引清理 commands。

mod access;
mod periodic_scan;
mod retention_cleanup;
mod root_discovery;
mod scan;
mod scan_orchestration;
mod settings;
mod source_reindex;
mod sources;
mod usage;
mod workbuddy;

pub(crate) use access::ensure_business_access;
pub(crate) use periodic_scan::spawn_periodic_local_scans;
pub(crate) use retention_cleanup::spawn_retention_cleanup;
pub(crate) use root_discovery::{
    ROOT_DISCOVERY_CANDIDATE_EVENT, add_root_candidate, cancel_root_discovery, candidate_to_dto,
    get_local_scan_status, get_root_discovery_status, list_root_candidates, start_root_discovery,
    to_status_dto as root_discovery_status_dto,
};
pub(crate) use scan::{
    clear_local_index, refresh_indexes_requiring_upgrade, refresh_local_indexes,
    run_periodic_quick_scans,
};
pub(crate) use settings::{
    get_privacy_settings, set_device_username, set_enabled_agents, set_retention_days,
    set_scan_interval, set_workbuddy_stats_enabled,
};
pub(crate) use source_reindex::reindex_source_root;
pub(crate) use sources::{get_source_roots, get_sources};
pub(crate) use usage::{
    get_usage_calls, get_usage_charts, get_usage_overview, get_usage_statistics,
};
pub(crate) use workbuddy::{
    get_workbuddy_source_status, get_workbuddy_statistics, get_workbuddy_usage_statistics,
};

#[cfg(test)]
mod tests;
