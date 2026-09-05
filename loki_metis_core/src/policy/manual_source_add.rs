//! 手动添加数据根：先核对所选路径，失败再决定子树深搜。

/// 所选目录的结构核对结果（由适配器映射自各客户端 `inspect_root`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualSourceInspectOutcome {
    /// 所选路径本身已是当前客户端合格根。
    Found,
    /// 确认不是合格根。
    NotRoot,
    /// 确认结构签名拒绝。
    Rejected,
    /// 无法得出确定结论。
    Indeterminate,
    /// 核对过程被取消。
    Cancelled,
    /// 核对预算耗尽。
    BudgetExhausted,
}

/// 手动添加在核对之后应执行的下一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualSourceAddDecision {
    /// 幂等登记所选路径。
    Register,
    /// 以所选路径为唯一搜索根做有界深搜。
    DeepSearch,
    /// 用户取消；不登记、不深搜。
    AbortCancelled,
}

/// 根据核对结果决定登记、深搜或取消中止。
pub const fn decide_manual_source_add(
    outcome: ManualSourceInspectOutcome,
) -> ManualSourceAddDecision {
    match outcome {
        ManualSourceInspectOutcome::Found => ManualSourceAddDecision::Register,
        ManualSourceInspectOutcome::NotRoot
        | ManualSourceInspectOutcome::Rejected
        | ManualSourceInspectOutcome::Indeterminate
        | ManualSourceInspectOutcome::BudgetExhausted => ManualSourceAddDecision::DeepSearch,
        ManualSourceInspectOutcome::Cancelled => ManualSourceAddDecision::AbortCancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证直接核对成功时立即登记而不进入子树深搜。
    fn found_registers_without_deep_search() {
        assert_eq!(
            decide_manual_source_add(ManualSourceInspectOutcome::Found),
            ManualSourceAddDecision::Register
        );
    }

    #[test]
    /// 验证无法核对的目录会进入受控子树深搜。
    fn unverified_outcomes_deep_search() {
        for outcome in [
            ManualSourceInspectOutcome::NotRoot,
            ManualSourceInspectOutcome::Rejected,
            ManualSourceInspectOutcome::Indeterminate,
            ManualSourceInspectOutcome::BudgetExhausted,
        ] {
            assert_eq!(
                decide_manual_source_add(outcome),
                ManualSourceAddDecision::DeepSearch
            );
        }
    }

    #[test]
    /// 验证用户取消后终止手动添加流程且不登记来源。
    fn cancelled_aborts() {
        assert_eq!(
            decide_manual_source_add(ManualSourceInspectOutcome::Cancelled),
            ManualSourceAddDecision::AbortCancelled
        );
    }
}
