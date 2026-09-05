import { Alert, Stack } from '@mantine/core';
import { useQuery } from '@tanstack/react-query';
import { useAtom, useAtomValue } from 'jotai';
import { useTranslation } from 'react-i18next';

import { getUsageOverview, timeStandardQueryKey } from '../api/usage';
import { FailureState, ImplementationState, LoadingState } from '../components/UsageUi';
import { usageViewAtom } from '../state/agent-client';
import { overviewWindowAtom, timeStandardAtom } from '../state/page-session';
import { uiMessageLabel } from '../i18n/backend-labels';
import { OverviewLocalSection } from './OverviewLocalSection';
import { WorkbuddyOverview } from './WorkbuddyOverview';
import { selectOverviewWindows } from './overview-windows';

/** 概览每十秒读取本机索引。 */
const OVERVIEW_REFRESH_INTERVAL_MS = 10_000;

/** 实现只展示本机记录的概览页。 */
export function OverviewPage() {
  const view = useAtomValue(usageViewAtom);
  if (view === 'workbuddy') {
    return <WorkbuddyOverview />;
  }
  return <LocalOverviewPage />;
}

/** 物理 Agent 与全部视图的本机概览查询。 */
function LocalOverviewPage() {
  const { i18n, t } = useTranslation();
  const view = useAtomValue(usageViewAtom);
  const [selectedWindow, setSelectedWindow] = useAtom(overviewWindowAtom);
  const timeStandard = useAtomValue(timeStandardAtom);
  const overviewQuery = useQuery({
    queryFn: () => getUsageOverview(view, timeStandard),
    queryKey: ['usage-overview', view, ...timeStandardQueryKey(timeStandard)],
    refetchInterval: (query) =>
      query.state.fetchStatus === 'fetching' ? false : OVERVIEW_REFRESH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });

  if (overviewQuery.isPending) {
    return <LoadingState />;
  }
  if (overviewQuery.isError) {
    return (
      <FailureState error={overviewQuery.error} onRetry={() => void overviewQuery.refetch()} />
    );
  }
  if (overviewQuery.data.productDefinitionRequired) {
    return (
      <ImplementationState
        message={overviewQuery.data.implementationMessage}
        messageCode={overviewQuery.data.implementationMessageCode}
      />
    );
  }

  const local = overviewQuery.data.localRecords;
  if (!local) {
    return (
      <FailureState
        error={new Error(t('overview.errors.missingSections'))}
        fallback={t('overview.errors.missingSections')}
        onRetry={() => void overviewQuery.refetch()}
      />
    );
  }

  const visibleWindows = selectOverviewWindows(local.windows);
  if (!visibleWindows) {
    return (
      <FailureState
        error={new Error(t('overview.errors.incompleteWindows'))}
        fallback={t('overview.errors.incompleteWindows')}
        onRetry={() => void overviewQuery.refetch()}
      />
    );
  }
  const selectedUsage =
    visibleWindows.find((windowUsage) => windowUsage.window === selectedWindow) ??
    visibleWindows[0]!;
  const implementationMessage = overviewQuery.data.implementationMessageCode
    ? uiMessageLabel(t, overviewQuery.data.implementationMessageCode)
    : i18n.resolvedLanguage === 'zh-CN'
      ? overviewQuery.data.implementationMessage
      : t('common.unknownError');

  return (
    <Stack className="page-stack" gap="xl">
      {overviewQuery.data.implementationMessage || overviewQuery.data.implementationMessageCode ? (
        <Alert color="orange" title={t('overview.partialTitle')}>
          {implementationMessage}
        </Alert>
      ) : null}

      <OverviewLocalSection
        local={local}
        selectedUsage={selectedUsage}
        selectedWindow={selectedWindow}
        onWindowChange={setSelectedWindow}
      />
    </Stack>
  );
}
