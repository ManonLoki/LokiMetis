//! 周期扫描触发门禁（共享给 GUI、CLI、MCP 的公共策略）。

use thiserror::Error;

use super::SourceClientKind;

/// 表示周期扫描触发门禁拒绝原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PeriodicQuickScanError {
    /// 初始化未完成时不允许任何后台扫描。
    #[error("initialization has not completed")]
    InitializationRequired,
    /// 客户端当前仍有扫描任务时不应重复认领。
    #[error("scan already in progress")]
    ScanAlreadyRunning,
}

/// 统一判断是否允许周期快速扫描（用于 CLI/MCP 与 GUI 共用）。
pub fn ensure_periodic_quick_scan_allowed(
    initialization_completed: bool,
    scan_running: bool,
) -> Result<(), PeriodicQuickScanError> {
    if !initialization_completed {
        Err(PeriodicQuickScanError::InitializationRequired)
    } else if scan_running {
        Err(PeriodicQuickScanError::ScanAlreadyRunning)
    } else {
        Ok(())
    }
}

/// 一次共享周期节拍应采取的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeriodicScanTick {
    /// 没有已开放客户端，本拍不扫描。
    Skip,
    /// 共享 writer 正忙或所有已开放客户端都在扫描，本拍不新增任务。
    Wait,
    /// 在同一个 writer 许可内按固定顺序依次扫描这些空闲客户端。
    Start {
        /// 本拍要顺序执行的已开放且空闲的客户端。
        clients: Vec<SourceClientKind>,
    },
}

/// 决定一次节拍要扫描哪些客户端：每个已开放且空闲的客户端都在本拍被扫描一次。
///
/// 早先“每拍只轮转一个客户端”会让每个客户端的实际刷新周期被放大为
/// `间隔 × 客户端数`，且进程重启后游标归零会反复优先 Codex；这里改为一拍
/// 覆盖全部客户端，adapter 在同一个 writer 许可内串行执行，仍然只有一个索引 worker。
pub fn decide_periodic_scan_tick(
    enabled_in_order: &[SourceClientKind],
    client_running: impl Fn(SourceClientKind) -> bool,
    writer_busy: bool,
) -> PeriodicScanTick {
    if enabled_in_order.is_empty() {
        return PeriodicScanTick::Skip;
    }
    if writer_busy {
        return PeriodicScanTick::Wait;
    }
    let clients: Vec<SourceClientKind> = enabled_in_order
        .iter()
        .copied()
        .filter(|client| !client_running(*client))
        .collect();
    if clients.is_empty() {
        PeriodicScanTick::Wait
    } else {
        PeriodicScanTick::Start { clients }
    }
}
