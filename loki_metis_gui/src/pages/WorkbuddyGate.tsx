import { Button, Paper, Stack, Text, Title } from '@mantine/core';
import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

import { FailureState, LoadingState } from '../components/UsageUi';
import type { useWorkbuddyStatisticsQuery } from './WorkbuddyShared';

/** 开关、隐私或统计查询尚未完成时的共用加载与失败态。 */
export function WorkbuddyQueryGate({
  enabled,
  privacyQuery,
  statisticsQuery,
}: ReturnType<typeof useWorkbuddyStatisticsQuery>) {
  const { t } = useTranslation();
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
    return <LoadingState label={t('workbuddy.loading')} />;
  }
  if (statisticsQuery.isError) {
    return (
      <FailureState error={statisticsQuery.error} onRetry={() => void statisticsQuery.refetch()} />
    );
  }
  return null;
}

/** 开关关闭时引导前往设置，不读取 WorkBuddy project JSONL 或 Trace。 */
export function WorkbuddyLockedState() {
  const { t } = useTranslation();
  return (
    <Paper className="page-stack" data-testid="workbuddy-locked" p="xl" radius="lg" withBorder>
      <Stack align="flex-start" gap="md">
        <Title order={2}>{t('workbuddy.lockedTitle')}</Title>
        <Text c="dimmed">{t('workbuddy.lockedDescription')}</Text>
        <Button component={Link} to="/privacy">
          {t('workbuddy.configureAction')}
        </Button>
      </Stack>
    </Paper>
  );
}
