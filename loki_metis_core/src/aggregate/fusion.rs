//! 业务范围内的可合并指标决策。

use thiserror::Error;

use crate::MetricFact;

/// 表示 provider 融合试图跨业务范围合并的稳定错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MetricFusionError {
    /// 两个候选事实的 scope 不同，必须分区展示。
    #[error("metric scopes must not be merged")]
    ScopeMismatch,
}

/// 在相同 scope 内选择较新的本机事实，跨 scope 时明确拒绝。
pub fn prefer_metric_within_scope<T>(
    left: MetricFact<T>,
    right: MetricFact<T>,
) -> Result<MetricFact<T>, MetricFusionError> {
    // 左右 scope 必须一致，避免设备、根与线程口径混用。
    if left.scope != right.scope {
        return Err(MetricFusionError::ScopeMismatch);
    }

    if left.observed_at_epoch_ms >= right.observed_at_epoch_ms {
        Ok(left)
    } else {
        Ok(right)
    }
}
