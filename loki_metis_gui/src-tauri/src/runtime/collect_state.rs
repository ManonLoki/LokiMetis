//! 多数据上报 Provider 的设置、独立门禁、身份与审计状态装配。

use std::sync::Arc;

use loki_metis_core::{
    CollectAuditError, CollectAuditStore, CollectBaseUrl, CollectClientVersion,
    CollectDeviceIdentity, CollectErrorKind, CollectProviderConfig, CollectProviderConfigs,
    CollectProviderConfigsError, CollectProviderConnectionStatus, CollectProviderId,
    collect_connection_status_after_save,
};
use reqwest::Client;
use tokio::sync::Mutex;

use crate::backend::collect_provider::build_transport_client;
use crate::privacy_store::save_settings;

use super::AppRuntimeState;

/// 配置值不符合 core 安全与集合边界时返回给前端的稳定说明。
const COLLECT_CONFIG_INVALID_MESSAGE: &str = "Data reporting configuration is invalid";
/// 设置文件无法保存时返回给前端的稳定说明。
const COLLECT_CONFIG_SAVE_FAILED_MESSAGE: &str = "Data reporting configuration could not be saved";

impl AppRuntimeState {
    /// 返回监督器与状态 command 使用的完整强类型 Provider 集合快照。
    pub(crate) async fn collect_provider_configs(&self) -> CollectProviderConfigs {
        self.ensure_privacy_settings_loaded().await;
        self.privacy_settings.read().await.collect_providers.clone()
    }

    /// 按稳定 ID 读取单个 Provider 快照；未知 ID 不回退到 URL。
    pub(crate) async fn collect_provider_config(
        &self,
        id: &CollectProviderId,
    ) -> Result<CollectProviderConfig, String> {
        self.collect_provider_configs()
            .await
            .get(id)
            .cloned()
            .map_err(collect_configs_error_message)
    }

    /// 校验、持久化并新增一个 Provider；保存路径只唤醒监督器且不执行网络。
    pub(crate) async fn add_collect_provider(
        &self,
        base_url: String,
        interval_minutes: u16,
        user_alias: Option<String>,
    ) -> Result<CollectProviderConfig, String> {
        self.ensure_privacy_settings_loaded().await;
        let _update = self.privacy_settings_update.lock().await;
        let mut settings = self.privacy_settings.read().await.clone();
        let config = settings
            .collect_providers
            .add(&base_url, interval_minutes, user_alias.as_deref())
            .map_err(collect_configs_error_message)?;
        self.persist_collect_settings(settings).await?;
        self.collect_connection_statuses
            .lock()
            .await
            .insert(config.id.clone(), CollectProviderConnectionStatus::Untested);
        self.collect_wakeup.notify_one();
        Ok(config)
    }

    /// 校验、持久化并编辑指定 Provider；稳定 ID 不随地址或别名变化。
    pub(crate) async fn update_collect_provider(
        &self,
        id: &CollectProviderId,
        base_url: String,
        interval_minutes: u16,
        user_alias: Option<String>,
    ) -> Result<CollectProviderConfig, String> {
        self.ensure_privacy_settings_loaded().await;
        let _update = self.privacy_settings_update.lock().await;
        let mut settings = self.privacy_settings.read().await.clone();
        let previous = settings
            .collect_providers
            .get(id)
            .map_err(collect_configs_error_message)?
            .clone();
        let config = settings
            .collect_providers
            .update(id, &base_url, interval_minutes, user_alias.as_deref())
            .map_err(collect_configs_error_message)?;
        let previous_status = self.collect_provider_connection_status(id).await;
        let next_status =
            collect_connection_status_after_save(Some(&previous), &config, previous_status);
        self.persist_collect_settings(settings).await?;
        self.set_collect_provider_connection_status(config.id.clone(), next_status)
            .await;
        self.collect_wakeup.notify_one();
        Ok(config)
    }

    /// 删除指定 Provider 的本地配置并停止后续调度；不删除远端数据或本地审计。
    pub(crate) async fn delete_collect_provider(
        &self,
        id: &CollectProviderId,
    ) -> Result<CollectProviderConfig, String> {
        self.ensure_privacy_settings_loaded().await;
        let _update = self.privacy_settings_update.lock().await;
        let mut settings = self.privacy_settings.read().await.clone();
        let removed = settings
            .collect_providers
            .remove(id)
            .map_err(collect_configs_error_message)?;
        self.persist_collect_settings(settings).await?;
        self.collect_provider_gates.lock().await.remove(id);
        self.collect_connection_statuses.lock().await.remove(id);
        self.collect_wakeup.notify_one();
        Ok(removed)
    }

    /// 获取指定 Provider 的独立在途门禁；删除后已有 Arc 只供在途操作安全收敛。
    pub(crate) async fn collect_provider_gate(&self, id: &CollectProviderId) -> Arc<Mutex<()>> {
        Arc::clone(
            self.collect_provider_gates
                .lock()
                .await
                .entry(id.clone())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    /// 读取指定 Provider 当前进程内的三态结果；尚未完成 Health 探测时返回未探测。
    pub(crate) async fn collect_provider_connection_status(
        &self,
        id: &CollectProviderId,
    ) -> CollectProviderConnectionStatus {
        self.collect_connection_statuses
            .lock()
            .await
            .get(id)
            .copied()
            .unwrap_or_default()
    }

    /// 返回全部 Provider 当前进程内三态结果的独立快照，供状态 command 一次映射。
    pub(crate) async fn collect_provider_connection_statuses(
        &self,
    ) -> std::collections::HashMap<CollectProviderId, CollectProviderConnectionStatus> {
        self.collect_connection_statuses.lock().await.clone()
    }

    /// 在一次已完成 Health 探测后更新指定 Provider 的三态结果。
    pub(crate) async fn set_collect_provider_connection_status(
        &self,
        id: CollectProviderId,
        status: CollectProviderConnectionStatus,
    ) {
        self.collect_connection_statuses
            .lock()
            .await
            .insert(id, status);
    }

    /// 仅当 Health 完成时 ID 仍指向原目标才提交灯态，避免改址或删除后的旧探测污染新卡片。
    pub(crate) async fn complete_collect_provider_health(
        &self,
        id: CollectProviderId,
        attempted_base_url: &CollectBaseUrl,
        status: CollectProviderConnectionStatus,
    ) -> bool {
        self.ensure_privacy_settings_loaded().await;
        let _update = self.privacy_settings_update.lock().await;
        let destination_matches = self
            .privacy_settings
            .read()
            .await
            .collect_providers
            .get(&id)
            .is_ok_and(|config| &config.base_url == attempted_base_url);
        if destination_matches {
            self.collect_connection_statuses
                .lock()
                .await
                .insert(id, status);
        }
        destination_matches
    }

    /// 在磁盘保存成功后一次性替换内存设置，失败时保留原快照。
    async fn persist_collect_settings(
        &self,
        settings: crate::privacy_store::LocalPrivacySettings,
    ) -> Result<(), String> {
        let app_data_dir = self.app_data_dir.clone();
        let settings_to_save = settings.clone();
        tauri::async_runtime::spawn_blocking(move || {
            save_settings(&app_data_dir, &settings_to_save)
        })
        .await
        .map_err(|_| COLLECT_CONFIG_SAVE_FAILED_MESSAGE.to_owned())?
        .map_err(|_| COLLECT_CONFIG_SAVE_FAILED_MESSAGE.to_owned())?;
        *self.privacy_settings.write().await = settings;
        Ok(())
    }

    /// 为指定 Provider 构造 payload 身份：用户名由 core 按“别名优先、否则全局设备用户名”决定；任一必填项缺失即拒绝发送。
    pub(crate) async fn collect_device_identity(
        &self,
        config: &CollectProviderConfig,
    ) -> Option<CollectDeviceIdentity> {
        self.ensure_privacy_settings_loaded().await;
        let settings = self.privacy_settings.read().await;
        Some(CollectDeviceIdentity {
            username: config
                .reported_username(settings.device_username.as_ref())?
                .as_str()
                .to_owned(),
            device_name: settings.device_name.as_ref()?.as_str().to_owned(),
            device_id: settings.device_unique_id.as_ref()?.as_str().to_owned(),
            client_version: CollectClientVersion::parse(env!("CARGO_PKG_VERSION"))
                .expect("Cargo package version must satisfy the Collect identity contract"),
        })
    }

    /// 惰性打开进程唯一审计库；打开失败时调用方不得继续网络请求。
    pub(crate) async fn collect_audit_store(
        &self,
    ) -> Result<&CollectAuditStore, CollectAuditError> {
        self.collect_audit
            .get_or_try_init(|| CollectAuditStore::open_in_app_data(&self.app_data_dir))
            .await
    }

    /// 惰性构造进程唯一传输客户端；构造失败时调用方不得继续网络请求。
    pub(crate) async fn collect_http_client(&self) -> Result<&Client, CollectErrorKind> {
        self.collect_http_client
            .get_or_try_init(|| async { build_transport_client() })
            .await
    }
}

/// 把 core 配置失败收敛为不含用户 URL 或 ID 的稳定消息。
fn collect_configs_error_message(_error: CollectProviderConfigsError) -> String {
    COLLECT_CONFIG_INVALID_MESSAGE.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 验证多个最小 Provider 配置可按稳定 ID 独立保存、编辑和删除。
    #[tokio::test]
    async fn persists_independent_provider_crud() {
        let temp = tempdir().expect("isolated app-data exists");
        let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
            Some("fixture-user".to_owned())
        });
        assert!(state.collect_provider_configs().await.as_slice().is_empty());

        let first = state
            .add_collect_provider("https://one.example/base".to_owned(), 15, None)
            .await
            .expect("first provider is saved");
        let second = state
            .add_collect_provider("http://two.example".to_owned(), 20, None)
            .await
            .expect("second provider is saved");
        assert_eq!(
            state.collect_provider_connection_status(&second.id).await,
            CollectProviderConnectionStatus::Untested
        );
        state
            .set_collect_provider_connection_status(
                second.id.clone(),
                CollectProviderConnectionStatus::Reachable,
            )
            .await;

        let updated = state
            .update_collect_provider(&second.id, "http://two.example".to_owned(), 30, None)
            .await
            .expect("second provider is independently updated");
        assert_eq!(updated.id, second.id);
        assert_eq!(updated.interval.get(), 30);
        assert_eq!(
            state.collect_provider_connection_status(&updated.id).await,
            CollectProviderConnectionStatus::Reachable,
            "只修改间隔必须保留当前结果"
        );
        let changed_destination = state
            .update_collect_provider(
                &updated.id,
                "http://three.example".to_owned(),
                updated.interval.get(),
                None,
            )
            .await
            .expect("destination update is saved");
        assert_eq!(
            state
                .collect_provider_connection_status(&changed_destination.id)
                .await,
            CollectProviderConnectionStatus::Untested,
            "改址必须回到黄色未探测状态"
        );
        assert_eq!(state.collect_provider_configs().await.as_slice().len(), 2);

        state
            .delete_collect_provider(&first.id)
            .await
            .expect("first provider is deleted");
        assert_eq!(
            state.collect_provider_configs().await.as_slice(),
            &[changed_destination]
        );
    }

    /// 验证重复目标或未知 ID 操作失败后不会覆盖原设置。
    #[tokio::test]
    async fn preserves_settings_when_provider_mutation_is_invalid() {
        let temp = tempdir().expect("isolated app-data exists");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        let first = state
            .add_collect_provider("https://one.example".to_owned(), 10, None)
            .await
            .expect("first provider is saved");
        assert!(
            state
                .add_collect_provider("https://one.example/".to_owned(), 10, None)
                .await
                .is_err()
        );
        let unknown = CollectProviderId::parse("99999999-9999-4999-8999-999999999999")
            .expect("fixture ID is valid");
        assert!(state.delete_collect_provider(&unknown).await.is_err());
        assert_eq!(state.collect_provider_configs().await.as_slice(), &[first]);
    }

    /// 验证每次构造设备身份都固定携带当前桌面包版本，且未设别名时用户名取全局设备用户名。
    #[tokio::test]
    async fn builds_required_current_client_version_identity() {
        let temp = tempdir().expect("isolated app-data exists");
        let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
            Some("fixture-user".to_owned())
        });
        let config = state
            .add_collect_provider("https://one.example".to_owned(), 10, None)
            .await
            .expect("provider without alias is saved");

        let included = state
            .collect_device_identity(&config)
            .await
            .expect("initialized fixture identity exists");
        assert_eq!(included.client_version.as_str(), env!("CARGO_PKG_VERSION"));
        assert_eq!(included.username, "fixture-user");
    }

    /// 验证设置别名的 Provider 上报用户名取别名、清除别名后回退设备用户名，且别名不影响其他 Provider。
    #[tokio::test]
    async fn uses_provider_user_alias_as_reported_username() {
        let temp = tempdir().expect("isolated app-data exists");
        let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
            Some("fixture-user".to_owned())
        });
        let aliased = state
            .add_collect_provider(
                "https://one.example".to_owned(),
                10,
                Some("  团队别名  ".to_owned()),
            )
            .await
            .expect("aliased provider is saved");
        let plain = state
            .add_collect_provider("https://two.example".to_owned(), 10, None)
            .await
            .expect("plain provider is saved");

        assert_eq!(
            state
                .collect_device_identity(&aliased)
                .await
                .expect("aliased identity exists")
                .username,
            "团队别名"
        );
        assert_eq!(
            state
                .collect_device_identity(&plain)
                .await
                .expect("plain identity exists")
                .username,
            "fixture-user"
        );

        let cleared = state
            .update_collect_provider(
                &aliased.id,
                "https://one.example".to_owned(),
                10,
                Some(String::new()),
            )
            .await
            .expect("blank alias clears the override");
        assert_eq!(cleared.user_alias, None);
        assert_eq!(
            state
                .collect_device_identity(&cleared)
                .await
                .expect("cleared identity exists")
                .username,
            "fixture-user"
        );
        assert!(
            state
                .update_collect_provider(
                    &plain.id,
                    "https://two.example".to_owned(),
                    10,
                    Some("x".repeat(65)),
                )
                .await
                .is_err(),
            "超长别名必须被 core 拒绝"
        );
    }

    /// 验证全局设备用户名缺失时，只有设置了别名的 Provider 才能形成完整上报身份。
    #[tokio::test]
    async fn alias_supplies_username_when_device_username_is_missing() {
        let temp = tempdir().expect("isolated app-data exists");
        let state =
            AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || None);
        let aliased = state
            .add_collect_provider(
                "https://one.example".to_owned(),
                10,
                Some("alias-only".to_owned()),
            )
            .await
            .expect("aliased provider is saved");
        let plain = state
            .add_collect_provider("https://two.example".to_owned(), 10, None)
            .await
            .expect("plain provider is saved");

        assert_eq!(
            state
                .collect_device_identity(&aliased)
                .await
                .map(|identity| identity.username),
            Some("alias-only".to_owned())
        );
        assert!(state.collect_device_identity(&plain).await.is_none());
    }

    /// 验证同一 ID 复用自身门禁，而不同 Provider 获得互不阻塞的门禁。
    #[tokio::test]
    async fn assigns_independent_inflight_gates_per_provider() {
        let temp = tempdir().expect("isolated app-data exists");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        let first = state
            .add_collect_provider("https://one.example".to_owned(), 10, None)
            .await
            .expect("first provider is saved");
        let second = state
            .add_collect_provider("https://two.example".to_owned(), 10, None)
            .await
            .expect("second provider is saved");
        let first_gate = state.collect_provider_gate(&first.id).await;
        let first_gate_again = state.collect_provider_gate(&first.id).await;
        let second_gate = state.collect_provider_gate(&second.id).await;
        assert!(Arc::ptr_eq(&first_gate, &first_gate_again));
        assert!(!Arc::ptr_eq(&first_gate, &second_gate));

        let _first_guard = first_gate
            .try_lock()
            .expect("first provider gate can be occupied");
        assert!(first_gate_again.try_lock().is_err());
        let _second_guard = second_gate
            .try_lock()
            .expect("second provider remains independently available");
    }

    /// 旧目标操作完成后不得覆盖改址后的黄色状态，也不得为已删除条目重建状态。
    #[tokio::test]
    async fn ignores_operation_result_after_destination_change_or_delete() {
        let temp = tempdir().expect("isolated app-data exists");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        let original = state
            .add_collect_provider("https://one.example".to_owned(), 10, None)
            .await
            .expect("provider is saved");
        let original_base_url = original.base_url.clone();
        let updated = state
            .update_collect_provider(&original.id, "https://two.example".to_owned(), 10, None)
            .await
            .expect("destination is updated");

        assert!(
            !state
                .complete_collect_provider_health(
                    original.id.clone(),
                    &original_base_url,
                    CollectProviderConnectionStatus::Reachable,
                )
                .await
        );
        assert_eq!(
            state.collect_provider_connection_status(&original.id).await,
            CollectProviderConnectionStatus::Untested
        );
        assert!(
            state
                .complete_collect_provider_health(
                    updated.id.clone(),
                    &updated.base_url,
                    CollectProviderConnectionStatus::Reachable,
                )
                .await
        );

        state
            .delete_collect_provider(&updated.id)
            .await
            .expect("provider is deleted");
        assert!(
            !state
                .complete_collect_provider_health(
                    updated.id.clone(),
                    &updated.base_url,
                    CollectProviderConnectionStatus::Unreachable,
                )
                .await
        );
        assert!(
            !state
                .collect_provider_connection_statuses()
                .await
                .contains_key(&updated.id)
        );
    }
}
