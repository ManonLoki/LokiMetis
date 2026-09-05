import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useAtomValue } from 'jotai';

import {
  getPrivacySettings,
  getWorkbuddyStatistics,
  timeStandardQueryKey,
  type WorkbuddyDailyBucketDto,
  type WorkbuddyHourlyTrendDto,
  type WorkbuddyWindowDto,
  type UsageWindow,
} from '../api/usage';
import {
  timeStandardAtom,
  type WorkbuddyChartGroup,
  type WorkbuddyChartMetric,
} from '../state/page-session';

export type { WorkbuddyChartGroup, WorkbuddyChartMetric };

/** WorkBuddy 统计每十秒读取一次本机数据，与其余看板刷新节奏一致。 */
export const WORKBUDDY_REFRESH_INTERVAL_MS = 10_000;
export const SESSIONS_SERIES_COLOR = '#2563eb';
export const REQUESTS_SERIES_COLOR = '#7c3aed';
export const TOKENS_SERIES_COLOR = '#16a34a';
export const CREDITS_SERIES_COLOR = '#f59e0b';

/** 把 `YYYY-MM-DD` 民用日期压缩为图表横轴使用的 `MM-DD` 短标签。 */
export function shortDayLabel(date: string): string {
  return date.slice(5);
}

/** 把秒数格式化为一位小数的分钟展示。 */
export function formatMinutes(seconds: number): string {
  return (seconds / 60).toFixed(1);
}

/** 读取当前窗口；缺失时回退到快照中的第一项。 */
export function selectWorkbuddyWindow(
  windows: WorkbuddyWindowDto[],
  selected: UsageWindow,
): WorkbuddyWindowDto | undefined {
  return windows.find((window) => window.window === selected) ?? windows[0];
}

/** 按窗口民用日过滤每日趋势，保持升序供图表从左到右阅读。 */
export function filterWorkbuddyDailyBuckets(
  buckets: WorkbuddyDailyBucketDto[],
  window: WorkbuddyWindowDto | undefined,
): WorkbuddyDailyBucketDto[] {
  const dateSet = new Set(window?.dates ?? []);
  if (dateSet.size === 0) {
    return [];
  }
  return buckets.filter((bucket) => dateSet.has(bucket.date));
}

/** 今日/昨日单日窗口读取 core 固定 24 小时趋势；多日窗口没有小时趋势，返回 `undefined`。 */
export function selectWorkbuddyHourlyTrend(
  trends: WorkbuddyHourlyTrendDto[],
  window: UsageWindow,
): WorkbuddyHourlyTrendDto | undefined {
  return trends.find((trend) => trend.window === window);
}

/** 用量日表按日期从新到旧排列，与物理 Agent 逐日趋势一致。 */
export function newestWorkbuddyDailyBuckets(
  buckets: WorkbuddyDailyBucketDto[],
): WorkbuddyDailyBucketDto[] {
  return [...buckets].sort((left, right) => right.date.localeCompare(left.date));
}

/** 计算窗口 trace 错误率；没有 trace 时保持未提供。 */
export function workbuddyTraceErrorRate(window: WorkbuddyWindowDto | undefined): number | null {
  if (!window || window.traceTotalCount === 0) {
    return null;
  }
  return window.traceErrorCount / window.traceTotalCount;
}

/** 图表用量固定分组选项，顺序与页头下拉一致。 */
export const WORKBUDDY_CHART_GROUPS: readonly WorkbuddyChartGroup[] = ['day', 'traceStatus'];

/** 返回当前分组允许的指标；切换分组时必须落到这个集合里。 */
export function workbuddyChartMetricsForGroup(
  group: WorkbuddyChartGroup,
): readonly WorkbuddyChartMetric[] {
  return group === 'day'
    ? [
        'tokens',
        'inputTokens',
        'cachedInputTokens',
        'uncachedInputTokens',
        'outputTokens',
        'requests',
        'sessions',
        'credits',
      ]
    : ['traceCount'];
}

/** 分组变化后若当前指标非法，回退到该分组的第一项。 */
export function coerceWorkbuddyChartMetric(
  group: WorkbuddyChartGroup,
  metric: WorkbuddyChartMetric,
): WorkbuddyChartMetric {
  const options = workbuddyChartMetricsForGroup(group);
  return options.includes(metric) ? metric : options[0]!;
}

/** 日桶上读取图表用量当前指标；Trace 计数不属于日桶。 */
export function workbuddyDailyMetricValue(
  bucket: WorkbuddyDailyBucketDto,
  metric: WorkbuddyChartMetric,
): number | null {
  switch (metric) {
    case 'requests':
      return bucket.requestCount;
    case 'sessions':
      return bucket.sessionCount;
    case 'credits':
      return bucket.credits;
    case 'inputTokens':
      return bucket.inputTokens;
    case 'cachedInputTokens':
      return bucket.cachedInputTokens;
    case 'uncachedInputTokens':
      return bucket.uncachedInputTokens;
    case 'outputTokens':
      return bucket.outputTokens;
    case 'tokens':
      return bucket.tokens;
    case 'traceCount':
      return 0;
  }
}

/** 已完成 Trace = 总数减去错误与取消，避免把未提供的分项画成零以外的值。 */
export function workbuddyCompletedTraceCount(window: WorkbuddyWindowDto): number {
  return Math.max(0, window.traceTotalCount - window.traceErrorCount - window.traceCancelledCount);
}

/** 读取开关与统计快照；关闭时不发起统计 command。 */
export function useWorkbuddyStatisticsQuery(options?: { keepPrevious?: boolean }) {
  const timeStandard = useAtomValue(timeStandardAtom);
  const privacyQuery = useQuery({
    queryFn: () => getPrivacySettings('codex'),
    queryKey: ['privacy-settings', 'codex'],
  });
  const enabled = privacyQuery.data?.workbuddyStatsEnabled === true;
  const statisticsQuery = useQuery({
    enabled,
    placeholderData: options?.keepPrevious ? keepPreviousData : undefined,
    queryFn: () => getWorkbuddyStatistics(timeStandard),
    queryKey: ['workbuddy-statistics', ...timeStandardQueryKey(timeStandard)],
    refetchInterval: (query) =>
      query.state.fetchStatus === 'fetching' ? false : WORKBUDDY_REFRESH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });
  return { enabled, privacyQuery, statisticsQuery };
}
