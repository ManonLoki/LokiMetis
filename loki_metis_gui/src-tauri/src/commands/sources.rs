//! 数据源查询命令：当前客户端数据源元数据与来源快照。

use std::sync::Arc;

use tauri::State;

use crate::commands::ensure_business_access;
use crate::dto::{AgentClientKindDto, SourcesDto};
use crate::local_view::load_source_roots_for_parser;
use crate::runtime::AppRuntimeState;

/// 读取当前客户端本机数据根覆盖与扫描状态。
#[tauri::command]
pub(crate) async fn get_sources(
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
) -> Result<SourcesDto, String> {
    get_sources_for_state(&state, client).await
}

/// 承载数据源 command 的可测试路径，只读取本机根与覆盖。
pub(super) async fn get_sources_for_state(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
) -> Result<SourcesDto, String> {
    ensure_business_access(state).await?;
    let binding = Arc::clone(&state.agent_clients.get(client.into()).local_analysis);
    let roots = load_source_roots_for_parser(
        binding.app_data_dir(),
        binding.parser_version(),
        binding.source_environment_label(),
    )
    .await?;
    *state.roots.get(client.into()).write().await = roots.clone();

    Ok(SourcesDto {
        roots,
        coverage: state.coverages.get(client.into()).read().await.clone(),
        scan: state.scans.get(client.into()).snapshot(),
    })
}

/// 只读取当前客户端本机数据根，不触发扫描或索引写入。
#[tauri::command]
pub(crate) async fn get_source_roots(
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
) -> Result<Vec<crate::dto::SourceRootDto>, String> {
    let binding = Arc::clone(&state.agent_clients.get(client.into()).local_analysis);
    let roots = load_source_roots_for_parser(
        binding.app_data_dir(),
        binding.parser_version(),
        binding.source_environment_label(),
    )
    .await?;
    *state.roots.get(client.into()).write().await = roots.clone();
    Ok(roots)
}
