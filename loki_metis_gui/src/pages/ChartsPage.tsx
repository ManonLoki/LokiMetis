import {
  Checkbox,
  Group,
  NativeSelect,
  Paper,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Text,
} from '@mantine/core';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { useAtom, useAtomValue } from 'jotai';
import { useTranslation } from 'react-i18next';

import {
  getUsageCharts,
  timeStandardQueryKey,
  type ChartPreferencesDto,
  type UsageChartBucketDto,
  type UsageChartDimension,
  type UsageChartDistributionMetric,
  type UsageChartTokenMetric,
  type UsageMeasureDto,
  type UsageWindow,
} from '../api/usage';
import { DistributionChart, type DistributionRow } from '../components/charts/DistributionChart';
import { TimeSeriesChart, type TimeSeries } from '../components/charts/TimeSeriesChart';
import {
  FailureState,
  LocalIndexNotice,
  LoadingState,
  TokenTotalDisplay,
} from '../components/UsageUi';
import { chartMetricLabel, displayLabel } from '../i18n/backend-labels';
import { usageViewAtom } from '../state/agent-client';
import { chartPreferencesAtom, timeStandardAtom } from '../state/page-session';
import { formatBasisPoints, formatCompactTokens, formatTokens } from '../usage-format';
import { usageWindowOrder } from './overview-windows';
import { WorkbuddyCharts } from './WorkbuddyCharts';
import '../charts.css';

const CHART_REFRESH_INTERVAL_MS = 10_000;

const tokenMetricOrder: UsageChartTokenMetric[] = [
  'totalTokens',
  'inputTokens',
  'cachedInputTokens',
  'cacheWriteInputTokens',
  'outputTokens',
  'reasoningOutputTokens',
];

const tokenMetricColors: Record<UsageChartTokenMetric, string> = {
  cacheWriteInputTokens: '#8b5cf6',
  cachedInputTokens: '#0ea5e9',
  inputTokens: '#f59e0b',
  outputTokens: '#ef4444',
  reasoningOutputTokens: '#d946ef',
  totalTokens: '#2563eb',
};

const physicalDimensions: UsageChartDimension[] = [
  'model',
  'reasoningEffort',
  'project',
  'thread',
  'root',
];

/** 从图表度量读取所选 Token 指标，并保留未提供状态。 */
function tokenMetricValue(measure: UsageMeasureDto, metric: UsageChartTokenMetric): number | null {
  switch (metric) {
    case 'totalTokens':
      return measure.tokens.totalTokens;
    case 'inputTokens':
      return measure.tokens.inputTokens;
    case 'cachedInputTokens':
      return measure.tokens.cachedInputTokens;
    case 'cacheWriteInputTokens':
      return measure.tokens.cacheWriteInputTokens;
    case 'outputTokens':
      return measure.tokens.outputTokens;
    case 'reasoningOutputTokens':
      return measure.tokens.reasoningOutputTokens;
  }
}

/** 从分布组读取当前用量指标对应的可比较数值。 */
function distributionValue(
  measure: UsageMeasureDto,
  metric: UsageChartDistributionMetric,
): number | null {
  return metric === 'callCount' ? measure.callCount : tokenMetricValue(measure, metric);
}

/** 看板图表选项卡：同一页同时展示趋势图与用量分布。 */
export function ChartsPage() {
  const view = useAtomValue(usageViewAtom);
  if (view === 'workbuddy') {
    return <WorkbuddyCharts />;
  }
  return <LocalChartsPage />;
}

/** 物理 Agent 与全部视图的图表查询与渲染。 */
function LocalChartsPage() {
  const { t } = useTranslation();
  const view = useAtomValue(usageViewAtom);
  const [filters, setFilters] = useAtom(chartPreferencesAtom);
  const timeStandard = useAtomValue(timeStandardAtom);
  const updateFilters = (patch: Partial<ChartPreferencesDto>) => {
    setFilters({ ...filters, ...patch });
  };
  const window = filters.overviewWindow;
  const dimension = filters.dimension;
  const chartQuery = useQuery({
    placeholderData: keepPreviousData,
    queryFn: () => getUsageCharts(view, window, dimension, timeStandard),
    queryKey: ['usage-charts', view, window, dimension, ...timeStandardQueryKey(timeStandard)],
    refetchInterval: (query) =>
      query.state.fetchStatus === 'fetching' ? false : CHART_REFRESH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });
  const chartData = chartQuery.data;
  const series = useMemo<TimeSeries[]>(() => {
    if (!chartData) return [];
    return filters.tokenMetrics.map<TimeSeries>((metric) => ({
      color: tokenMetricColors[metric],
      id: metric,
      label: chartMetricLabel(t, metric),
      values: chartData.buckets.map((bucket) => tokenMetricValue(bucket.measure, metric)),
    }));
  }, [chartData, filters, t]);
  const distributionRows = useMemo<DistributionRow[]>(() => {
    if (!chartData) return [];
    return [...chartData.groups, ...(chartData.remainder ? [chartData.remainder] : [])].map(
      (group) => ({
        id: group.id,
        label: displayLabel(t, group.label, group.labelCode, group.disambiguationIndex),
        remainder: group.remainder,
        shareLabel: t('charts.distribution.totalTokenShare', {
          share: formatBasisPoints(group.totalTokenShareBasisPoints),
        }),
        value: distributionValue(group.measure, filters.distributionMetric),
      }),
    );
  }, [chartData, filters, t]);

  if (chartQuery.isPending) {
    return <LoadingState label={t('charts.loading')} />;
  }
  if (chartQuery.isError) {
    return <FailureState error={chartQuery.error} onRetry={() => void chartQuery.refetch()} />;
  }

  const chart = chartQuery.data;
  const aggregate = chart.fact.value;
  const windowOptions = usageWindowOrder.map((value) => ({
    label: t(`window.${value}`),
    value,
  }));
  const dimensionValues = view === 'all' ? ['agent', ...physicalDimensions] : physicalDimensions;
  const dimensionOptions = dimensionValues
    .filter((value) => view !== 'grokBuildCli' || value !== 'reasoningEffort')
    .map((value) => ({ label: t(`dimension.${value}`), value }));
  const distributionMetricOptions: UsageChartDistributionMetric[] = [
    ...tokenMetricOrder,
    'callCount',
  ];

  return (
    <Stack className="page-stack" data-testid="dashboard-charts" gap="xl">
      <LocalIndexNotice state={chart.indexState} />

      {chart.indexState === 'notScanned' || chart.indexState === 'needsRescan' ? null : (
        <>
          <Stack gap="sm">
            <SegmentedControl
              aria-label={t('charts.controls.window')}
              data={windowOptions}
              onChange={(value) =>
                updateFilters({
                  overviewWindow: value as UsageWindow,
                  usageWindow: value as UsageWindow,
                })
              }
              value={window}
            />
            <div className="chart-dimension-control">
              <NativeSelect
                data={dimensionOptions}
                label={t('charts.controls.dimension')}
                onChange={(event) =>
                  updateFilters({ dimension: event.currentTarget.value as UsageChartDimension })
                }
                value={filters.dimension}
              />
            </div>
          </Stack>

          <OverviewCharts
            buckets={chart.buckets}
            filters={filters}
            onMetricsChange={(metrics) => updateFilters({ tokenMetrics: metrics })}
            series={series}
            totalCalls={aggregate.callCount}
            totalInput={aggregate.tokens.inputTokens}
            totalOutput={aggregate.tokens.outputTokens}
            totalTokens={aggregate.tokens.totalTokens}
          />
          <UsageDistribution
            metric={filters.distributionMetric}
            metricOptions={distributionMetricOptions}
            onMetricChange={(metric) => updateFilters({ distributionMetric: metric })}
            rows={distributionRows}
          />
        </>
      )}
    </Stack>
  );
}

/** 渲染可组合 Token 趋势与独立调用数趋势。 */
function OverviewCharts({
  buckets,
  filters,
  onMetricsChange,
  series,
  totalCalls,
  totalInput,
  totalOutput,
  totalTokens,
}: {
  buckets: UsageChartBucketDto[];
  filters: ChartPreferencesDto;
  onMetricsChange: (metrics: UsageChartTokenMetric[]) => void;
  series: TimeSeries[];
  totalCalls: number;
  totalInput: number;
  totalOutput: number;
  totalTokens: number;
}) {
  const { t } = useTranslation();
  const labels = buckets.map((bucket) => bucket.label);
  const callSeries: TimeSeries[] = [
    {
      color: '#16a34a',
      id: 'callCount',
      label: chartMetricLabel(t, 'callCount'),
      values: buckets.map((bucket) => bucket.measure.callCount),
    },
  ];
  return (
    <Stack gap="lg">
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
        {[
          { isToken: true, label: chartMetricLabel(t, 'totalTokens'), value: totalTokens },
          { isToken: true, label: chartMetricLabel(t, 'inputTokens'), value: totalInput },
          { isToken: true, label: chartMetricLabel(t, 'outputTokens'), value: totalOutput },
          { isToken: false, label: chartMetricLabel(t, 'callCount'), value: totalCalls },
        ].map(({ label, value, isToken }) => (
          <Paper className="mini-metric" key={label} p="lg" radius="lg" withBorder>
            <Text c="dimmed" size="sm">
              {label}
            </Text>
            {isToken ? (
              <TokenTotalDisplay className="window-number" value={value} />
            ) : (
              <Text className="window-number" fw={800}>
                {formatTokens(value)}
              </Text>
            )}
          </Paper>
        ))}
      </SimpleGrid>

      <Paper className="chart-panel" p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Stack gap={2}>
            <Text fw={700}>{t('charts.overview.tokensTitle')}</Text>
            <Text c="dimmed" size="sm">
              {t('charts.overview.tokensDescription')}
            </Text>
          </Stack>
          <Checkbox.Group
            aria-label={t('charts.overview.metricSelectorAria')}
            onChange={(values) => {
              if (values.length > 0) onMetricsChange(values as UsageChartTokenMetric[]);
            }}
            value={filters.tokenMetrics}
          >
            <Group gap="md">
              {tokenMetricOrder.map((metric) => (
                <Checkbox
                  key={metric}
                  label={chartMetricLabel(t, metric)}
                  size="xs"
                  value={metric}
                />
              ))}
            </Group>
          </Checkbox.Group>
          <TimeSeriesChart
            ariaLabel={t('charts.overview.tokensChartAria')}
            formatTooltipValue={(value) => formatTokens(value)}
            formatValue={(value) => formatCompactTokens(value)}
            labels={labels}
            series={series}
          />
        </Stack>
      </Paper>

      <Paper className="chart-panel" p="lg" radius="lg" withBorder>
        <Stack gap="md">
          <Text fw={700}>{t('charts.overview.callsTitle')}</Text>
          <TimeSeriesChart
            ariaLabel={t('charts.overview.callsChartAria')}
            formatTooltipValue={(value) => formatTokens(value)}
            formatValue={(value) => formatTokens(value)}
            labels={labels}
            series={callSeries}
          />
        </Stack>
      </Paper>
    </Stack>
  );
}

/** 渲染固定横向柱状图形式的 Top 10 加其余项分布。 */
function UsageDistribution({
  metric,
  metricOptions,
  onMetricChange,
  rows,
}: {
  metric: UsageChartDistributionMetric;
  metricOptions: UsageChartDistributionMetric[];
  onMetricChange: (metric: UsageChartDistributionMetric) => void;
  rows: DistributionRow[];
}) {
  const { t } = useTranslation();
  const isCalls = metric === 'callCount';
  const formatAxisValue = (value: number) =>
    isCalls ? formatTokens(value) : formatCompactTokens(value);
  const formatValue = (value: number | null) => {
    if (value === null) return t('common.notProvided');
    return isCalls ? formatTokens(value) : `${formatCompactTokens(value)} · ${formatTokens(value)}`;
  };
  return (
    <Paper className="chart-panel" p="lg" radius="lg" withBorder>
      <Stack gap="lg">
        <Group align="end" justify="space-between">
          <Stack gap={2}>
            <Text fw={700}>{t('charts.distribution.title')}</Text>
            <Text c="dimmed" size="sm">
              {t('charts.distribution.description')}
            </Text>
          </Stack>
          <NativeSelect
            data={metricOptions.map((value) => ({
              label: chartMetricLabel(t, value),
              value,
            }))}
            label={t('charts.controls.metric')}
            onChange={(event) =>
              onMetricChange(event.currentTarget.value as UsageChartDistributionMetric)
            }
            value={metric}
          />
        </Group>
        <DistributionChart
          ariaLabel={t('charts.distribution.barAria')}
          emptyLabel={t('charts.distribution.empty')}
          formatAxisValue={formatAxisValue}
          formatValue={formatValue}
          rows={rows}
        />
      </Stack>
    </Paper>
  );
}
