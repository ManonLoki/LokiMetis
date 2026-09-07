//! 本机窗口汇总与来源根聚合的纯业务装配。

use crate::aggregate::copy_matching_snapshots;
use crate::{
    CanonicalUsageSet, Confidence, CoverageReport, LocalIndexState, LocalUsageAggregate,
    LocalUsageWindow, MetricFact, MetricScope, ProviderKind, SourceClientKind, TimeStandard,
    aggregate_canonical_usage, civil_date_for_timestamp, empty_local_usage_aggregate_for_provider,
    filter_canonical_usage, inclusive_calendar_range, local_fact_quality, safe_root_label,
};

/// 一组窗口的可展示事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRecordsSummary {
    /// 当前索引状态。
    pub index_state: LocalIndexState,
    /// 今日、昨日、本周、上周、本月和上月六个日历窗口。
    pub windows: Vec<WindowUsage>,
}

/// 单个窗口的可展示事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowUsage {
    /// 窗口键。
    pub window: LocalUsageWindow,
    /// 该窗口内的统计事实。
    pub fact: MetricFact<LocalUsageAggregate>,
}

/// 来源根记录输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRootInput {
    /// 来源根稳定 ID。
    pub id: String,
    /// 来源根别名。
    pub alias: String,
    /// 是否启用。
    pub enabled: bool,
    /// 首次索引激活状态。
    pub activation_state: crate::RootActivationState,
    /// 是否为主目录。
    pub is_primary: bool,
    /// 发现方式。
    pub discovery_method: SourceDiscoveryMethod,
    /// 该根的 source 文件计数。
    pub source_file_count: u64,
    /// 该根当前 generation 的 source observe 总数。
    pub call_observation_count: u64,
}

/// 来源根展示摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRootSummary {
    /// 来源根 ID。
    pub id: String,
    /// 展示别名。
    pub alias: String,
    /// 是否启用。
    pub enabled: bool,
    /// 首次索引激活状态。
    pub activation_state: crate::RootActivationState,
    /// 是否主目录。
    pub is_primary: bool,
    /// 来源展示文案。
    pub discovery_label: String,
    /// 来源稳定代码。
    pub discovery_code: SourceDiscoveryCode,
    /// 该根文件数量。
    pub file_count: u64,
    /// 跳过计数（当前边界暂不上报）。
    pub skipped_count: u64,
    /// 错误计数（当前边界暂不上报）。
    pub error_count: u64,
    /// 去重来源计数。
    pub duplicate_count: u64,
}

/// 来源发现稳定代码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceDiscoveryCode {
    /// 默认数据根。
    DefaultRoot,
    /// Codex 环境变量来源。
    CodexEnvironment,
    /// Claude 环境变量来源。
    ClaudeEnvironment,
    /// Grok 环境变量来源。
    GrokEnvironment,
    /// 用户手工登记。
    UserRegistered,
    /// 全设备发现。
    FullDevice,
    /// 元数据发现确认。
    MetadataDiscovery,
}

/// 来源发现方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceDiscoveryMethod {
    /// 默认路径。
    DefaultHome,
    /// 环境变量覆盖来源。
    Environment,
    /// 用户登记。
    Registered,
    /// 全设备发现。
    FullDevice,
    /// 元数据发现确认。
    MetadataDiscovery,
}

/// 组装固定四窗口的本机窗口数据。
pub fn build_local_windows(
    canonical: &CanonicalUsageSet,
    coverage: &CoverageReport,
    index_state: LocalIndexState,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
) -> Result<LocalRecordsSummary, String> {
    build_local_windows_with_standard(
        canonical,
        coverage,
        index_state,
        observed_at_epoch_ms,
        provider,
        source_version,
        TimeStandard::Local,
        &jiff::tz::TimeZone::system(),
    )
}

/// 按已保存时间标准和设备时区组装固定四窗口的本机窗口数据。
#[allow(clippy::too_many_arguments)]
pub fn build_local_windows_with_standard(
    canonical: &CanonicalUsageSet,
    coverage: &CoverageReport,
    index_state: LocalIndexState,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<LocalRecordsSummary, String> {
    // 每个窗口过滤后的调用都可能为空，此时聚合会退回 `empty_tokens`
    // 表达的零值形状；这里预先按当前 provider 订正，避免复用推断自
    // 历史调用、可能不匹配当前 provider 的默认形状。
    //
    // 联合视图下 `canonical.empty_tokens` 是跨 Agent 声明能力的交集（见
    // `combined_empty_tokens`），可能比某个窗口实际观测到的分项更保守
    // （例如本窗口只有 Codex 有调用，但 Claude 不支持 reasoning_output_
    // tokens 会把交集降为“未提供”）。这里之所以不需要像
    // `statistics_view::build_usage_statistics_with_window` 那样在聚合后
    // 用 `aggregate.tokens.zero_matching_availability()` 再订正一次，是
    // 因为本函数每个窗口只产出一份扁平事实、不做小时/分组桶分解：
    // 只要 `window_calls.calls` 非空，`aggregate_canonical_usage` 就直接
    // 从真实调用折叠出形状，压根不读 `empty_tokens`；只有窗口调用为空
    // 时才会退回这里的交集形状，而那正是唯一合理的形状。若未来给窗口
    // 事实加上桶/分组分解，必须补上同样的聚合后订正，否则空桶会重新
    // 继承这个交集形状。
    let empty_tokens = if provider == ProviderKind::CombinedLocalAgents {
        canonical.empty_tokens.clone()
    } else {
        crate::empty_token_usage_for_provider(provider)
    };

    const WINDOW_AGGREGATION_ERROR: &str = "本机窗口无法聚合。";

    // 六个日历窗口彼此重叠（例如今日调用同时落在本周与本月里）：逐窗口各自
    // 重新把每条调用/快照的发生时刻换算成民用日，会让同一条记录被换算最多
    // 六次。这里先单次遍历换算出每条记录的民用日缓存下来，六个窗口再各自
    // 按闭区间边界复用同一份结果，时区换算总量与窗口数无关。
    let today = civil_date_for_timestamp(observed_at_epoch_ms, &time_standard, device_tz)
        .ok_or_else(|| WINDOW_AGGREGATION_ERROR.to_owned())?;
    let call_dates = canonical
        .calls
        .iter()
        .map(|call| civil_date_for_timestamp(call.occurred_at_epoch_ms, &time_standard, device_tz))
        .collect::<Vec<_>>();
    let snapshot_dates = canonical
        .snapshots
        .iter()
        .map(|snapshot| {
            civil_date_for_timestamp(snapshot.occurred_at_epoch_ms, &time_standard, device_tz)
        })
        .collect::<Vec<_>>();

    let windows = LocalUsageWindow::OVERVIEW_WINDOWS
        .into_iter()
        .map(|window| {
            let (from, to) = inclusive_calendar_range(window, today)
                .map_err(|_| WINDOW_AGGREGATION_ERROR.to_owned())?;
            let mut call_dates_iter = call_dates.iter();
            let filtered = filter_canonical_usage(canonical, |_call| {
                call_dates_iter
                    .next()
                    .copied()
                    .flatten()
                    .is_some_and(|date| date >= from && date <= to)
            });
            let mut snapshot_dates_iter = snapshot_dates.iter();
            let mut window_calls = copy_matching_snapshots(canonical, filtered, |_snapshot| {
                snapshot_dates_iter
                    .next()
                    .copied()
                    .flatten()
                    .is_some_and(|date| date >= from && date <= to)
            });
            window_calls.empty_tokens = empty_tokens.clone();
            window_calls.total_token_accounting = provider.local_total_token_accounting();
            let aggregate = aggregate_canonical_usage(&window_calls)
                .map_err(|_| WINDOW_AGGREGATION_ERROR.to_owned())?;
            let (freshness, completeness, confidence) =
                local_fact_quality(index_state, coverage.state, aggregate.confidence);
            Ok(WindowUsage {
                window,
                fact: MetricFact::new(
                    aggregate,
                    provider,
                    MetricScope::DeviceObserved,
                    observed_at_epoch_ms,
                    freshness,
                    completeness,
                    confidence,
                    source_version.map(ToOwned::to_owned),
                ),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(LocalRecordsSummary {
        index_state,
        windows,
    })
}

/// 组装无数据且未扫描窗口的稳定空事实。
pub fn build_empty_local_windows(
    coverage: &CoverageReport,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
) -> LocalRecordsSummary {
    let index_state = LocalIndexState::NotScanned;
    let (freshness, completeness, _) =
        local_fact_quality(index_state, coverage.state, Confidence::Exact);
    let empty = empty_local_usage_aggregate_for_provider(provider);

    let windows = LocalUsageWindow::OVERVIEW_WINDOWS
        .into_iter()
        .map(|window| WindowUsage {
            window,
            fact: MetricFact::new(
                empty.clone(),
                provider,
                MetricScope::DeviceObserved,
                observed_at_epoch_ms,
                freshness,
                completeness,
                Confidence::Derived,
                source_version.map(ToOwned::to_owned),
            ),
        })
        .collect();

    LocalRecordsSummary {
        index_state,
        windows,
    }
}

/// 将来源根输入与 canonical 映射为可展示摘要。
pub fn build_source_roots(
    records: &[SourceRootInput],
    canonical: &CanonicalUsageSet,
    environment_label: &'static str,
) -> Vec<SourceRootSummary> {
    let call_counts_by_root = call_counts_by_root(canonical);
    build_source_roots_with_counts(records, &call_counts_by_root, environment_label)
}

/// 直接使用本机索引的有界 SQL 计数组装来源根摘要，不装载 canonical 调用实体。
pub fn build_indexed_source_roots(
    records: &[crate::local_index::RootUsageSummaryRecord],
    environment_label: &'static str,
) -> Vec<SourceRootSummary> {
    let inputs = records
        .iter()
        .map(|record| SourceRootInput {
            id: record.root.root_id.clone(),
            alias: record.root.alias.clone(),
            enabled: record.root.enabled,
            activation_state: record.root.activation_state,
            is_primary: record.root.is_primary,
            discovery_method: indexed_discovery_method(record.root.discovery_method),
            source_file_count: record.root.source_file_count,
            call_observation_count: record.root.call_observation_count,
        })
        .collect::<Vec<_>>();
    let call_counts_by_root = records
        .iter()
        .map(|record| (record.root.root_id.as_str(), record.canonical_call_count))
        .collect::<std::collections::HashMap<_, _>>();
    build_source_roots_with_counts(&inputs, &call_counts_by_root, environment_label)
}

/// 使用已计算的按根 canonical 计数组装展示摘要。
fn build_source_roots_with_counts(
    records: &[SourceRootInput],
    call_counts_by_root: &std::collections::HashMap<&str, u64>,
    environment_label: &'static str,
) -> Vec<SourceRootSummary> {
    records
        .iter()
        .map(|record| {
            let canonical_call_count = call_counts_by_root
                .get(record.id.as_str())
                .copied()
                .unwrap_or(0);
            let duplicate_count = if record.enabled {
                record
                    .call_observation_count
                    .saturating_sub(canonical_call_count)
            } else {
                0
            };

            SourceRootSummary {
                id: record.id.clone(),
                alias: safe_root_label(&record.alias).unwrap_or_else(|| "未命名数据根".to_owned()),
                enabled: record.enabled,
                activation_state: record.activation_state,
                is_primary: record.is_primary,
                discovery_label: discovery_label(record.discovery_method, environment_label)
                    .to_owned(),
                discovery_code: discovery_code(record.discovery_method, environment_label),
                file_count: record.source_file_count,
                skipped_count: 0,
                error_count: 0,
                duplicate_count,
            }
        })
        .collect()
}

/// 把索引 registry 的发现方式映射为来源摘要使用的稳定业务枚举。
const fn indexed_discovery_method(
    method: crate::local_index::DiscoveryMethod,
) -> SourceDiscoveryMethod {
    match method {
        crate::local_index::DiscoveryMethod::DefaultHome => SourceDiscoveryMethod::DefaultHome,
        crate::local_index::DiscoveryMethod::Environment => SourceDiscoveryMethod::Environment,
        crate::local_index::DiscoveryMethod::Registered => SourceDiscoveryMethod::Registered,
        crate::local_index::DiscoveryMethod::FullDevice => SourceDiscoveryMethod::FullDevice,
        crate::local_index::DiscoveryMethod::MetadataDiscovery => {
            SourceDiscoveryMethod::MetadataDiscovery
        }
    }
}

/// 单遍扫描规范集合，按数据根 id 统计调用数，避免每个数据根各扫一遍全量调用。
fn call_counts_by_root(canonical: &CanonicalUsageSet) -> std::collections::HashMap<&str, u64> {
    let mut counts = std::collections::HashMap::new();
    for call in &canonical.calls {
        let mut counted_roots = std::collections::HashSet::new();
        for source in &call.provenance {
            if counted_roots.insert(source.root_id.as_str()) {
                *counts.entry(source.root_id.as_str()).or_insert(0u64) += 1;
            }
        }
    }
    counts
}

/// 为数据根发现方式选择不含路径的展示标签。
fn discovery_label(method: SourceDiscoveryMethod, environment_label: &'static str) -> &'static str {
    match method {
        SourceDiscoveryMethod::DefaultHome => "默认数据根",
        // 调用方已经按当前 provider 解析出准确文案，直接透传，无需重新猜测。
        SourceDiscoveryMethod::Environment => environment_label,
        SourceDiscoveryMethod::Registered => "用户登记",
        SourceDiscoveryMethod::FullDevice => "全设备发现",
        SourceDiscoveryMethod::MetadataDiscovery => "元数据发现",
    }
}

// 只能靠比较文案字符串区分 Claude/Codex 的 Environment 发现方式（Codex 是
// 隐式 else 分支），因为这里没有直接拿到 provider 枚举；若任一 provider 的
// source_environment_label() 文案改变，这里的分类会跟着悄悄错位，需要同步检查。
/// 将发现方式与环境标签映射为稳定的来源代码。
fn discovery_code(method: SourceDiscoveryMethod, environment_label: &str) -> SourceDiscoveryCode {
    match method {
        SourceDiscoveryMethod::DefaultHome => SourceDiscoveryCode::DefaultRoot,
        SourceDiscoveryMethod::Environment
            if environment_label == SourceClientKind::ClaudeCode.source_environment_label() =>
        {
            SourceDiscoveryCode::ClaudeEnvironment
        }
        SourceDiscoveryMethod::Environment
            if environment_label == SourceClientKind::GrokBuildCli.source_environment_label() =>
        {
            SourceDiscoveryCode::GrokEnvironment
        }
        SourceDiscoveryMethod::Environment => SourceDiscoveryCode::CodexEnvironment,
        SourceDiscoveryMethod::Registered => SourceDiscoveryCode::UserRegistered,
        SourceDiscoveryMethod::FullDevice => SourceDiscoveryCode::FullDevice,
        SourceDiscoveryMethod::MetadataDiscovery => SourceDiscoveryCode::MetadataDiscovery,
    }
}
