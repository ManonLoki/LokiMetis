//! 设置与初始化状态的更新、持久化与读取路径。

use crate::dto::{
    IndexLocationCodeDto, InitializationStatusDto, LanguagePreferenceDto, PrivacySettingsDto,
    UsageClientKindDto,
};
use crate::privacy_store::{LocalPrivacySettings, save_settings};
#[cfg(test)]
use loki_metis_core::initial_scan_state_save_failed_message;
use loki_metis_core::{
    DeviceUsername, EnabledAgents, RetentionDays, ScanIntervalMinutes, SourceClientKind,
    device_username_error_message, device_username_save_failed_message,
    enabled_agents_error_message, index_location_claude_code_label, index_location_codex_label,
    initialization_state_save_failed_message, language_setting_save_failed_message,
    privacy_settings_save_failed_message, retention_days_range_message,
    retention_days_save_failed_message, scan_interval_range_message,
    scan_interval_save_failed_message, workbuddy_stats_enabled_save_failed_message,
};

use super::AppRuntimeState;

impl AppRuntimeState {
    /// 保存仅本机偏好；该字段只作兼容持久化，不再启动任何官方读取。
    pub(crate) async fn set_local_only(&self, local_only: bool) -> Result<(), String> {
        self.ensure_privacy_settings_loaded().await;
        let _update = self.privacy_settings_update.lock().await;
        let mut settings = self.privacy_settings.read().await.clone();
        settings.local_only = local_only;
        *self.privacy_settings.write().await = settings.clone();
        let app_data_dir = self.app_data_dir.clone();
        // spawn_blocking：把同步的磁盘文件写入（save_settings 内部是阻塞 I/O）
        // 丢到 Tokio 专门的阻塞线程池执行，避免占用异步运行时的少量工作线程，
        // 这是 Rust async 生态处理“不得不用的同步操作”的标准写法。
        tauri::async_runtime::spawn_blocking(move || save_settings(&app_data_dir, &settings))
            .await
            .map_err(|_| privacy_settings_save_failed_message().to_owned())?
            .map_err(|_| privacy_settings_save_failed_message().to_owned())?;
        Ok(())
    }

    /// 下面这一组 set_xxx 方法（设备用户名、扫描间隔、语言偏好、初始化完成
    /// 标记……）共享的固定流程，集中在这里只实现一次：
    ///   1. 确保设置已从磁盘加载过一次（ensure_privacy_settings_loaded）；
    ///   2. 用 privacy_settings_update 互斥锁防止并发写互相覆盖；
    ///   3. 基于内存里当前设置克隆一份，交给 `mutate` 修改目标字段；
    ///   4. spawn_blocking 落盘，任一环节失败都保留旧的内存状态（不做部分更新）；
    ///   5. 全部成功后才用新值整体替换内存里的 privacy_settings。
    ///
    /// 各调用方只负责自己的输入校验和落盘失败文案。
    async fn update_privacy_settings(
        &self,
        save_failed_message: &str,
        mutate: impl FnOnce(&mut LocalPrivacySettings),
    ) -> Result<(), String> {
        self.ensure_privacy_settings_loaded().await;
        let _update = self.privacy_settings_update.lock().await;
        let mut settings = self.privacy_settings.read().await.clone();
        mutate(&mut settings);
        let app_data_dir = self.app_data_dir.clone();
        let settings_to_save = settings.clone();
        // spawn_blocking：把同步的磁盘文件写入（save_settings 内部是阻塞 I/O）
        // 丢到 Tokio 专门的阻塞线程池执行，避免占用异步运行时的少量工作线程。
        tauri::async_runtime::spawn_blocking(move || save_settings(&app_data_dir, &settings_to_save))
            .await
            .map_err(|_| save_failed_message.to_owned())?
            .map_err(|_| save_failed_message.to_owned())?;
        *self.privacy_settings.write().await = settings;
        Ok(())
    }

    /// 校验并保存用户提供的设备用户名；留空清除且禁止后续自动回填。
    pub(crate) async fn set_device_username(&self, input: String) -> Result<(), String> {
        let device_username = DeviceUsername::from_setting_input(&input)
            .map_err(|error| device_username_error_message(error).to_owned())?;
        self.update_privacy_settings(device_username_save_failed_message(), |settings| {
            settings.device_username = device_username;
            settings.device_username_initialized = true;
        })
        .await
    }

    /// 保存单一扫描间隔；只改本机周期扫描节奏，不发网。
    pub(crate) async fn set_scan_interval(&self, minutes: u16) -> Result<(), String> {
        let interval = ScanIntervalMinutes::new(minutes)
            .map_err(|_| scan_interval_range_message().to_owned())?;
        self.update_privacy_settings(scan_interval_save_failed_message(), |settings| {
            settings.scan_interval = interval;
        })
        .await
    }

    /// 返回本机周期扫描与远端偏好共用的扫描间隔。
    pub(crate) async fn scan_interval(&self) -> ScanIntervalMinutes {
        self.ensure_privacy_settings_loaded().await;
        self.privacy_settings.read().await.scan_interval
    }

    /// 保存派生用量自动清理天数；保存本身不立刻删数据。
    pub(crate) async fn set_retention_days(&self, days: u16) -> Result<(), String> {
        let retention_days =
            RetentionDays::new(days).map_err(|_| retention_days_range_message().to_owned())?;
        self.update_privacy_settings(retention_days_save_failed_message(), |settings| {
            settings.retention_days = retention_days;
        })
        .await
    }

    /// 返回已保存的派生用量保留天数。
    pub(crate) async fn retention_days(&self) -> RetentionDays {
        self.ensure_privacy_settings_loaded().await;
        self.privacy_settings.read().await.retention_days
    }

    /// 返回不含客户端数据的首次初始化门禁状态。
    pub(crate) async fn initialization_status(&self) -> InitializationStatusDto {
        self.ensure_privacy_settings_loaded().await;
        let settings = self.privacy_settings.read().await;
        InitializationStatusDto {
            initialization_completed: settings.initialization_completed,
            language_preference: settings.language_preference,
        }
    }

    /// 持久化界面语言偏好；不刷新查询、不启动 provider，也不触发扫描或索引。
    pub(crate) async fn set_language_preference(
        &self,
        language_preference: LanguagePreferenceDto,
    ) -> Result<(), String> {
        self.update_privacy_settings(language_setting_save_failed_message(), |settings| {
            settings.language_preference = language_preference;
        })
        .await
    }

    /// 看板不接源项目向导；已开启 Agent 即可读取与扫描。
    pub(crate) async fn initialization_completed(&self) -> bool {
        self.ensure_privacy_settings_loaded().await;
        true
    }

    /// 持久化首次初始化完成或测试重置状态，同时保留其他设置字段。
    pub(crate) async fn set_initialization_completed(&self, completed: bool) -> Result<(), String> {
        self.update_privacy_settings(initialization_state_save_failed_message(), |settings| {
            settings.initialization_completed = completed;
        })
        .await
    }

    /// 原子认领首次自动快速扫描；持久标记成功后才允许启动磁盘任务。
    // “认领”（claim）模式：多处代码路径都可能在启动时尝试触发首次扫描，
    // 这个方法必须保证“最多真正触发一次”。做法是——在拿到
    // privacy_settings_update 独占锁的前提下检查+置位 initial_scan_attempted
    // 标记并立即落盘，返回 true 的调用方才被允许真正发起扫描，
    // 后来者看到标记已置位就直接返回 false，从而避免竞态下重复扫描。
    #[cfg(test)]
    pub(crate) async fn claim_initial_scan(&self) -> Result<bool, String> {
        self.ensure_privacy_settings_loaded().await;
        let _update = self.privacy_settings_update.lock().await;
        let mut settings = self.privacy_settings.read().await.clone();
        if !settings.initialization_completed || settings.initial_scan_attempted {
            return Ok(false);
        }
        settings.initial_scan_attempted = true;
        let app_data_dir = self.app_data_dir.clone();
        let settings_to_save = settings.clone();
        tauri::async_runtime::spawn_blocking(move || {
            save_settings(&app_data_dir, &settings_to_save)
        })
        .await
        .map_err(|_| initial_scan_state_save_failed_message().to_owned())?
        .map_err(|_| initial_scan_state_save_failed_message().to_owned())?;
        *self.privacy_settings.write().await = settings;
        Ok(true)
    }

    /// 返回不暴露绝对 app-data 路径的隐私设置快照。
    pub(crate) async fn privacy_settings(&self, client: UsageClientKindDto) -> PrivacySettingsDto {
        self.ensure_privacy_settings_loaded().await;
        let (
            language_preference,
            local_only,
            device_username,
            device_name,
            device_unique_id,
            scan_interval_minutes,
            retention_days,
            enabled_agents,
            workbuddy_stats_enabled,
        ) = {
            let settings = self.privacy_settings.read().await;
            (
                settings.language_preference,
                settings.local_only,
                settings
                    .device_username
                    .as_ref()
                    .map(|username| username.as_str().to_owned()),
                settings
                    .device_name
                    .as_ref()
                    .map(|name| name.as_str().to_owned()),
                settings
                    .device_unique_id
                    .as_ref()
                    .map(|id| id.as_str().to_owned()),
                settings.scan_interval.get(),
                settings.retention_days.get(),
                to_dto_enabled_agents(settings.enabled_agents),
                settings.workbuddy_stats_enabled,
            )
        };
        let local_client = client.local_client();
        let index_size_bytes = self.index_size_bytes(local_client).await;
        let last_cleared_at_epoch_ms = *self
            .last_cleared_at_epoch_ms
            .get(local_client.into())
            .read()
            .await;
        PrivacySettingsDto {
            language_preference,
            local_only,
            device_username,
            device_name,
            device_unique_id,
            scan_interval_minutes,
            retention_days,
            index_location_label: match client {
                UsageClientKindDto::Codex => index_location_codex_label().to_owned(),
                UsageClientKindDto::ClaudeCode => index_location_claude_code_label().to_owned(),
                UsageClientKindDto::GrokBuildCli => {
                    loki_metis_core::index_location_grok_build_cli_label().to_owned()
                }
            },
            index_location_code: match client {
                UsageClientKindDto::Codex => IndexLocationCodeDto::Codex,
                UsageClientKindDto::ClaudeCode => IndexLocationCodeDto::ClaudeCode,
                UsageClientKindDto::GrokBuildCli => IndexLocationCodeDto::GrokBuildCli,
            },
            index_size_bytes,
            last_cleared_at_epoch_ms,
            enabled_agents,
            workbuddy_stats_enabled,
            device_time_zone: loki_metis_core::device_time_zone_name(),
        }
    }

    /// 返回用户是否已显式开放读取 WorkBuddy 本地用量统计。
    pub(crate) async fn workbuddy_stats_enabled(&self) -> bool {
        self.ensure_privacy_settings_loaded().await;
        self.privacy_settings.read().await.workbuddy_stats_enabled
    }

    /// 保存 WorkBuddy 本地统计开关；关闭时后续读取命令必须拒绝返回统计数据。
    pub(crate) async fn set_workbuddy_stats_enabled(&self, enabled: bool) -> Result<(), String> {
        self.update_privacy_settings(workbuddy_stats_enabled_save_failed_message(), |settings| {
            settings.workbuddy_stats_enabled = enabled;
        })
        .await
    }

    /// 返回当前已开放的本机 Agent 集合。
    pub(crate) async fn enabled_agents(&self) -> EnabledAgents {
        self.ensure_privacy_settings_loaded().await;
        self.privacy_settings.read().await.enabled_agents
    }

    /// 保存用户显式开放的本机 Agent 集合；未知标识由 core 拒绝。
    pub(crate) async fn set_enabled_agents(
        &self,
        agents: &[UsageClientKindDto],
    ) -> Result<(), String> {
        let enabled = EnabledAgents::try_from_labels(
            &agents
                .iter()
                .map(|agent| match agent {
                    UsageClientKindDto::Codex => "codex",
                    UsageClientKindDto::ClaudeCode => "claudeCode",
                    UsageClientKindDto::GrokBuildCli => "grokBuildCli",
                })
                .collect::<Vec<_>>(),
        )
        .map_err(enabled_agents_error_message)?;
        self.update_privacy_settings(privacy_settings_save_failed_message(), |settings| {
            settings.enabled_agents = enabled;
        })
        .await
    }
}

/// 把已开放集合映射为 IPC 枚举，保持 Codex / Claude / Grok 固定顺序；
/// `EnabledAgents::iter` 从不产出 WorkBuddy，故该分支不可达。
fn to_dto_enabled_agents(enabled: EnabledAgents) -> Vec<UsageClientKindDto> {
    enabled
        .iter()
        .map(|client| match client {
            SourceClientKind::Codex => UsageClientKindDto::Codex,
            SourceClientKind::ClaudeCode => UsageClientKindDto::ClaudeCode,
            SourceClientKind::GrokBuildCli => UsageClientKindDto::GrokBuildCli,
            SourceClientKind::WorkBuddy => unreachable!("EnabledAgents 从不产出 WorkBuddy"),
        })
        .collect()
}
