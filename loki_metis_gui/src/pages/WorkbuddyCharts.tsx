import {
  Group,
  NativeSelect,
  Paper,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Text,
} from '@mantine/core';
import { useAtom, useAtomValue } from 'jotai';
import { useTranslation } from 'react-i18next';

import type { UsageWindow, WorkbuddyDailyBucketDto, WorkbuddyWindowDto } from '../api/usage';
import { DistributionChart, type DistributionRow } from '../components/charts/DistributionChart';
import { TimeSeriesChart } from '../components/charts/TimeSeriesChart';
import { TokenTotalDisplay } from '../components/UsageUi';
import {
  chartPreferencesAtom,
  workbuddyChartPreferencesAtom,
  type WorkbuddyChartGroup,
  type WorkbuddyChartMetric,
} from '../state/page-session';
import {
  formatBasisPoints,
  formatCompactTokens,
  formatCredits,
  formatTokens,
} from '../usage-format';
import { usageWindowOrder } from './overview-windows';
import { WorkbuddyQueryGate } from './WorkbuddyGate';
import {
  CREDITS_SERIES_COLOR,
  REQUESTS_SERIES_COLOR,
  SESSIONS_SERIES_COLOR,
  TOKENS_SERIES_COLOR,
  WORKBUDDY_CHART_GROUPS,
  coerceWorkbuddyChartMetric,
  filterWorkbuddyDailyBuckets,
  selectWorkbuddyHourlyTrend,
  selectWorkbuddyWindow,
  shortDayLabel,
  useWorkbuddyStatisticsQuery,
  workbuddyChartMetricsForGroup,
  workbuddyCompletedTraceCount,
  workbuddyDailyMetricValue,
} from './WorkbuddyShared';
import '../charts.css';

/** 看板图表选项卡选中 WorkBuddy：同一页同时展示趋势图与用量分布。 */
export function WorkbuddyCharts() {
  const [filters, setFilters] = useAtom(chartPreferencesAtom);
  return (
    <Stack className="page-stack" data-testid="workbuddy-charts" gap="xl">
      <WorkbuddyChartWindowControl
        onChange={(window) =>
          setFilters({ ...filters, overviewWindow: window, usageWindow: window })
        }
        value={filters.overviewWindow}
      />
      <WorkbuddyOverviewCharts />
      <WorkbuddyUsageDistribution />
    </Stack>
  );
}

/** 图表概览沿用窗口分段控件；今日/昨日按小时、多日窗口按民用日绘制趋势。 */
function WorkbuddyOverviewCharts() {
  const { t } = useTranslation();
  const filters = useAtomValue(chartPreferencesAtom);
  const query = useWorkbuddyStatisticsQuery({ keepPrevious: true });
  if (!query.statisticsQuery.isSuccess) {
    return <WorkbuddyQueryGate {...query} />;
  }

  const selected = selectWorkbuddyWindow(
    query.statisticsQuery.data.windows,
    filters.overviewWindow,
  );
  const hourlyTrend = selectWorkbuddyHourlyTrend(
    query.statisticsQuery.data.hourlyTrends,
    filters.overviewWindow,
  );
  const dailyBuckets = filterWorkbuddyDailyBuckets(
    query.statisticsQuery.data.dailyBuckets,
    selected,
  );
  return (
    <Stack gap="xl">
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('statistics.summary.totalTokens')}
          </Text>
          <TokenTotalDisplay className="window-number" value={selected?.tokens ?? 0} />
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('metric.input')}
          </Text>
          <TokenTotalDisplay className="window-number" value={selected?.inputTokens ?? 0} />
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('metric.cachedInput')}
          </Text>
          <TokenTotalDisplay className="window-number" value={selected?.cachedInputTokens ?? 0} />
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('metric.uncachedInput')}
          </Text>
          <TokenTotalDisplay className="window-number" value={selected?.uncachedInputTokens ?? 0} />
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('metric.output')}
          </Text>
          <TokenTotalDisplay className="window-number" value={selected?.outputTokens ?? 0} />
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('workbuddy.totalRequests')}
          </Text>
          <Text className="window-number" fw={800}>
            {formatTokens(selected?.requestCount ?? 0)}
          </Text>
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('workbuddy.summarySessions')}
          </Text>
          <Text className="window-number" fw={800}>
            {formatTokens(selected?.sessionCount ?? 0)}
          </Text>
        </Paper>
        <Paper className="mini-metric" p="lg" radius="lg" withBorder>
          <Text c="dimmed" size="sm">
            {t('workbuddy.totalCredits')}
          </Text>
          <Text className="window-number" fw={800}>
            {formatCredits(selected?.credits ?? null)}
          </Text>
        </Paper>
      </SimpleGrid>
      {hourlyTrend ? (
        <WorkbuddyTrendCharts
          granularity="hour"
          labels={hourlyTrend.buckets.map((bucket) => bucket.label)}
          points={hourlyTrend.buckets}
        />
      ) : dailyBuckets.length === 0 ? (
        <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
          <Text c="dimmed" ta="center">
            {t('workbuddy.noDailyData')}
          </Text>
        </Paper>
      ) : (
        <WorkbuddyTrendCharts
          granularity="day"
          labels={dailyBuckets.map((bucket) => shortDayLabel(bucket.date))}
          points={dailyBuckets}
        />
      )}
    </Stack>
  );
}

/** 趋势图共用的桶度量；小时桶与日桶都暴露请求、会话、总 Token 与积分。 */
interface WorkbuddyTrendPoint {
  /** 桶内产生过请求的去重会话数量。 */
  sessionCount: number;
  /** 桶内上游请求数。 */
  requestCount: number;
  /** 桶内输入与输出 Token 合计。 */
  tokens: number;
  /** 桶内积分；存在缺失字段时保持未提供。 */
  credits: number | null;
}

/** 绘制请求、会话、Token 与积分趋势图；标题跟随小时/日粒度。 */
function WorkbuddyTrendCharts({
  granularity,
  labels,
  points,
}: {
  granularity: 'hour' | 'day';
  labels: string[];
  points: WorkbuddyTrendPoint[];
}) {
  const { t } = useTranslation();
  if (labels.length === 0) {
    return null;
  }
  const sessionsTitle =
    granularity === 'hour' ? t('workbuddy.hourlySessionsTitle') : t('workbuddy.dailySessionsTitle');
  const requestsTitle =
    granularity === 'hour' ? t('workbuddy.hourlyRequestsTitle') : t('workbuddy.dailyRequestsTitle');
  const tokensTitle =
    granularity === 'hour' ? t('workbuddy.hourlyTokensTitle') : t('workbuddy.dailyTokensTitle');
  const creditsTitle =
    granularity === 'hour' ? t('workbuddy.hourlyCreditsTitle') : t('workbuddy.dailyCreditsTitle');
  return (
    <>
      <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Text fw={700}>{requestsTitle}</Text>
          <TimeSeriesChart
            ariaLabel={requestsTitle}
            formatTooltipValue={(value) => formatTokens(value)}
            formatValue={(value) => formatTokens(value)}
            labels={labels}
            series={[
              {
                color: REQUESTS_SERIES_COLOR,
                id: 'requests',
                label: t('workbuddy.totalRequests'),
                values: points.map((point) => point.requestCount),
              },
            ]}
          />
        </Stack>
      </Paper>
      <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Text fw={700}>{sessionsTitle}</Text>
          <TimeSeriesChart
            ariaLabel={sessionsTitle}
            formatTooltipValue={(value) => formatTokens(value)}
            formatValue={(value) => formatTokens(value)}
            labels={labels}
            series={[
              {
                color: SESSIONS_SERIES_COLOR,
                id: 'sessions',
                label: t('workbuddy.totalSessions'),
                values: points.map((point) => point.sessionCount),
              },
            ]}
          />
        </Stack>
      </Paper>
      <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Text fw={700}>{tokensTitle}</Text>
          <TimeSeriesChart
            ariaLabel={tokensTitle}
            formatTooltipValue={(value) => formatTokens(value)}
            formatValue={(value) => formatCompactTokens(value)}
            labels={labels}
            series={[
              {
                color: TOKENS_SERIES_COLOR,
                id: 'tokens',
                label: t('workbuddy.totalTokens'),
                values: points.map((point) => point.tokens),
              },
            ]}
          />
        </Stack>
      </Paper>
      <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Text fw={700}>{creditsTitle}</Text>
          <TimeSeriesChart
            ariaLabel={creditsTitle}
            formatTooltipValue={(value) => formatCredits(value)}
            formatValue={(value) => formatCredits(value)}
            labels={labels}
            series={[
              {
                color: CREDITS_SERIES_COLOR,
                id: 'credits',
                label: t('workbuddy.totalCredits'),
                values: points.map((point) => point.credits),
              },
            ]}
          />
        </Stack>
      </Paper>
    </>
  );
}

/** 图表用量：时间范围分段 + 本机真实分组/指标 + 横向分布图。 */
function WorkbuddyUsageDistribution() {
  const { t } = useTranslation();
  const filters = useAtomValue(chartPreferencesAtom);
  const [chartSearch, setChartSearch] = useAtom(workbuddyChartPreferencesAtom);
  const query = useWorkbuddyStatisticsQuery({ keepPrevious: true });
  if (!query.statisticsQuery.isSuccess) {
    return <WorkbuddyQueryGate {...query} />;
  }

  const selected = selectWorkbuddyWindow(
    query.statisticsQuery.data.windows,
    filters.overviewWindow,
  );
  const dailyBuckets = filterWorkbuddyDailyBuckets(
    query.statisticsQuery.data.dailyBuckets,
    selected,
  );
  const metric = coerceWorkbuddyChartMetric(chartSearch.group, chartSearch.metric);
  const rows = buildWorkbuddyDistributionRows({
    buckets: dailyBuckets,
    group: chartSearch.group,
    metric,
    shareLabel: (value, total) =>
      t('workbuddy.chartShare', {
        share: formatBasisPoints(total <= 0 ? 0 : Math.round((value / total) * 10_000)),
      }),
    t: (key) => t(key),
    window: selected,
  });
  const groupOptions = WORKBUDDY_CHART_GROUPS.map((value) => ({
    label: t(`workbuddy.chartGroup.${value}`),
    value,
  }));
  const metricOptions = workbuddyChartMetricsForGroup(chartSearch.group).map((value) => ({
    label: t(`workbuddy.chartMetric.${value}`),
    value,
  }));
  return (
    <Stack gap="xl">
      <Stack gap="sm">
        <Group align="end" gap="md">
          <NativeSelect
            data={groupOptions}
            label={t('charts.controls.dimension')}
            onChange={(event) => {
              const group = event.currentTarget.value as WorkbuddyChartGroup;
              setChartSearch({
                group,
                metric: coerceWorkbuddyChartMetric(group, chartSearch.metric),
              });
            }}
            value={chartSearch.group}
          />
          <NativeSelect
            data={metricOptions}
            label={t('charts.controls.metric')}
            onChange={(event) =>
              setChartSearch({
                ...chartSearch,
                metric: event.currentTarget.value as WorkbuddyChartMetric,
              })
            }
            value={metric}
          />
        </Group>
      </Stack>
      <Paper className="chart-panel" p="lg" radius="lg" withBorder>
        <Stack gap="lg">
          <Stack gap={2}>
            <Text fw={700}>{t('charts.distribution.title')}</Text>
            <Text c="dimmed" size="sm">
              {t('workbuddy.chartDistributionDescription')}
            </Text>
          </Stack>
          <DistributionChart
            ariaLabel={t('charts.distribution.barAria')}
            emptyLabel={t('charts.distribution.empty')}
            formatAxisValue={(value) => formatWorkbuddyChartAxis(metric, value)}
            formatValue={(value) =>
              value === null ? t('common.notProvided') : formatWorkbuddyChartAxis(metric, value)
            }
            rows={rows}
          />
        </Stack>
      </Paper>
    </Stack>
  );
}

/** 图表页窗口使用分段控件，与看板用量的统计窗口下拉区分。 */
function WorkbuddyChartWindowControl({
  onChange,
  value,
}: {
  onChange: (window: UsageWindow) => void;
  value: UsageWindow;
}) {
  const { t } = useTranslation();
  return (
    <SegmentedControl
      aria-label={t('charts.controls.window')}
      data={usageWindowOrder.map((window) => ({
        label: t(`window.${window}`),
        value: window,
      }))}
      onChange={(next) => onChange(next as UsageWindow)}
      value={value}
    />
  );
}

/** 把日桶或 Trace 状态映射为分布图行，不发明模型/项目分组。 */
function buildWorkbuddyDistributionRows({
  buckets,
  group,
  metric,
  shareLabel,
  t,
  window,
}: {
  buckets: WorkbuddyDailyBucketDto[];
  group: WorkbuddyChartGroup;
  metric: WorkbuddyChartMetric;
  shareLabel: (value: number, total: number) => string;
  t: (key: string) => string;
  window: WorkbuddyWindowDto | undefined;
}): DistributionRow[] {
  if (group === 'traceStatus') {
    if (!window || window.traceTotalCount === 0) {
      return [];
    }
    const rows = [
      {
        id: 'completed',
        label: t('workbuddy.traceCompleted'),
        value: workbuddyCompletedTraceCount(window),
      },
      { id: 'error', label: t('workbuddy.traceError'), value: window.traceErrorCount },
      { id: 'cancelled', label: t('workbuddy.traceCancelled'), value: window.traceCancelledCount },
    ];
    const total = window.traceTotalCount;
    return rows.map((row) => ({
      ...row,
      remainder: false,
      shareLabel: shareLabel(row.value, total),
    }));
  }
  const total = buckets.reduce(
    (sum, bucket) => sum + (workbuddyDailyMetricValue(bucket, metric) ?? 0),
    0,
  );
  return buckets.map((bucket) => {
    const value = workbuddyDailyMetricValue(bucket, metric);
    return {
      id: bucket.date,
      label: bucket.date,
      remainder: false,
      shareLabel: value === null ? t('common.notProvided') : shareLabel(value, total),
      value,
    };
  });
}

/** 按当前指标格式化分布图轴与精确值。 */
function formatWorkbuddyChartAxis(metric: WorkbuddyChartMetric, value: number): string {
  if (metric === 'credits') {
    return formatCredits(value);
  }
  return formatTokens(value);
}
