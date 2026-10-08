//! 用量聚合与去重规则。

mod aggregation;
mod canonicalization;
mod filter;
mod fusion;
#[cfg(test)]
mod tests;

pub use aggregation::aggregate_canonical_usage;
pub use canonicalization::{
    CanonicalUsageSet, CanonicalizationWarning, CanonicalizationWarningKind,
    attach_session_snapshots, canonicalize_session_snapshots, canonicalize_usage_calls,
};
pub use filter::{copy_matching_snapshots, filter_canonical_usage, partition_canonical_usage_two};
pub use fusion::{MetricFusionError, prefer_metric_within_scope};

pub(crate) use canonicalization::lower_confidence;
