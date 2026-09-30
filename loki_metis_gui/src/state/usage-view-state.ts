import { atom } from "jotai";

import {
  LOCAL_TIME_STANDARD,
  type AgentClientKind,
  type ChartPreferencesDto,
  type TimeStandard,
  type UsageDimension,
  type UsageViewKind,
  type UsageWindow,
} from "../api/usage-types";
import { agentClientAtom, usageViewAtom } from "./agent-client";

/** 概览窗口按四个只读视图保留到当前桌面进程结束。 */
const overviewWindowsAtom = atom<Record<UsageViewKind, UsageWindow>>({
  all: "today",
  claudeCode: "today",
  codex: "today",
  grokBuildCli: "today",
  workbuddy: "today",
});

/** 读取或写入当前只读视图的概览窗口。 */
export const overviewWindowAtom = atom(
  (get) => get(overviewWindowsAtom)[get(usageViewAtom)],
  (get, set, window: UsageWindow) => {
    const view = get(usageViewAtom);
    set(overviewWindowsAtom, { ...get(overviewWindowsAtom), [view]: window });
  },
);

/** 保存单个物理 Agent 在当前进程内的用量视图选择。 */
interface UsageSessionPreferences {
  dimension: UsageDimension;
  window: UsageWindow;
}

/** 用量窗口与维度按物理 Agent 保留，切换视图时不串值。 */
const usagePreferencesAtom = atom<Record<AgentClientKind, UsageSessionPreferences>>({
  claudeCode: { dimension: "model", window: "thisWeek" },
  codex: { dimension: "model", window: "thisWeek" },
  grokBuildCli: { dimension: "model", window: "thisWeek" },
});

/** WorkBuddy 用量页窗口只在当前进程内保留，不与物理 Agent 串值。 */
const workbuddyUsageWindowAtom = atom<UsageWindow>("thisWeek");

/** WorkBuddy 用量页分组维度只在当前进程内保留，不与物理 Agent 串值。 */
const workbuddyUsageDimensionAtom = atom<UsageDimension>("model");

/** 读取或写入当前物理 Agent 的用量窗口。 */
export const usageWindowAtom = atom(
  (get) =>
    get(usageViewAtom) === "workbuddy"
      ? get(workbuddyUsageWindowAtom)
      : get(usagePreferencesAtom)[get(agentClientAtom)].window,
  (get, set, window: UsageWindow) => {
    if (get(usageViewAtom) === "workbuddy") {
      set(workbuddyUsageWindowAtom, window);
      return;
    }
    const client = get(agentClientAtom);
    const current = get(usagePreferencesAtom);
    set(usagePreferencesAtom, {
      ...current,
      [client]: { ...current[client], window },
    });
  },
);

/** 读取或写入当前物理 Agent 或 WorkBuddy 的用量分组维度。 */
export const usageDimensionAtom = atom(
  (get) =>
    get(usageViewAtom) === "workbuddy"
      ? get(workbuddyUsageDimensionAtom)
      : get(usagePreferencesAtom)[get(agentClientAtom)].dimension,
  (get, set, dimension: UsageDimension) => {
    if (get(usageViewAtom) === "workbuddy") {
      set(workbuddyUsageDimensionAtom, dimension);
      return;
    }
    const client = get(agentClientAtom);
    const current = get(usagePreferencesAtom);
    set(usagePreferencesAtom, {
      ...current,
      [client]: { ...current[client], dimension },
    });
  },
);

/** 为一个只读视图建立互不共享引用的图表默认值。 */
function defaultChartPreferences(view: UsageViewKind): ChartPreferencesDto {
  return {
    dimension: view === "all" ? "agent" : "model",
    distributionMetric: "totalTokens",
    overviewWindow: "today",
    tokenMetrics: ["totalTokens", "inputTokens", "outputTokens"],
    usageWindow: "thisWeek",
  };
}

/** 图表偏好按四个只读视图保留到当前桌面进程结束。 */
const chartPreferencesByViewAtom = atom<Record<UsageViewKind, ChartPreferencesDto>>({
  all: defaultChartPreferences("all"),
  claudeCode: defaultChartPreferences("claudeCode"),
  codex: defaultChartPreferences("codex"),
  grokBuildCli: defaultChartPreferences("grokBuildCli"),
  workbuddy: defaultChartPreferences("workbuddy"),
});

/** 读取或完整替换当前只读视图的图表偏好。 */
export const chartPreferencesAtom = atom(
  (get) => get(chartPreferencesByViewAtom)[get(usageViewAtom)],
  (get, set, preferences: ChartPreferencesDto) => {
    const view = get(usageViewAtom);
    set(chartPreferencesByViewAtom, {
      ...get(chartPreferencesByViewAtom),
      [view]: preferences,
    });
  },
);

/** WorkBuddy 图表用量可用的本机真实分组，不发明模型或项目维度。 */
export type WorkbuddyChartGroup = "day" | "traceStatus";

/** WorkBuddy 图表用量可选指标；Trace 状态只有计数。 */
export type WorkbuddyChartMetric =
  | "tokens"
  | "inputTokens"
  | "cachedInputTokens"
  | "uncachedInputTokens"
  | "outputTokens"
  | "requests"
  | "sessions"
  | "credits"
  | "traceCount";

/** WorkBuddy 图表用量的分组与指标，只在当前进程内保留，不与看板用量窗口串值。 */
export interface WorkbuddyChartPreferences {
  group: WorkbuddyChartGroup;
  metric: WorkbuddyChartMetric;
}

/** WorkBuddy 图表用量搜索默认从日桶 Token 分布开始。 */
const defaultWorkbuddyChartPreferences: WorkbuddyChartPreferences = {
  group: "day",
  metric: "tokens",
};

/** 保存 WorkBuddy 图表用量自己的分组与指标。 */
export const workbuddyChartPreferencesAtom = atom<WorkbuddyChartPreferences>(
  defaultWorkbuddyChartPreferences,
);

/** 看板页头时间标准；新桌面进程固定从当地时间开始。 */
export const timeStandardAtom = atom<TimeStandard>({ ...LOCAL_TIME_STANDARD });
