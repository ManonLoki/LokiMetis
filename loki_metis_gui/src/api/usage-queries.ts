import type { QueryClient } from "@tanstack/react-query";

import { type AgentClientKind, type PrivacySettingsDto } from "./usage";

/** 扫描进行中时的状态轮询间隔：足够快显得实时，又不会打满 Tauri IPC 桥。 */
export const SCAN_STATUS_POLL_INTERVAL_MS = 1_500;

// TanStack Query 的 queryKey 是一个数组（如 ['usage-overview', 'codex']），
// 它的缓存 API（cancelQueries/removeQueries/invalidateQueries）在只给出
// 前缀（如 ['usage-overview']）时，会匹配所有以这个前缀开头的完整
// key——不需要手动枚举每个客户端、每个筛选条件组合出来的具体 key，
// 这也是本文件反复出现“只传前缀数组”写法的原因。

/** 让所有依赖本机索引的视图在扫描、移除或清空后读取同一代数据。 */
export async function invalidateLocalUsageQueries(
  queryClient: QueryClient,
  client?: AgentClientKind,
): Promise<void> {
  const suffix = client ? [client] : [];
  const invalidations: Promise<unknown>[] = [
    queryClient.invalidateQueries({ queryKey: ["usage-overview", ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ["usage-statistics", ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ["usage-charts", ...suffix] }),
    queryClient.resetQueries({ queryKey: ["usage-calls", ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ["usage-sources", ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ["source-roots", ...suffix] }),
    queryClient.invalidateQueries({ queryKey: ["privacy-settings", ...suffix] }),
  ];
  if (client) {
    invalidations.push(
      queryClient.invalidateQueries({ queryKey: ["usage-overview", "all"] }),
      queryClient.invalidateQueries({ queryKey: ["usage-charts", "all"] }),
      queryClient.resetQueries({ queryKey: ["usage-calls", "all"] }),
    );
  }
  await Promise.all(invalidations);
}

/** 将 runtime 全局设置同步进所有客户端缓存，同时保留各自独立索引元数据。 */
// PrivacySettingsDto 里的字段其实混合了两种作用域：像语言偏好、刷新间隔、
// 已启用 Agent、WorkBuddy 开关这些是“全局唯一一份”的设置（不区分
// Codex/Claude），但索引位置、索引大小这些字段是“每个客户端各自独立”的。缓存却是按
// ['privacy-settings', client] 分客户端存的，所以改一次全局设置后，
// 必须把变化同步广播进两个客户端各自的缓存条目里。
// `setQueriesData`（复数）能一次性匹配前缀命中的全部 query（这里是
// 两个客户端各一条），对每条都跑同一个更新函数——只覆盖全局字段，
// 用展开运算符 `...existing` 保留每条缓存里客户端专属的字段不被覆盖。
// Privacy 页（语言/扫描间隔）与数据源页（数据读取模式）都会
// 写入这些全局字段，因此这个广播助手在两个页面间共享，而不是各自
// 复制一份。
export function synchronizeGlobalPrivacySettings(
  queryClient: QueryClient,
  settings: PrivacySettingsDto,
): void {
  queryClient.setQueriesData<PrivacySettingsDto>(
    { queryKey: ["privacy-settings"] },
    (existing) =>
      existing
        ? {
            ...existing,
            languagePreference: settings.languagePreference,
            scanIntervalMinutes: settings.scanIntervalMinutes,
            retentionDays: settings.retentionDays,
            availableAiTypes: settings.availableAiTypes,
            enabledAgents: settings.enabledAgents,
            workbuddyStatsEnabled: settings.workbuddyStatsEnabled,
          }
        : existing,
  );
}
