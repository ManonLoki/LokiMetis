import { invoke } from '@tauri-apps/api/core';

import type {
  AgentClientKind,
  UsageClientKind,
  UsageViewKind,
  ClearIndexResultDto,
  AddRootCandidateDto,
  LocalIndexRefreshTrigger,
  PrivacySettingsDto,
  ScanStatusDto,
  RootCandidateDto,
  RootDiscoveryStatusDto,
  RootDiscoveryScope,
  ManualAddSourceRootDto,
  SourceRootMutationDto,
  SourceRootDto,
  SourcesDto,
  UsageCallsPageDto,
  UsageCallsQueryDto,
  UsageDimension,
  UsageChartDimension,
  UsageChartDto,
  UsageOverviewDto,
  UsageStatisticsDto,
  UsageWindow,
  TimeStandard,
  WorkbuddySourceStatusDto,
  WorkbuddyStatisticsDto,
  WorkbuddyUsageDetailsDto,
} from './usage-types';

export * from './usage-types';

// 本文件是前端唯一与 Rust backend 通信的边界层：`invoke(命令名, 参数)`
// 对应 Rust 那边用 `#[tauri::command]` 标记、并在 `tauri::generate_handler!`
// 里注册过的函数（参见 bifang_ai_usage_dashboard_gui/src/lib.rs）。

/** 用稳定 command 名集中前端与 Tauri 的全部交互边界。 */
export const usageCommands = {
  overview: 'get_usage_overview',
  calls: 'get_usage_calls',
  statistics: 'get_usage_statistics',
  charts: 'get_usage_charts',
  sources: 'get_sources',
  sourceRoots: 'get_source_roots',
  startRootDiscovery: 'start_root_discovery',
  rootDiscoveryStatus: 'get_root_discovery_status',
  rootCandidates: 'list_root_candidates',
  addRootCandidate: 'add_root_candidate',
  cancelRootDiscovery: 'cancel_root_discovery',
  manualAddSourceRoot: 'manual_add_source_root',
  localScanStatus: 'get_local_scan_status',
  refreshLocalIndexes: 'refresh_local_indexes',
  reindexSourceRoot: 'reindex_source_root',
  privacy: 'get_privacy_settings',
  setDeviceUsername: 'set_device_username',
  setScanInterval: 'set_scan_interval',
  setRetentionDays: 'set_retention_days',
  setEnabledAgents: 'set_enabled_agents',
  setWorkbuddyStatsEnabled: 'set_workbuddy_stats_enabled',
  workbuddyStatistics: 'get_workbuddy_statistics',
  workbuddyUsageStatistics: 'get_workbuddy_usage_statistics',
  workbuddySourceStatus: 'get_workbuddy_source_status',
  clearIndex: 'clear_local_index',
  setSourceRootEnabled: 'set_source_root_enabled',
  renameSourceRoot: 'rename_source_root',
  removeSourceRoot: 'remove_source_root',
  setPrimarySourceRoot: 'set_primary_source_root',
} as const;

/** 读取概览窗口。 */
export async function getUsageOverview(
  client: UsageViewKind,
  timeStandard: TimeStandard,
): Promise<UsageOverviewDto> {
  return invoke<UsageOverviewDto>(usageCommands.overview, { client, timeStandard });
}

/** 按固定筛选、排序与不透明游标读取一页调用，不传 limit 或表达式。 */
export async function getUsageCalls(
  client: UsageViewKind,
  query: UsageCallsQueryDto,
  timeStandard: TimeStandard,
): Promise<UsageCallsPageDto> {
  return invoke<UsageCallsPageDto>(usageCommands.calls, { client, query, timeStandard });
}

/** 按后端固定枚举读取本机统计，不传递路径、limit 或任意查询表达式。 */
export async function getUsageStatistics(
  client: AgentClientKind,
  window: UsageWindow,
  dimension: UsageDimension,
  timeStandard: TimeStandard,
): Promise<UsageStatisticsDto> {
  return invoke<UsageStatisticsDto>(usageCommands.statistics, {
    client,
    dimension,
    timeStandard,
    window,
  });
}

/** 读取固定时间桶趋势与维度分布，不传递路径或任意查询表达式。 */
export async function getUsageCharts(
  client: UsageViewKind,
  window: UsageWindow,
  dimension: UsageChartDimension,
  timeStandard: TimeStandard,
): Promise<UsageChartDto> {
  return invoke<UsageChartDto>(usageCommands.charts, { client, dimension, timeStandard, window });
}

/** 读取当前客户端的官方能力、数据目录覆盖和扫描状态。 */
export async function getSources(client: AgentClientKind): Promise<SourcesDto> {
  return invoke<SourcesDto>(usageCommands.sources, { client });
}

/** 只读取本机数据目录，初始化向导调用时不会触发扫描状态。 */
export async function getSourceRoots(client: AgentClientKind): Promise<SourceRootDto[]> {
  return invoke<SourceRootDto[]>(usageCommands.sourceRoots, { client });
}

/** 启动不打开会话文件的数据源发现。 */
export async function startRootDiscovery(
  scope: RootDiscoveryScope,
): Promise<RootDiscoveryStatusDto> {
  return invoke<RootDiscoveryStatusDto>(usageCommands.startRootDiscovery, { scope });
}

/** 轮询数据源发现状态。 */
export async function getRootDiscoveryStatus(): Promise<RootDiscoveryStatusDto> {
  return invoke<RootDiscoveryStatusDto>(usageCommands.rootDiscoveryStatus);
}

/** 读取当前任务完整路径候选。 */
export async function listRootCandidates(): Promise<RootCandidateDto[]> {
  return invoke<RootCandidateDto[]>(usageCommands.rootCandidates);
}

/** 添加单个候选并排入对应客户端的后台统计。 */
export async function addRootCandidate(candidateId: string): Promise<AddRootCandidateDto> {
  return invoke<AddRootCandidateDto>(usageCommands.addRootCandidate, { candidateId });
}

/** 取消数据源发现。 */
export async function cancelRootDiscovery(): Promise<RootDiscoveryStatusDto> {
  return invoke<RootDiscoveryStatusDto>(usageCommands.cancelRootDiscovery);
}

/** 读取对应客户端的后台本机 Token 统计状态。 */
export async function getLocalScanStatus(client: AgentClientKind): Promise<ScanStatusDto> {
  return invoke<ScanStatusDto>(usageCommands.localScanStatus, { client });
}

/** 在批准的发现终态或直接手动登记后，统一执行一次近 30 日索引。 */
export async function refreshLocalIndexes(
  clients: AgentClientKind[],
  trigger: LocalIndexRefreshTrigger,
): Promise<ScanStatusDto[]> {
  return invoke<ScanStatusDto[]>(usageCommands.refreshLocalIndexes, { clients, trigger });
}

/** 强制重建一个已启用数据根，只传客户端与稳定根 ID。 */
export async function reindexSourceRoot(
  client: AgentClientKind,
  rootId: string,
): Promise<ScanStatusDto> {
  return invoke<ScanStatusDto>(usageCommands.reindexSourceRoot, { client, rootId });
}

/** 读取仅本机模式与本产品索引位置说明。 */
export async function getPrivacySettings(client: UsageClientKind): Promise<PrivacySettingsDto> {
  return invoke<PrivacySettingsDto>(usageCommands.privacy, { client });
}

/** 保存或清除设备用户名；空字符串由后端解释为清除且不再自动回填。 */
export async function setDeviceUsername(
  client: UsageClientKind,
  deviceUsername: string,
): Promise<PrivacySettingsDto> {
  return invoke<PrivacySettingsDto>(usageCommands.setDeviceUsername, { client, deviceUsername });
}

/** 保存用户显式开放的本机 Agent 集合。 */
export async function setEnabledAgents(
  client: UsageClientKind,
  agents: UsageClientKind[],
): Promise<PrivacySettingsDto> {
  return invoke<PrivacySettingsDto>(usageCommands.setEnabledAgents, { client, agents });
}

/** 保存单一扫描间隔；只改本机周期扫描节奏。 */
export async function setScanInterval(
  client: UsageClientKind,
  minutes: number,
): Promise<PrivacySettingsDto> {
  return invoke<PrivacySettingsDto>(usageCommands.setScanInterval, { client, minutes });
}

/** 保存派生用量自动清理天数；保存不立刻删数据。 */
export async function setRetentionDays(
  client: UsageClientKind,
  days: number,
): Promise<PrivacySettingsDto> {
  return invoke<PrivacySettingsDto>(usageCommands.setRetentionDays, { client, days });
}

/** 保存 WorkBuddy 本地统计开关；关闭时后续读取一律被拒绝。 */
export async function setWorkbuddyStatsEnabled(
  client: UsageClientKind,
  enabled: boolean,
): Promise<PrivacySettingsDto> {
  return invoke<PrivacySettingsDto>(usageCommands.setWorkbuddyStatsEnabled, { client, enabled });
}

/** 读取 WorkBuddy 本地用量统计快照；开关关闭时后端直接拒绝。 */
export async function getWorkbuddyStatistics(
  timeStandard: TimeStandard,
): Promise<WorkbuddyStatisticsDto> {
  return invoke<WorkbuddyStatisticsDto>(usageCommands.workbuddyStatistics, { timeStandard });
}

/** 一次读取同一批 WorkBuddy project JSONL 请求统计与实际模型 Token 明细。 */
export async function getWorkbuddyUsageStatistics(
  window: UsageWindow,
  dimension: UsageDimension,
  timeStandard: TimeStandard,
): Promise<WorkbuddyUsageDetailsDto> {
  return invoke<WorkbuddyUsageDetailsDto>(usageCommands.workbuddyUsageStatistics, {
    dimension,
    timeStandard,
    window,
  });
}

/** 读取数据源页展示的 WorkBuddy 只读发现状态；开关关闭时后端不触碰磁盘。 */
export async function getWorkbuddySourceStatus(): Promise<WorkbuddySourceStatusDto> {
  return invoke<WorkbuddySourceStatusDto>(usageCommands.workbuddySourceStatus);
}

/** 仅清空当前客户端的本产品索引，不删除会话、登录状态或原始文件。 */
export async function clearLocalIndex(client: AgentClientKind): Promise<ClearIndexResultDto> {
  return invoke<ClearIndexResultDto>(usageCommands.clearIndex, { client });
}

/** 由 Rust 侧原生目录选择器登记或深搜；前端不接收或传回路径。 */
export async function manualAddSourceRoot(
  client: AgentClientKind,
): Promise<ManualAddSourceRootDto> {
  return invoke<ManualAddSourceRootDto>(usageCommands.manualAddSourceRoot, { client });
}

/** 按稳定根 ID 启用或停用数据目录，不接受任意文件系统路径。 */
export async function setSourceRootEnabled(
  client: AgentClientKind,
  rootId: string,
  enabled: boolean,
): Promise<SourceRootMutationDto> {
  return invoke<SourceRootMutationDto>(usageCommands.setSourceRootEnabled, {
    client,
    rootId,
    enabled,
  });
}

/** 按稳定根 ID 更新安全短别名，不改变数据目录路径。 */
export async function renameSourceRoot(
  client: AgentClientKind,
  rootId: string,
  alias: string,
): Promise<SourceRootMutationDto> {
  return invoke<SourceRootMutationDto>(usageCommands.renameSourceRoot, { client, rootId, alias });
}

/** 按稳定根 ID 移除本产品索引，不修改当前客户端原始文件。 */
export async function removeSourceRoot(
  client: AgentClientKind,
  rootId: string,
): Promise<SourceRootMutationDto> {
  return invoke<SourceRootMutationDto>(usageCommands.removeSourceRoot, { client, rootId });
}

/** 按稳定根 ID 设置或清除 Codex 主数据目录；绝对路径始终留在 Rust backend。 */
export async function setPrimarySourceRoot(
  client: AgentClientKind,
  rootId: string | null,
): Promise<SourceRootMutationDto> {
  if (client !== 'codex') {
    throw new Error('只有 Codex 数据目录可设为主数据目录。');
  }
  return invoke<SourceRootMutationDto>(usageCommands.setPrimarySourceRoot, { client, rootId });
}
