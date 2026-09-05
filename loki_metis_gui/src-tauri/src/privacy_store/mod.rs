//! 在本产品 app-data 中持久化最小本地设置；设备身份仅供本机展示和已保存的 CollectProvider 使用。

mod stable_device_id;
mod write;

pub(crate) use stable_device_id::read_stable_device_unique_id;

use std::fs;
use std::io::Read;
use std::path::Path;

use loki_metis_core::{
    DEFAULT_LOCAL_ONLY, DEFAULT_WORKBUDDY_STATS_ENABLED, DeviceName, DeviceUniqueId,
    DeviceUsername, EnabledAgents, RetentionDays, ScanIntervalMinutes, resolve_device_unique_id,
    resolve_retention_days, resolve_scan_interval_minutes, resolve_workbuddy_stats_enabled,
};
use serde::{Deserialize, Serialize};

use crate::dto::LanguagePreferenceDto;

/// 隐私设置持久化文件名，位于本产品 app-data 目录下。
const SETTINGS_FILE_NAME: &str = "privacy-settings.json";
/// Windows 原子写入使用的临时事务文件名。
#[cfg(windows)]
const SETTINGS_TRANSACTION_FILE_NAME: &str = "privacy-settings.transaction.json";
/// 设置文件允许的最大字节数，超出即拒绝读取。
const MAX_SETTINGS_BYTES: u64 = 128 * 1024;

/// 表示设置文件无法安全读取、校验或写入，不携带路径和原始个人值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrivacyStoreError;

/// 保存运行时需要的完整本地设置，所有写入都必须基于该快照避免字段互相覆盖。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalPrivacySettings {
    /// 当前界面语言偏好；系统语言的具体解析由前端完成。
    pub(crate) language_preference: LanguagePreferenceDto,
    /// 兼容字段：只保存仅本机偏好。
    pub(crate) local_only: bool,
    /// 用户手动设置的可选设备标签，不是系统用户名或唯一身份。
    pub(crate) device_username: Option<DeviceUsername>,
    /// 标识一次性系统会话用户名初始化已经完成或被用户显式清除。
    pub(crate) device_username_initialized: bool,
    /// 只读 OS 主机名快照，供设置页展示与已保存的 CollectProvider 使用。
    pub(crate) device_name: Option<DeviceName>,
    /// 已持久化的本机设备唯一 ID；缺失时优先用稳定设备 ID，否则生成。
    pub(crate) device_unique_id: Option<DeviceUniqueId>,
    /// 本机周期扫描与保留远端偏好共用的扫描间隔。
    pub(crate) scan_interval: ScanIntervalMinutes,
    /// 派生用量自动清理窗口；缺省 90 天。
    pub(crate) retention_days: RetentionDays,
    /// 本仓不引入源项目向导；看板读取始终视为已完成初始化。
    pub(crate) initialization_completed: bool,
    /// 标识首次打开的一次性自动快速扫描已经安排或被安全跳过。
    pub(crate) initial_scan_attempted: bool,
    /// 用户显式开放监控和上报的本机 Agent；缺省为空。
    pub(crate) enabled_agents: EnabledAgents,
    /// 用户是否显式开放读取 WorkBuddy 本地用量统计；缺省关闭。
    pub(crate) workbuddy_stats_enabled: bool,
}

impl Default for LocalPrivacySettings {
    /// 首次启动持久化仅本机偏好。
    fn default() -> Self {
        Self {
            language_preference: LanguagePreferenceDto::System,
            local_only: DEFAULT_LOCAL_ONLY,
            device_username: None,
            device_username_initialized: false,
            device_name: None,
            device_unique_id: None,
            scan_interval: ScanIntervalMinutes::default(),
            retention_days: RetentionDays::default(),
            initialization_completed: true,
            initial_scan_attempted: false,
            enabled_agents: EnabledAgents::empty(),
            workbuddy_stats_enabled: DEFAULT_WORKBUDDY_STATS_ENABLED,
        }
    }
}

impl LocalPrivacySettings {
    /// 设置损坏或 I/O 失败时保守禁用 provider，且禁止再用系统值覆盖损坏状态。
    pub(crate) fn safe_fallback() -> Self {
        Self {
            language_preference: LanguagePreferenceDto::System,
            local_only: true,
            device_username: None,
            device_username_initialized: true,
            device_name: None,
            device_unique_id: None,
            scan_interval: ScanIntervalMinutes::default(),
            retention_days: RetentionDays::default(),
            initialization_completed: true,
            initial_scan_attempted: true,
            enabled_agents: EnabledAgents::empty(),
            workbuddy_stats_enabled: DEFAULT_WORKBUDDY_STATS_ENABLED,
        }
    }
}

/// 定义向后兼容的磁盘 JSON；旧文件缺少初始化位时按已有用户名安全迁移。
// 这是运行时内存模型 LocalPrivacySettings 与磁盘 JSON 之间的“镜像结构体”，
// 两者故意分开：内存结构体的字段是已校验的强类型（DeviceUsername 等），
// 磁盘结构体的字段是宽松的原始类型（String/u16），专门负责应对旧版本
// 设置文件缺字段、字段类型不同等兼容性问题。
// `deny_unknown_fields`：JSON 里出现未声明的字段会直接反序列化失败，
// 防止将来意外把没设计过的字段静默丢弃或误解析。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredPrivacySettings {
    /// 当前界面语言偏好；旧设置默认继续跟随系统。
    // `#[serde(default)]`：字段在 JSON 中缺失时不报错，改用类型的
    // Default 实现填充——这是升级旧版本设置文件时最常用的兼容手段。
    #[serde(default)]
    language_preference: LanguagePreferenceDto,
    /// 兼容字段：只保存仅本机偏好。
    local_only: bool,
    /// 保存一次性初始化或用户显式提供的可选标签。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device_username: Option<String>,
    /// 标识初始化已经尝试；显式清除后仍保持 true，避免下次启动回填。
    #[serde(default)]
    device_username_initialized: Option<bool>,
    /// 最近一次刷新的 OS 主机名快照。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device_name: Option<String>,
    /// 首次生成后保持不变的设备唯一 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device_unique_id: Option<String>,
    /// 单一扫描间隔；缺失时再折叠旧双字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scan_interval_minutes: Option<u16>,
    /// 派生用量自动清理天数；缺失视为 90。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retention_days: Option<u16>,
    /// 旧远端刷新分钟；只读兼容，新格式不再写回。
    #[serde(default, skip_serializing)]
    remote_refresh_interval_minutes: Option<u16>,
    /// 旧本机扫描分钟；只读兼容，新格式不再写回。
    #[serde(default, skip_serializing)]
    local_scan_interval_minutes: Option<u16>,
    /// 新格式保存完整 Provider 集合；`None` 表示需要读取三个旧单配置字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    collect_providers: Option<Vec<StoredCollectProviderConfig>>,
    /// 旧单配置启停位只用于迁移，新格式不再写回。
    #[serde(default, skip_serializing)]
    collect_provider_enabled: bool,
    /// 旧单配置 BaseURL 只用于迁移，新格式不再写回。
    #[serde(default, skip_serializing)]
    collect_provider_base_url: Option<String>,
    /// 旧单配置周期间隔只用于迁移，新格式不再写回。
    #[serde(
        default = "default_collect_provider_interval_minutes",
        skip_serializing
    )]
    collect_provider_interval_minutes: u16,
    /// 旧页面工作状态只为兼容读取任意历史形状；规范写回时全部移除。
    #[serde(default, skip_serializing)]
    overview_window_codex: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    overview_window_claude_code: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    overview_window_grok_build_cli: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    usage_window_codex: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    usage_window_claude_code: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    usage_window_grok_build_cli: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    usage_dimension_codex: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    usage_dimension_claude_code: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    usage_dimension_grok_build_cli: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    chart_preferences: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    leaderboard_window: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    leaderboard_provider_id: Option<serde_json::Value>,
    /// 磁盘完成位；必须与本轮向导 Key 同时匹配才视为已完成。
    #[serde(default)]
    initialization_completed: bool,
    /// 完成时所写的向导合同 Key；缺失或过期时即使完成位为 true 也拉回向导。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    initialization_wizard_key: Option<String>,
    /// 首次自动快速扫描是否已经安排；旧设置在升级后自动尝试一次。
    #[serde(default)]
    initial_scan_attempted: bool,
    /// 用户显式开放的本机 Agent 线标；缺失视为全部关闭。
    #[serde(default)]
    enabled_agents: Option<Vec<String>>,
    /// 用户是否显式开放读取 WorkBuddy 本地用量统计；缺失视为关闭。
    #[serde(default)]
    workbuddy_stats_enabled: Option<bool>,
    /// 旧时间标准与最近 Agent 同属页面工作状态；仅兼容读取并在规范写回时移除。
    #[serde(default, skip_serializing)]
    time_standard: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    custom_time_zone: Option<serde_json::Value>,
    #[serde(default, skip_serializing)]
    last_selected_agent: Option<serde_json::Value>,
}

/// 定义单个 Provider 的磁盘 JSON 形状；所有字段加载后交给 core 重新校验。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCollectProviderConfig {
    /// 本地稳定 UUID。
    id: String,
    /// 旧多项配置的展示名称只用于兼容读取，当前格式不再写回。
    #[serde(default, skip_serializing)]
    name: Option<String>,
    /// 旧多项配置的启停位只用于兼容读取，当前格式不再写回。
    #[serde(default, skip_serializing)]
    enabled: Option<bool>,
    /// 规范化 BaseURL。
    base_url: String,
    /// 独立周期间隔分钟数。
    interval_minutes: u16,
    /// 该 Provider 的可选上报用户别名；未设置时不写入磁盘。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_alias: Option<String>,
    /// 旧客户端版本选择只用于兼容读取；当前格式始终上传且不再写回开关。
    #[serde(default, rename = "includeClientVersion", skip_serializing)]
    legacy_client_version_choice: Option<bool>,
}

/// 从本产品 app-data 读取并校验完整设置；文件不存在时返回安全的首次启动默认值。
pub(crate) fn load_settings(
    app_data_dir: &Path,
) -> Result<LocalPrivacySettings, PrivacyStoreError> {
    load_settings_with_migration_state(app_data_dir).map(|(settings, _)| settings)
}

/// 读取设置并标记旧用户名是否需要补写显式初始化位。
fn load_settings_with_migration_state(
    app_data_dir: &Path,
) -> Result<(LocalPrivacySettings, bool), PrivacyStoreError> {
    reject_symlink(app_data_dir)?;
    let settings_path = app_data_dir.join(SETTINGS_FILE_NAME);
    let primary = read_stored_settings(&settings_path);
    #[cfg(windows)]
    let stored = match primary {
        Ok(Some(stored)) => stored,
        Ok(None) => match read_stored_settings(&app_data_dir.join(SETTINGS_TRANSACTION_FILE_NAME))?
        {
            Some(stored) => return normalize_stored_settings(stored, true),
            None => return Ok((LocalPrivacySettings::default(), false)),
        },
        Err(_) => match read_stored_settings(&app_data_dir.join(SETTINGS_TRANSACTION_FILE_NAME))? {
            Some(stored) => return normalize_stored_settings(stored, true),
            None => return Err(PrivacyStoreError),
        },
    };
    #[cfg(not(windows))]
    let stored = match primary? {
        Some(stored) => stored,
        None => return Ok((LocalPrivacySettings::default(), false)),
    };

    normalize_stored_settings(stored, false)
}

/// 从单个设置或事务文件读取受限 JSON；缺失与损坏保持不同结果供恢复协议判断。
// 返回值区分三种情况：Ok(None) 表示文件确实不存在（合法的“首次启动”状态）；
// Ok(Some(_)) 表示成功读到合法设置；Err 表示文件存在但不可信
// （符号链接、不是普通文件、超出大小上限或 JSON 格式错误）——
// 调用方靠这三态区分“真的没有”和“有但坏了”，采取不同的恢复策略。
fn read_stored_settings(path: &Path) -> Result<Option<StoredPrivacySettings>, PrivacyStoreError> {
    // symlink_metadata（而不是 metadata）不会自动跟随符号链接，
    // 这样才能在下面显式检测并拒绝符号链接，防止设置文件路径被
    // 篡改指向应用无权访问或不该读写的位置。
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PrivacyStoreError),
    };
    if metadata_is_link_like(&metadata)
        || !metadata.is_file()
        || metadata.len() > MAX_SETTINGS_BYTES
    {
        return Err(PrivacyStoreError);
    }
    let file = fs::File::open(path).map_err(|_| PrivacyStoreError)?;
    let mut payload = String::new();
    file.take(MAX_SETTINGS_BYTES)
        .read_to_string(&mut payload)
        .map_err(|_| PrivacyStoreError)?;
    serde_json::from_str::<StoredPrivacySettings>(&payload)
        .map(Some)
        .map_err(|_| PrivacyStoreError)
}

/// 把已解析的磁盘结构规范化为运行时设置，并合并迁移或事务恢复的补写要求。
fn normalize_stored_settings(
    stored: StoredPrivacySettings,
    recovered_from_transaction: bool,
) -> Result<(LocalPrivacySettings, bool), PrivacyStoreError> {
    let device_username = stored
        .device_username
        .as_deref()
        .map(DeviceUsername::from_setting_input)
        .transpose()
        .map_err(|_| PrivacyStoreError)?
        .flatten();
    let device_username_initialized =
        stored.device_username_initialized.unwrap_or(false) || device_username.is_some();
    let device_name = stored
        .device_name
        .as_deref()
        .map(DeviceName::from_hostname)
        .transpose()
        .map_err(|_| PrivacyStoreError)?
        .flatten();
    let device_unique_id = stored
        .device_unique_id
        .as_deref()
        .map(DeviceUniqueId::parse)
        .transpose()
        .map_err(|_| PrivacyStoreError)?;
    let scan_interval = resolve_scan_interval_minutes(
        stored.scan_interval_minutes,
        stored.local_scan_interval_minutes,
        stored.remote_refresh_interval_minutes,
    )
    .map_err(|_| PrivacyStoreError)?;
    let retention_days =
        resolve_retention_days(stored.retention_days).map_err(|_| PrivacyStoreError)?;
    let legacy_page_state_needs_write = stored.overview_window_codex.is_some()
        || stored.overview_window_claude_code.is_some()
        || stored.overview_window_grok_build_cli.is_some()
        || stored.usage_window_codex.is_some()
        || stored.usage_window_claude_code.is_some()
        || stored.usage_window_grok_build_cli.is_some()
        || stored.usage_dimension_codex.is_some()
        || stored.usage_dimension_claude_code.is_some()
        || stored.usage_dimension_grok_build_cli.is_some()
        || stored.chart_preferences.is_some()
        || stored.leaderboard_window.is_some()
        || stored.leaderboard_provider_id.is_some()
        || stored.time_standard.is_some()
        || stored.custom_time_zone.is_some()
        || stored.last_selected_agent.is_some();
    let scan_interval_shape_needs_write = stored.scan_interval_minutes.is_none()
        || stored.local_scan_interval_minutes.is_some()
        || stored.remote_refresh_interval_minutes.is_some();
    let enabled_agents = EnabledAgents::from_stored_labels(stored.enabled_agents.as_deref())
        .map_err(|_| PrivacyStoreError)?;
    let workbuddy_stats_enabled = resolve_workbuddy_stats_enabled(stored.workbuddy_stats_enabled);
    let initialization_marker_needs_write = recovered_from_transaction
        || stored.device_username_initialized != Some(device_username_initialized)
        || stored.collect_providers.is_some()
        || stored.collect_provider_base_url.is_some()
        || scan_interval_shape_needs_write
        || legacy_page_state_needs_write
        || stored.enabled_agents.is_none()
        || stored.workbuddy_stats_enabled.is_none()
        || !stored.initialization_completed;

    Ok((
        LocalPrivacySettings {
            language_preference: stored.language_preference,
            local_only: stored.local_only,
            device_username,
            device_username_initialized,
            device_name,
            device_unique_id,
            scan_interval,
            retention_days,
            initialization_completed: true,
            initial_scan_attempted: stored.initial_scan_attempted,
            enabled_agents,
            workbuddy_stats_enabled,
        },
        initialization_marker_needs_write,
    ))
}

/// 首次成功读取设置时完成用户名一次性初始化、缺失唯一 ID 补齐与主机名快照刷新。
pub(crate) fn load_or_initialize_settings<F, H, I>(
    app_data_dir: &Path,
    username_candidate: F,
    hostname_candidate: H,
    stable_id_candidate: I,
) -> Result<LocalPrivacySettings, PrivacyStoreError>
where
    F: FnOnce() -> Option<String>,
    H: FnOnce() -> Option<String>,
    I: FnOnce() -> Option<String>,
{
    let (mut settings, mut needs_write) = load_settings_with_migration_state(app_data_dir)?;
    if !settings.device_username_initialized {
        settings.device_username = username_candidate().and_then(|candidate| {
            DeviceUsername::from_setting_input(&candidate)
                .ok()
                .flatten()
        });
        settings.device_username_initialized = true;
        needs_write = true;
    }
    if settings.device_unique_id.is_none() {
        settings.device_unique_id = Some(resolve_device_unique_id(
            None,
            stable_id_candidate().as_deref(),
        ));
        needs_write = true;
    }
    let refreshed_name = hostname_candidate()
        .and_then(|candidate| DeviceName::from_hostname(&candidate).ok().flatten());
    if settings.device_name != refreshed_name {
        settings.device_name = refreshed_name;
        needs_write = true;
    }
    if needs_write {
        save_settings(app_data_dir, &settings)?;
    }
    Ok(settings)
}

/// 把完整设置快照写入本产品 app-data；Unix 上限制为当前用户读写。
// runtime/settings_state.rs 里七个 set_xxx 方法全部落到这一个函数完成
// 落盘，是它们共用的唯一失败出口；在这里记一次 warn 就覆盖了全部设置
// 持久化失败场景，不需要在每个调用方各写一遍。`PrivacyStoreError` 本身
// 不携带路径或系统错误细节（呼应模块级隐私边界），日志里只能看到
// “保存失败”这一类事实，无法用于反推用户的文件系统状态。
pub(crate) fn save_settings(
    app_data_dir: &Path,
    settings: &LocalPrivacySettings,
) -> Result<(), PrivacyStoreError> {
    save_settings_inner(app_data_dir, settings).inspect_err(|_| {
        tracing::warn!("failed to persist privacy settings to disk");
    })
}

/// 拒绝符号链接边界，序列化设置并原子写入磁盘。
fn save_settings_inner(
    app_data_dir: &Path,
    settings: &LocalPrivacySettings,
) -> Result<(), PrivacyStoreError> {
    reject_symlink(app_data_dir)?;
    fs::create_dir_all(app_data_dir).map_err(|_| PrivacyStoreError)?;
    let settings_path = app_data_dir.join(SETTINGS_FILE_NAME);
    if let Ok(metadata) = fs::symlink_metadata(&settings_path)
        && (metadata_is_link_like(&metadata) || !metadata.is_file())
    {
        return Err(PrivacyStoreError);
    }
    let stored = StoredPrivacySettings {
        language_preference: settings.language_preference,
        local_only: settings.local_only,
        device_username: settings
            .device_username
            .as_ref()
            .map(|username| username.as_str().to_owned()),
        device_username_initialized: Some(settings.device_username_initialized),
        device_name: settings
            .device_name
            .as_ref()
            .map(|name| name.as_str().to_owned()),
        device_unique_id: settings
            .device_unique_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        scan_interval_minutes: Some(settings.scan_interval.get()),
        retention_days: Some(settings.retention_days.get()),
        remote_refresh_interval_minutes: None,
        local_scan_interval_minutes: None,
        collect_providers: None,
        collect_provider_enabled: false,
        collect_provider_base_url: None,
        collect_provider_interval_minutes: 1,
        overview_window_codex: None,
        overview_window_claude_code: None,
        overview_window_grok_build_cli: None,
        usage_window_codex: None,
        usage_window_claude_code: None,
        usage_window_grok_build_cli: None,
        usage_dimension_codex: None,
        usage_dimension_claude_code: None,
        usage_dimension_grok_build_cli: None,
        chart_preferences: None,
        leaderboard_window: None,
        leaderboard_provider_id: None,
        initialization_completed: true,
        initialization_wizard_key: None,
        initial_scan_attempted: settings.initial_scan_attempted,
        enabled_agents: Some(
            settings
                .enabled_agents
                .labels()
                .into_iter()
                .map(str::to_owned)
                .collect(),
        ),
        workbuddy_stats_enabled: Some(settings.workbuddy_stats_enabled),
        time_standard: None,
        custom_time_zone: None,
        last_selected_agent: None,
    };
    let payload = serde_json::to_vec(&stored).map_err(|_| PrivacyStoreError)?;
    write::write_settings_payload(&settings_path, &payload)
}

/// 为缺少 Collect 间隔的旧设置提供一分钟默认值；本仓不再保存该字段。
const fn default_collect_provider_interval_minutes() -> u16 {
    1
}

/// 拒绝把符号链接 app-data 当作可写设置边界。
fn reject_symlink(path: &Path) -> Result<(), PrivacyStoreError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata_is_link_like(&metadata)
    {
        return Err(PrivacyStoreError);
    }
    Ok(())
}

/// 把 Unix 符号链接与 Windows reparse point 统一视为不可写设置边界。
pub(super) fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        /// Windows 文件属性位，标识该项是 reparse point（含符号链接）。
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests;
