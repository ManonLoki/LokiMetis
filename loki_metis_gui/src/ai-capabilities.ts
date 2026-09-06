import type { MonitorAiTool, MonitorAiToolDescriptor } from "./api/monitor";
import type { AgentClientKind, AvailableAiTypeDto } from "./api/usage-types";

/**
 * 按后端统一目录顺序去重监控工具。
 *
 * 公开范围完全由 core 投影出的 capability 决定，前端不复制第二份白名单。
 */
export function selectAvailableMonitorAiTools(
  descriptors: readonly MonitorAiToolDescriptor[],
): MonitorAiToolDescriptor[] {
  const seen = new Set<MonitorAiTool>();
  return descriptors.filter((item) => {
    if (seen.has(item.tool)) return false;
    seen.add(item.tool);
    return true;
  });
}

/** 从后端当前可用监控目录中规范化启用集合，丢弃历史隐藏项。 */
export function selectEnabledAvailableMonitorTools(
  enabled: readonly MonitorAiTool[],
  available: readonly MonitorAiToolDescriptor[],
): MonitorAiTool[] {
  const enabledSet = new Set(enabled);
  return selectAvailableMonitorAiTools(available)
    .map((item) => item.tool)
    .filter((tool) => enabledSet.has(tool));
}

/** 仅当事件或浮窗工具能在后端当前目录中匹配时返回其展示描述。 */
export function findAvailableMonitorAiTool(
  available: readonly MonitorAiToolDescriptor[],
  tool: unknown,
): MonitorAiToolDescriptor | null {
  return (
    selectAvailableMonitorAiTools(available).find((item) => item.tool === tool) ?? null
  );
}

/** 判断运行时值是否是看板已实现的物理客户端。 */
function isDashboardClient(value: unknown): value is AgentClientKind {
  return value === "codex" || value === "claudeCode" || value === "grokBuildCli";
}

/** 从后端统一目录中选择可供看板展示的物理客户端，并按目录顺序去重。 */
export function selectDashboardClientOptions(
  available: readonly AvailableAiTypeDto[],
): Array<{ label: string; value: AgentClientKind }> {
  const seen = new Set<AgentClientKind>();
  return available.flatMap((item) => {
    if (!isDashboardClient(item.value) || seen.has(item.value)) return [];
    seen.add(item.value);
    return [{ label: item.name, value: item.value }];
  });
}

/** 返回后端统一目录中的 WorkBuddy 看板入口；未提供或重复时只取第一项。 */
export function selectDashboardWorkbuddyOption(
  available: readonly AvailableAiTypeDto[],
): AvailableAiTypeDto | null {
  return available.find((item) => item.value === "workbuddy") ?? null;
}

/**
 * 按后端可用目录规范化看板已启用客户端。
 *
 * 该边界同时保证未知值不会进入 Jotai 当前客户端或后台扫描查询。
 */
export function selectEnabledDashboardClients(
  enabled: readonly AgentClientKind[],
  available: readonly AvailableAiTypeDto[],
): AgentClientKind[] {
  const enabledSet = new Set(enabled);
  return selectDashboardClientOptions(available)
    .map((item) => item.value)
    .filter((client) => enabledSet.has(client));
}
