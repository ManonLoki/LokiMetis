//! 看板相关设置查询和写入命令；界面语言与宿主能力仍由既有 host-settings 路径负责。

use tauri::State;

use crate::dto::{PrivacySettingsDto, UsageClientKindDto};
use crate::runtime::AppRuntimeState;

/// 读取扫描间隔、清理天数与已开放 Agent。
#[tauri::command]
pub(crate) async fn get_privacy_settings(
    state: State<'_, AppRuntimeState>,
    client: UsageClientKindDto,
) -> Result<PrivacySettingsDto, String> {
    Ok(state.privacy_settings(client).await)
}

/// 保存单一扫描间隔；不触发本机扫描。
#[tauri::command]
pub(crate) async fn set_scan_interval(
    state: State<'_, AppRuntimeState>,
    client: UsageClientKindDto,
    minutes: u16,
) -> Result<PrivacySettingsDto, String> {
    state.set_scan_interval(minutes).await?;
    Ok(state.privacy_settings(client).await)
}

/// 保存派生用量自动清理天数；保存不立刻清数据。
#[tauri::command]
pub(crate) async fn set_retention_days(
    state: State<'_, AppRuntimeState>,
    client: UsageClientKindDto,
    days: u16,
) -> Result<PrivacySettingsDto, String> {
    state.set_retention_days(days).await?;
    Ok(state.privacy_settings(client).await)
}
