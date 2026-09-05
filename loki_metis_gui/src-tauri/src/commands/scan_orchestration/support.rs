use crate::backend::local_index::{
    FullDiscoveryOptions, LocalError, LocalVolumeRoots, enumerate_local_volume_roots,
};
use loki_metis_core::{
    append_platform_discovery_excludes_for_volumes, local_scan_error_message_from,
    local_scan_task_error_message as core_local_scan_task_error_message,
};

/// 按当前平台卷发现约束组装全设备主动发现边界（仅应在 spawn_blocking 岛内调用）。
pub(crate) fn full_device_options() -> FullDiscoveryOptions {
    full_device_options_from_local_volume_roots(enumerate_local_volume_roots())
}

/// 不限目录数量时返回零，让进度保持不确定而不是伪造百分比上限。
pub(crate) fn codex_full_device_progress_max_directories() -> u64 {
    0
}

/// Claude 全盘进度上限：Default 预算目录数（卷枚举不改预算数字）。
pub(crate) fn claude_full_device_progress_max_directories() -> u64 {
    0
}

/// 把已分类卷装配为可注入测试的发现边界，不读取任何卷内容。
pub(crate) fn full_device_options_from_local_volume_roots(
    volumes: LocalVolumeRoots,
) -> FullDiscoveryOptions {
    let mut excluded_roots = volumes.excluded_roots;
    excluded_roots =
        append_platform_discovery_excludes_for_volumes(excluded_roots, &volumes.search_roots);

    FullDiscoveryOptions {
        search_roots: volumes.search_roots,
        excluded_roots,
        preflight_network_skipped_count: volumes.network_skipped_count,
        preflight_other_skipped_count: volumes.other_skipped_count,
        ..FullDiscoveryOptions::default()
    }
}

/// 折叠 blocking worker 或本机索引失败，避免暴露绝对路径和 SQL。
pub(crate) fn local_scan_task_error_message() -> String {
    core_local_scan_task_error_message().to_owned()
}

/// 把本机错误类别映射为稳定中文标签，同时丢弃路径、SQL 与原始记录细节。
pub(crate) fn local_scan_error_message_from_local(error: LocalError) -> String {
    local_scan_error_message_from(&error)
}
