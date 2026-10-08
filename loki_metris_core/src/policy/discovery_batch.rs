use crate::root_discovery::RootDiscoveryLifecycle;

/// 表示一次数据源发现批次对后续统一 Token 索引的稳定决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryBatchIndexDecision {
    /// 发现尚未达到终态，必须继续等待并禁止逐候选索引。
    Wait,
    /// 发现已完成、部分完成或取消，可以在候选登记收敛后统一索引一次。
    Index,
    /// 发现失败，不自动读取已登记根，等待用户显式重试。
    Skip,
}

/// 区分首次向导与已初始化数据源页对发现取消的不同处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryBatchKind {
    /// 首次向导必须完成发现与索引，取消后保持未初始化。
    Initialization,
    /// 已初始化用户显式发现；取消时仍统一索引取消前已登记的命中。
    ExplicitUser,
}

/// 根据发现生命周期决定是否允许启动本轮统一 Token 索引。
pub const fn discovery_batch_index_decision(
    kind: DiscoveryBatchKind,
    lifecycle: RootDiscoveryLifecycle,
) -> DiscoveryBatchIndexDecision {
    match lifecycle {
        RootDiscoveryLifecycle::Idle | RootDiscoveryLifecycle::Running => {
            DiscoveryBatchIndexDecision::Wait
        }
        RootDiscoveryLifecycle::Complete | RootDiscoveryLifecycle::Partial => {
            DiscoveryBatchIndexDecision::Index
        }
        RootDiscoveryLifecycle::Cancelled => match kind {
            DiscoveryBatchKind::Initialization => DiscoveryBatchIndexDecision::Skip,
            DiscoveryBatchKind::ExplicitUser => DiscoveryBatchIndexDecision::Index,
        },
        RootDiscoveryLifecycle::Failed => DiscoveryBatchIndexDecision::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 发现运行期间不得逐候选索引，完成、部分完成或取消后只开放统一索引。
    #[test]
    fn discovery_batch_index_waits_for_approved_terminal_states() {
        for lifecycle in [
            RootDiscoveryLifecycle::Idle,
            RootDiscoveryLifecycle::Running,
        ] {
            assert_eq!(
                discovery_batch_index_decision(DiscoveryBatchKind::ExplicitUser, lifecycle),
                DiscoveryBatchIndexDecision::Wait
            );
        }
        for lifecycle in [
            RootDiscoveryLifecycle::Complete,
            RootDiscoveryLifecycle::Partial,
        ] {
            for kind in [
                DiscoveryBatchKind::Initialization,
                DiscoveryBatchKind::ExplicitUser,
            ] {
                assert_eq!(
                    discovery_batch_index_decision(kind, lifecycle),
                    DiscoveryBatchIndexDecision::Index
                );
            }
        }
        assert_eq!(
            discovery_batch_index_decision(
                DiscoveryBatchKind::Initialization,
                RootDiscoveryLifecycle::Cancelled
            ),
            DiscoveryBatchIndexDecision::Skip
        );
        assert_eq!(
            discovery_batch_index_decision(
                DiscoveryBatchKind::ExplicitUser,
                RootDiscoveryLifecycle::Cancelled
            ),
            DiscoveryBatchIndexDecision::Index
        );
        for kind in [
            DiscoveryBatchKind::Initialization,
            DiscoveryBatchKind::ExplicitUser,
        ] {
            assert_eq!(
                discovery_batch_index_decision(kind, RootDiscoveryLifecycle::Failed),
                DiscoveryBatchIndexDecision::Skip
            );
        }
    }
}
