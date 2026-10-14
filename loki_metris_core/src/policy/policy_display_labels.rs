// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：校验模型标签安全可展示后返回去除首尾空白的副本，否则返回 `None`。；在展示路径中允许直接透传的数据根别名。；在分页或列表显示上保留内部标识短尾，避免完整 ID 外泄。

/// 校验模型标签安全可展示后返回去除首尾空白的副本，否则返回 `None`。
/// 各适配器共用同一条规则，集中一份避免两侧各自漂移。
pub fn safe_model_label(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()
        && trimmed.len() <= 128
        && !trimmed.contains(['/', '\\'])
        && !trimmed.chars().any(char::is_control))
    .then(|| trimmed.to_owned())
}

/// 在展示路径中允许直接透传的数据根别名。
pub fn safe_root_label(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > 64
        || value.chars().any(char::is_control)
        || value.contains('/')
        || value.contains('\\')
    {
        None
    } else {
        Some(value.to_owned())
    }
}

/// 在分页或列表显示上保留内部标识短尾，避免完整 ID 外泄。
pub fn safe_short_value(key: &str) -> String {
    key.chars()
        .rev()
        .take(8)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

/// 将覆盖覆盖状态映射为展示完整度：仅完整扫描为 Complete，其余为 Partial。
pub const fn coverage_completeness(state: CoverageState) -> Completeness {
    match state {
        CoverageState::Complete => Completeness::Complete,
        CoverageState::Partial | CoverageState::Cancelled | CoverageState::Failed => {
            Completeness::Partial
        }
    }
}

/// 合并已确认/全设备发现覆盖报告与扫描覆盖报告。
pub fn merge_coverage_reports(discovery: CoverageReport, scan: &CoverageReport) -> CoverageReport {
    let state = if discovery.state == CoverageState::Cancelled
        || scan.state == CoverageState::Cancelled
    {
        CoverageState::Cancelled
    } else if discovery.state == CoverageState::Failed || scan.state == CoverageState::Failed {
        CoverageState::Failed
    } else if discovery.state == CoverageState::Partial || scan.state == CoverageState::Partial {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };
    CoverageReport {
        state,
        roots_scanned: discovery.roots_scanned.max(scan.roots_scanned),
        roots_discovered: discovery.roots_discovered,
        permission_denied_count: discovery
            .permission_denied_count
            .saturating_add(scan.permission_denied_count),
        skipped_count: discovery.skipped_count.saturating_add(scan.skipped_count),
        warning_count: discovery.warning_count.saturating_add(scan.warning_count),
    }
}

/// 单根重建没有复核其余根，只能保守扩展旧覆盖，不能清除全客户端的既有缺口。
pub fn extend_coverage_with_partial_scan(previous:CoverageReport,scan:&CoverageReport)->CoverageReport {
    let mut result=merge_coverage_reports(previous.clone(),scan);
    result.roots_discovered=previous.roots_discovered.max(scan.roots_discovered);
    result.permission_denied_count=previous.permission_denied_count.max(scan.permission_denied_count);
    result.skipped_count=previous.skipped_count.max(scan.skipped_count);
    result.warning_count=previous.warning_count.max(scan.warning_count);
    result
}

/// 返回清空索引后不声称已完整覆盖的默认覆盖报告。
pub fn empty_coverage() -> CoverageReport {
    CoverageReport {
        state: CoverageState::Partial,
        roots_scanned: 0,
        roots_discovered: 0,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    }
}

#[cfg(test)]
mod policy_tests;
