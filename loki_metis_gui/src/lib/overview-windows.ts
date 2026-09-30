import type { UsageWindow, WindowUsageDto } from "../api/usage";

/** 固定概览与用量窗口顺序，忽略旧响应中的额外窗口，避免恢复全量累计展示。 */
export const usageWindowOrder: UsageWindow[] = [
  "today",
  "yesterday",
  "thisWeek",
  "lastWeek",
  "thisMonth",
  "lastMonth",
];

/** 概览筛选与装配共用同一六个窗口顺序。 */
export const overviewWindowOrder = usageWindowOrder;

/** 校验六个允许窗口各出现一次，再按固定顺序返回；无关额外字段不参与展示。 */
export function selectOverviewWindows(windows: WindowUsageDto[]): WindowUsageDto[] | null {
  const allowedWindows = windows.filter((candidate) =>
    overviewWindowOrder.includes(candidate.window),
  );
  const uniqueWindows = new Set(allowedWindows.map((candidate) => candidate.window));
  if (
    allowedWindows.length !== overviewWindowOrder.length ||
    uniqueWindows.size !== overviewWindowOrder.length
  ) {
    return null;
  }

  return overviewWindowOrder.flatMap((window) => {
    const match = allowedWindows.find((candidate) => candidate.window === window);
    return match ? [match] : [];
  });
}
