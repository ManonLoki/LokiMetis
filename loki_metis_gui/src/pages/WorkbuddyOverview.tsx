import { Paper, SegmentedControl, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import { useAtom } from 'jotai';
import { useTranslation } from 'react-i18next';

import type { UsageWindow, WorkbuddyWindowDto } from '../api/usage';
import { TokenTotalDisplay } from '../components/UsageUi';
import { overviewWindowAtom } from '../state/page-session';
import { formatCredits, formatTokens } from '../usage-format';
import { overviewWindowOrder } from './overview-windows';
import { WorkbuddyQueryGate } from './WorkbuddyGate';
import {
  formatMinutes,
  selectWorkbuddyWindow,
  useWorkbuddyStatisticsQuery,
  workbuddyTraceErrorRate,
} from './WorkbuddyShared';

/** WorkBuddy 概览：与本机看板共用六个日历窗口，展示 project JSONL 精确用量与 Trace 诊断。 */
export function WorkbuddyOverview() {
  const { t } = useTranslation();
  const [selectedWindow, setSelectedWindow] = useAtom(overviewWindowAtom);
  const query = useWorkbuddyStatisticsQuery();
  if (!query.statisticsQuery.isSuccess) {
    return <WorkbuddyQueryGate {...query} />;
  }

  const selected = selectWorkbuddyWindow(query.statisticsQuery.data.windows, selectedWindow);
  const label = t(`window.${selectedWindow}`);
  return (
    <Stack className="page-stack" data-testid="workbuddy-overview" gap="xl">
      <section aria-labelledby="workbuddy-overview-heading">
        <Stack gap="md">
          <Title id="workbuddy-overview-heading" order={2}>
            {t('overview.local.title')}
          </Title>
          <SegmentedControl
            aria-label={t('overview.local.windowAria')}
            data={overviewWindowOrder.map((window) => ({
              label: t(`window.${window}`),
              value: window,
            }))}
            onChange={(value) => setSelectedWindow(value as UsageWindow)}
            value={selectedWindow}
          />
          <WorkbuddyWindowSummary label={label} window={selected} />
        </Stack>
      </section>
    </Stack>
  );
}

/** 渲染所选 WorkBuddy 窗口的请求用量与独立 Trace 诊断，不提供调用入口。 */
function WorkbuddyWindowSummary({
  label,
  window,
}: {
  label: string;
  window: WorkbuddyWindowDto | undefined;
}) {
  const { t } = useTranslation();
  const traceErrorRate = workbuddyTraceErrorRate(window);
  const breakdown = [
    { label: t('metric.input'), value: formatTokens(window?.inputTokens ?? 0) },
    { label: t('metric.cachedInput'), value: formatTokens(window?.cachedInputTokens ?? 0) },
    { label: t('metric.uncachedInput'), value: formatTokens(window?.uncachedInputTokens ?? 0) },
    { label: t('metric.output'), value: formatTokens(window?.outputTokens ?? 0) },
    {
      label: t('workbuddy.topLevelRequests'),
      value: formatTokens(window?.topLevelRequestCount ?? 0),
    },
    {
      label: t('workbuddy.subagentRequests'),
      value: formatTokens(window?.subagentRequestCount ?? 0),
    },
    {
      label: t('workbuddy.averageDuration'),
      value: t('workbuddy.minutesValue', {
        minutes: formatMinutes(window?.averageSessionDurationSeconds ?? 0),
      }),
    },
    { label: t('workbuddy.traceTotal'), value: formatTokens(window?.traceTotalCount ?? 0) },
    {
      label: t('workbuddy.traceErrorRate'),
      value:
        traceErrorRate === null
          ? t('common.notApplicable')
          : `${(traceErrorRate * 100).toFixed(1)}%`,
    },
    {
      label: t('workbuddy.traceCancelled'),
      value: formatTokens(window?.traceCancelledCount ?? 0),
    },
    {
      label: t('workbuddy.traceAverageDuration'),
      value: t('workbuddy.millisecondsValue', {
        ms: Math.round(window?.traceAverageDurationMs ?? 0),
      }),
    },
  ];
  return (
    <section aria-label={t('overviewCards.local.sectionAria', { window: label })}>
      <Stack gap="md">
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }}>
          <Paper className="metric-card local-card local-hero-card" p="lg" radius="lg" withBorder>
            <Text c="dimmed" fw={700} size="sm">
              {t('workbuddy.totalSessions')}
            </Text>
            <Text className="hero-number" fw={800}>
              {formatTokens(window?.sessionCount ?? 0)}
            </Text>
          </Paper>
          <Paper className="metric-card local-card local-hero-card" p="lg" radius="lg" withBorder>
            <Text c="dimmed" fw={700} size="sm">
              {t('workbuddy.totalRequests')}
            </Text>
            <Text className="hero-number" fw={800}>
              {formatTokens(window?.requestCount ?? 0)}
            </Text>
          </Paper>
          <Paper className="metric-card local-card local-hero-card" p="lg" radius="lg" withBorder>
            <Text c="dimmed" fw={700} size="sm">
              {t('overviewCards.local.tokenTotal')}
            </Text>
            <TokenTotalDisplay className="hero-number" value={window?.tokens ?? 0} />
          </Paper>
          <Paper className="metric-card local-card local-hero-card" p="lg" radius="lg" withBorder>
            <Text c="dimmed" fw={700} size="sm">
              {t('workbuddy.totalCredits')}
            </Text>
            <Text className="hero-number" fw={800}>
              {formatCredits(window?.credits ?? null)}
            </Text>
          </Paper>
        </SimpleGrid>

        <Paper className="breakdown-panel" p="lg" radius="lg" withBorder>
          <Stack gap="md">
            <Title order={3}>{t('workbuddy.breakdownTitle', { window: label })}</Title>
            <SimpleGrid cols={{ base: 2, sm: 3 }}>
              {breakdown.map((metric) => (
                <div className="mini-metric" key={metric.label}>
                  <Text c="dimmed" size="xs">
                    {metric.label}
                  </Text>
                  <Text fw={800}>{metric.value}</Text>
                </div>
              ))}
            </SimpleGrid>
          </Stack>
        </Paper>
      </Stack>
    </section>
  );
}
