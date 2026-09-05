import { Stack } from '@mantine/core';
import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useAtom, useAtomValue } from 'jotai';
import { useTranslation } from 'react-i18next';

import {
  getPrivacySettings,
  getWorkbuddyUsageStatistics,
  timeStandardQueryKey,
  type UsageDimension,
} from '../api/usage';
import { FailureState, LoadingState } from '../components/UsageUi';
import { timeStandardAtom, usageDimensionAtom, usageWindowAtom } from '../state/page-session';
import { UsageStatistics } from './UsageStatistics';
import { WorkbuddyLockedState } from './WorkbuddyGate';
import { WorkbuddyModelUsageTable } from './WorkbuddyModelUsageTable';
import { WORKBUDDY_REFRESH_INTERVAL_MS } from './WorkbuddyShared';

/** WorkBuddy 看板用量：通用请求统计与同源 project JSONL 实际模型分项。 */
export function WorkbuddyUsage() {
  const { t } = useTranslation();
  const [statisticsWindow, setStatisticsWindow] = useAtom(usageWindowAtom);
  const [selectedDimension, setSelectedDimension] = useAtom(usageDimensionAtom);
  const timeStandard = useAtomValue(timeStandardAtom);
  const privacyQuery = useQuery({
    queryFn: () => getPrivacySettings('codex'),
    queryKey: ['privacy-settings', 'codex'],
  });
  const enabled = privacyQuery.data?.workbuddyStatsEnabled === true;
  const effectiveDimension: UsageDimension =
    selectedDimension === 'reasoningEffort' ? 'model' : selectedDimension;
  const statisticsQuery = useQuery({
    enabled,
    placeholderData: keepPreviousData,
    queryFn: () => getWorkbuddyUsageStatistics(statisticsWindow, effectiveDimension, timeStandard),
    queryKey: [
      'workbuddy-usage-statistics',
      statisticsWindow,
      effectiveDimension,
      ...timeStandardQueryKey(timeStandard),
    ],
    refetchInterval: (query) =>
      query.state.fetchStatus === 'fetching' ? false : WORKBUDDY_REFRESH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });
  if (privacyQuery.isPending) {
    return <LoadingState label={t('workbuddy.loading')} />;
  }
  if (privacyQuery.isError) {
    return <FailureState error={privacyQuery.error} onRetry={() => void privacyQuery.refetch()} />;
  }
  if (!enabled) {
    return <WorkbuddyLockedState />;
  }
  if (statisticsQuery.isPending) {
    return <LoadingState label={t('statistics.page.loading')} />;
  }
  if (statisticsQuery.isError) {
    return (
      <FailureState error={statisticsQuery.error} onRetry={() => void statisticsQuery.refetch()} />
    );
  }

  return (
    <Stack className="page-stack" data-testid="workbuddy-page" gap="xl">
      <UsageStatistics
        dimension={effectiveDimension}
        fetching={statisticsQuery.isFetching}
        onDimensionChange={setSelectedDimension}
        onWindowChange={setStatisticsWindow}
        reasoningAvailable={false}
        statistics={statisticsQuery.data.statistics}
        timeStandard={timeStandard}
        window={statisticsWindow}
      />
      <WorkbuddyModelUsageTable modelUsage={statisticsQuery.data.modelUsage} />
    </Stack>
  );
}
