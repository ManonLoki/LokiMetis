//! 看板相关设置查询和写入命令；界面语言与宿主能力仍由既有 host-settings 路径负责。

use tauri::State;

use crate::dto::{PrivacySettingsDto, UsageClientKindDto};
use crate::runtime::AppRuntimeState;

/// 读取设备身份、扫描间隔、清理天数与已开放 Agent。
#[tauri::command]
pub(crate) async fn get_privacy_settings(
    state: State<'_, AppRuntimeState>,
    client: UsageClientKindDto,
) -> Result<PrivacySettingsDto, String> {
    Ok(state.privacy_settings(client).await)
}

/// 保存或清除设备用户名；系统会话候选只在首次设置加载时初始化一次。
#[tauri::command]
pub(crate) async fn set_device_username(
    state: State<'_, AppRuntimeState>,
    client: UsageClientKindDto,
    device_username: String,
) -> Result<PrivacySettingsDto, String> {
    state.set_device_username(device_username).await?;
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

/// 保存用户显式开放的本机 Agent 集合。
#[tauri::command]
pub(crate) async fn set_enabled_agents(
    state: State<'_, AppRuntimeState>,
    client: UsageClientKindDto,
    agents: Vec<UsageClientKindDto>,
) -> Result<PrivacySettingsDto, String> {
    state.set_enabled_agents(&agents).await?;
    Ok(state.privacy_settings(client).await)
}

/// 保存 WorkBuddy 本地统计开关；关闭时后续统计读取命令必须拒绝返回数据。
#[tauri::command]
pub(crate) async fn set_workbuddy_stats_enabled(
    state: State<'_, AppRuntimeState>,
    client: UsageClientKindDto,
    enabled: bool,
) -> Result<PrivacySettingsDto, String> {
    state.set_workbuddy_stats_enabled(enabled).await?;
    Ok(state.privacy_settings(client).await)
}
