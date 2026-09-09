//! 实现只接受稳定根 ID 的数据根停用与索引移除 commands。

mod manual_add;
mod manual_inspection;
mod support;

pub(crate) use manual_add::manual_add_source_root;

use crate::backend::local_index::{DiscoveryInputs, LocalIndex, discover_quick};
use loki_metis_core::source_client_app_data_dir;
use loki_metis_core::{
    SourceRootLookupError, ensure_primary_source_root_supported,
    extract_single_verified_source_root, normalize_source_root_alias,
    source_root_alias_invalid_message, source_root_id_invalid_message,
    source_root_not_found_message, source_root_primary_only_for_codex_message,
    source_root_primary_root_not_enabled_message, source_root_primary_validation_failed_message,
    source_root_reindex_request, source_root_reindex_requires_enabled_message, source_root_remove,
    source_root_rename, source_root_set_enabled, source_root_set_primary,
    source_root_store_error_message, validate_source_root_id,
};
use tauri::{AppHandle, State};

use crate::commands::ensure_business_access;
use crate::dto::{AgentClientKindDto, SourceRootMutationDto};
use crate::runtime::AppRuntimeState;
use crate::source_commands::support::{
    SourceRootCatalogAdapter, acquire_source_root_write_context,
    build_source_root_mutation_response, parser_version_for_client, refresh_source_roots_snapshot,
    source_root_catalog_error_message, to_source_client_kind,
};
use crate::tray::refresh_tray_daily_token_title;
pub(crate) use support::acquire_source_root_write_permit;

/// 校验单根重新索引请求，并只把稳定根 ID 交给扫描 adapter。
pub(crate) async fn validated_source_root_reindex_request(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
    root_id: &str,
) -> Result<loki_metis_core::SourceRootReindexRequest, String> {
    let source_client = to_source_client_kind(client);
    validate_source_root_id(source_client, root_id)
        .map_err(|_| source_root_id_invalid_message().to_owned())?;
    let app_data_dir = source_client_app_data_dir(&state.app_data_dir, source_client);
    let catalog = SourceRootCatalogAdapter::new(app_data_dir, source_client);
    source_root_reindex_request(&catalog, root_id)
        .await
        .map_err(|error| {
            source_root_catalog_error_message(error, |lookup_error| match lookup_error {
                SourceRootLookupError::Missing => source_root_not_found_message(),
                SourceRootLookupError::NotEnabled => source_root_reindex_requires_enabled_message(),
            })
        })
}

/// 启用或停用已登记数据根；前端不得传入文件系统路径。
#[tauri::command]
pub(crate) async fn set_source_root_enabled(
    app: AppHandle,
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
    root_id: String,
    enabled: bool,
) -> Result<SourceRootMutationDto, String> {
    ensure_business_access(&state).await?;
    validate_source_root_id(to_source_client_kind(client), &root_id)
        .map_err(|_| source_root_id_invalid_message().to_owned())?;
    let (_write_permit, account_context_guard) =
        acquire_source_root_write_context(&state, client).await?;
    let source_client = to_source_client_kind(client);
    let app_data_dir = source_client_app_data_dir(&state.app_data_dir, source_client);
    let mut catalog = SourceRootCatalogAdapter::new(app_data_dir, source_client);
    let mutation = source_root_set_enabled(&mut catalog, &root_id, enabled)
        .await
        .map_err(|error| {
            source_root_catalog_error_message(error, |lookup_error| match lookup_error {
                SourceRootLookupError::Missing => source_root_not_found_message(),
                SourceRootLookupError::NotEnabled => source_root_not_found_message(),
            })
        })?;
    drop(account_context_guard);
    refresh_source_roots_snapshot(&state, client).await;
    refresh_tray_daily_token_title(&app).await;
    Ok(build_source_root_mutation_response(client, mutation))
}

/// 更新数据根的安全展示别名；别名不得伪装成路径或携带控制字符。
#[tauri::command]
pub(crate) async fn rename_source_root(
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
    root_id: String,
    alias: String,
) -> Result<SourceRootMutationDto, String> {
    ensure_business_access(&state).await?;
    validate_source_root_id(to_source_client_kind(client), &root_id)
        .map_err(|_| source_root_id_invalid_message().to_owned())?;
    let alias = normalize_source_root_alias(&alias)
        .map_err(|_| source_root_alias_invalid_message().to_owned())?;
    let _write_permit = acquire_source_root_write_permit(&state, client)?;
    let source_client = to_source_client_kind(client);
    let app_data_dir = source_client_app_data_dir(&state.app_data_dir, source_client);
    let mut catalog = SourceRootCatalogAdapter::new(app_data_dir, source_client);
    let mutation = source_root_rename(&mut catalog, &root_id, &alias)
        .await
        .map_err(|error| {
            source_root_catalog_error_message(error, |_| source_root_not_found_message())
        })?;
    refresh_source_roots_snapshot(&state, client).await;
    Ok(build_source_root_mutation_response(client, mutation))
}

/// 移除数据根及其本产品派生索引，不删除或修改根中的客户端原始文件。
#[tauri::command]
pub(crate) async fn remove_source_root(
    app: AppHandle,
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
    root_id: String,
) -> Result<SourceRootMutationDto, String> {
    ensure_business_access(&state).await?;
    validate_source_root_id(to_source_client_kind(client), &root_id)
        .map_err(|_| source_root_id_invalid_message().to_owned())?;
    let (_write_permit, account_context_guard) =
        acquire_source_root_write_context(&state, client).await?;
    let source_client = to_source_client_kind(client);
    let app_data_dir = source_client_app_data_dir(&state.app_data_dir, source_client);
    let mut catalog = SourceRootCatalogAdapter::new(app_data_dir, source_client);
    let mutation = source_root_remove(&mut catalog, &root_id)
        .await
        .map_err(|error| {
            source_root_catalog_error_message(error, |_| source_root_not_found_message())
        })?;
    drop(account_context_guard);
    refresh_source_roots_snapshot(&state, client).await;
    refresh_tray_daily_token_title(&app).await;
    Ok(build_source_root_mutation_response(client, mutation))
}

/// 设置或清除 Codex 唯一主数据目录；Claude Code 在任何数据库访问前即被拒绝。
#[tauri::command]
pub(crate) async fn set_primary_source_root(
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
    root_id: Option<String>,
) -> Result<SourceRootMutationDto, String> {
    let source_client = to_source_client_kind(client);
    ensure_primary_source_root_supported(source_client)
        .map_err(|_| source_root_primary_only_for_codex_message().to_owned())?;
    if let Some(root_id) = root_id.as_deref() {
        validate_source_root_id(source_client, root_id)
            .map_err(|_| source_root_id_invalid_message().to_owned())?;
    }
    let (_write_permit, account_context_guard) =
        acquire_source_root_write_context(&state, client).await?;
    let requested_primary_root_id = root_id.clone();
    let app_data_dir = source_client_app_data_dir(&state.app_data_dir, source_client);
    let parser_version = parser_version_for_client(client);
    if let Some(primary_root_id) = requested_primary_root_id.as_deref() {
        let index = LocalIndex::open_in_app_data(&app_data_dir, parser_version)
            .await
            .map_err(|_| source_root_store_error_message())?;
        let all_roots = index
            .all_roots()
            .await
            .map_err(|_| source_root_store_error_message())?;
        let root = all_roots
            .iter()
            .find(|root| root.root_id.as_deref() == Some(primary_root_id))
            .ok_or_else(|| source_root_not_found_message().to_owned())?
            .clone();
        if !root.enabled {
            return Err(source_root_primary_root_not_enabled_message().to_owned());
        }
        let registered = tauri::async_runtime::spawn_blocking(move || {
            discover_quick(
                &DiscoveryInputs {
                    home_dir: None,
                    codex_home: None,
                    registered_roots: vec![root],
                },
                &Default::default(),
            )
        })
        .await
        .map_err(|_| source_root_store_error_message())?;
        extract_single_verified_source_root(
            &registered.roots,
            |root| root.root_id.as_str(),
            &registered.confirmed_invalid_root_ids,
            &registered.unconfirmed_root_ids,
            Some(primary_root_id),
        )
        .map_err(|_| source_root_primary_validation_failed_message().to_owned())?;
    }
    let mut catalog = SourceRootCatalogAdapter::new(app_data_dir, source_client);
    let mutation = source_root_set_primary(&mut catalog, requested_primary_root_id.as_deref())
        .await
        .map_err(|error| {
            source_root_catalog_error_message(error, |lookup_error| match lookup_error {
                SourceRootLookupError::Missing => source_root_not_found_message(),
                SourceRootLookupError::NotEnabled => source_root_primary_root_not_enabled_message(),
            })
        })?;
    drop(account_context_guard);
    refresh_source_roots_snapshot(&state, client).await;
    Ok(build_source_root_mutation_response(client, mutation))
}

#[cfg(test)]
mod tests;
