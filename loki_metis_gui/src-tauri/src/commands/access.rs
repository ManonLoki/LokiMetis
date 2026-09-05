use crate::dto::ScanKindDto;
use crate::runtime::AppRuntimeState;

use loki_metis_core::{
    ScanKind as CoreScanKind, ScanStartAccessError, ScanStartOrigin, ensure_scan_start_allowed,
    scan_start_access_error_message,
};

/// 看板读取不接源项目向导门禁；Agent 已开启时允许概览、用量、数据源与调用。
pub(crate) async fn ensure_business_access(_state: &AppRuntimeState) -> Result<(), String> {
    Ok(())
}

/// 扫描触发前的统一策略门禁；无向导时仍允许显式与周期快速扫描。
pub(crate) async fn ensure_scan_start_access_by_policy(
    _state: &AppRuntimeState,
    origin: ScanStartOrigin,
    kind: ScanKindDto,
) -> Result<(), String> {
    let scan_kind = match kind {
        ScanKindDto::Quick => CoreScanKind::Quick,
        ScanKindDto::FullDevice => CoreScanKind::FullDevice,
    };
    ensure_scan_start_allowed(origin, true, scan_kind)
        .map_err(|error: ScanStartAccessError| scan_start_access_error_message(error).to_owned())
}
