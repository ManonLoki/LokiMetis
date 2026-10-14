//! 扫描准入：一次扫描请求覆盖哪些 Agent，以及遇到共享 writer 忙碌、单个 Agent 失败时如何处理。

use super::ScanStartOrigin;
use crate::SourceClientKind;

/// 共享索引 writer 正忙时，一次扫描请求应采取的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyWriterAction {
    /// 放弃本次请求；下一次节拍会重新尝试。
    Skip,
    /// 排队等待 writer 释放后执行。
    Wait,
}

impl ScanStartOrigin {
    /// 是否为后台自动触发；自动扫描只允许覆盖用户已开放的 Agent。
    pub const fn is_automatic(self) -> bool {
        !matches!(self, Self::ExplicitUser)
    }

    /// 周期节拍遇 writer 忙碌直接跳过，避免积压；用户与初始化触发必须等待执行。
    pub const fn busy_writer_action(self) -> BusyWriterAction {
        match self {
            Self::PeriodicAutomatic => BusyWriterAction::Skip,
            Self::ExplicitUser | Self::InitialAutomatic => BusyWriterAction::Wait,
        }
    }

    /// 周期扫描中单个 Agent 失败只记录错误并继续下一个；其它触发立即以失败结束。
    pub const fn continues_after_client_failure(self) -> bool {
        matches!(self, Self::PeriodicAutomatic)
    }
}

/// 决定一次扫描请求覆盖的 Agent，顺序沿用已开放列表或请求本身。
///
/// - 指定了 Agent 时只扫描它；自动触发还要求它仍处于开放状态，用户显式触发则不受限。
/// - 未指定时覆盖全部已开放 Agent。
pub fn scan_target_clients(
    origin: ScanStartOrigin,
    requested: Option<SourceClientKind>,
    enabled_in_order: &[SourceClientKind],
) -> Vec<SourceClientKind> {
    match requested {
        Some(client) if origin.is_automatic() && !enabled_in_order.contains(&client) => Vec::new(),
        Some(client) => vec![client],
        None => enabled_in_order.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENABLED: [SourceClientKind; 2] = [SourceClientKind::Codex, SourceClientKind::ClaudeCode];

    /// 未指定 Agent 时按已开放顺序覆盖全部；无开放 Agent 时没有扫描目标。
    #[test]
    fn unspecified_request_covers_enabled_agents_in_order() {
        for origin in [
            ScanStartOrigin::ExplicitUser,
            ScanStartOrigin::InitialAutomatic,
            ScanStartOrigin::PeriodicAutomatic,
        ] {
            assert_eq!(scan_target_clients(origin, None, &ENABLED), ENABLED.to_vec());
            assert!(scan_target_clients(origin, None, &[]).is_empty());
        }
    }

    /// 显式请求可以扫描未开放的 Agent，自动触发不得越过开放边界。
    #[test]
    fn only_explicit_requests_may_target_a_disabled_agent() {
        let disabled = SourceClientKind::GrokBuildCli;
        assert_eq!(
            scan_target_clients(ScanStartOrigin::ExplicitUser, Some(disabled), &ENABLED),
            vec![disabled]
        );
        for origin in [ScanStartOrigin::InitialAutomatic, ScanStartOrigin::PeriodicAutomatic] {
            assert!(scan_target_clients(origin, Some(disabled), &ENABLED).is_empty());
            assert_eq!(
                scan_target_clients(origin, Some(SourceClientKind::Codex), &ENABLED),
                vec![SourceClientKind::Codex]
            );
        }
    }

    /// 只有周期节拍在 writer 忙碌时跳过，并在单个 Agent 失败后继续。
    #[test]
    fn periodic_ticks_skip_busy_writer_and_continue_after_failure() {
        assert_eq!(
            ScanStartOrigin::PeriodicAutomatic.busy_writer_action(),
            BusyWriterAction::Skip
        );
        assert!(ScanStartOrigin::PeriodicAutomatic.continues_after_client_failure());
        for origin in [ScanStartOrigin::ExplicitUser, ScanStartOrigin::InitialAutomatic] {
            assert_eq!(origin.busy_writer_action(), BusyWriterAction::Wait);
            assert!(!origin.continues_after_client_failure());
        }
        assert!(!ScanStartOrigin::ExplicitUser.is_automatic());
        assert!(ScanStartOrigin::InitialAutomatic.is_automatic());
    }
}
