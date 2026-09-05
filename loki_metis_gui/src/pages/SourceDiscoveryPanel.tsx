import { Alert, Badge, Button, Group, Paper, Stack, Text, Title } from '@mantine/core';
import { useTranslation } from 'react-i18next';

import type {
  AgentClientKind,
  RootCandidateDto,
  RootDiscoveryScope,
  RootDiscoveryStatusDto,
} from '../api/usage';
import { agentClientLabel } from '../state/agent-client';
import { RootCandidateCapsules } from './RootCandidateCapsules';
import {
  selectVisibleDiscoveryCandidates,
  visibleDiscoveryCandidateCount,
} from './visible-discovery-candidates';

/** 定义数据源页快速/完全发现的状态和显式操作。 */
interface SourceDiscoveryPanelProps {
  candidates: RootCandidateDto[];
  discovery: RootDiscoveryStatusDto;
  discoveryPending: boolean;
  mutationBlocked: boolean;
  cancelPending: boolean;
  manualAddClients: AgentClientKind[];
  manualAddPending: boolean;
  onCancel: () => void;
  onAdd: (candidate: RootCandidateDto) => Promise<void>;
  onStart: (scope: RootDiscoveryScope) => void;
  onManualAdd: (client: AgentClientKind) => void;
}

/** 展示分平台数据源发现范围、无百分比计数与实时候选。 */
export function SourceDiscoveryPanel({
  candidates,
  discovery,
  discoveryPending,
  mutationBlocked,
  cancelPending,
  manualAddClients,
  manualAddPending,
  onCancel,
  onAdd,
  onStart,
  onManualAdd,
}: SourceDiscoveryPanelProps) {
  const { t } = useTranslation();
  const visibleCandidates = selectVisibleDiscoveryCandidates(candidates, manualAddClients);
  const visibleCount = visibleDiscoveryCandidateCount(candidates, manualAddClients);
  const controlsDisabled = discovery.state === 'running' || mutationBlocked || manualAddPending;
  const showEmptyManualDeepSearch =
    discovery.scope === 'manualSubtree' &&
    discovery.state === 'complete' &&
    discovery.candidatesFound === 0 &&
    visibleCount === 0;
  return (
    <Paper className="scan-card" p="lg" radius="lg" withBorder>
      <Stack gap="md">
        <Group align="flex-start" justify="space-between">
          <div>
            <Group gap="xs">
              <Title order={3}>{t('sources.discovery.title')}</Title>
              <Badge color={discovery.state === 'partial' ? 'orange' : 'gray'} variant="light">
                {t(`sources.discovery.state.${discovery.state}`)}
              </Badge>
            </Group>
            <Text c="dimmed" size="xs">
              {t(`sources.discovery.scope.${discovery.scope}`)}
            </Text>
          </div>
          <Group>
            <Button
              disabled={controlsDisabled}
              loading={discoveryPending}
              onClick={() => onStart('userPriority')}
            >
              {t('sources.discovery.startUser')}
            </Button>
            <Button
              disabled={controlsDisabled}
              loading={discoveryPending}
              onClick={() => onStart('fullLocalVolumes')}
              variant="light"
            >
              {t('sources.discovery.startFull')}
            </Button>
            {manualAddClients.map((client) => (
              <Button
                key={client}
                disabled={controlsDisabled}
                loading={manualAddPending}
                onClick={() => onManualAdd(client)}
                variant="default"
              >
                {manualAddClients.length === 1
                  ? t('sources.page.addRoot')
                  : t('sources.discovery.addManualClient', {
                      client: agentClientLabel(client),
                    })}
              </Button>
            ))}
            <Button
              color="red"
              disabled={discovery.state !== 'running'}
              loading={cancelPending}
              onClick={onCancel}
              variant="light"
            >
              {t('sources.discovery.cancel')}
            </Button>
          </Group>
        </Group>
        <Group gap="xl">
          <Text size="sm">
            {t('sources.discovery.volumes', {
              completed: discovery.volumesCompleted,
              total: discovery.volumesTotal,
            })}
          </Text>
          <Text size="sm">
            {t('sources.discovery.directories', { count: discovery.directoriesChecked })}
          </Text>
          <Text size="sm">
            {t('sources.discovery.fileNames', { count: discovery.fileNamesChecked })}
          </Text>
          <Text size="sm">{t('sources.discovery.candidates', { count: visibleCount })}</Text>
        </Group>
        {showEmptyManualDeepSearch ? (
          <Alert color="orange">{t('backend.message.sourceManualDeepSearchEmpty')}</Alert>
        ) : null}
        {discovery.state === 'failed' ? (
          <Alert color="red" title={t('sources.discovery.failedTitle')}>
            {t('sources.discovery.failedBody', {
              code: discovery.errorCode ?? 'unknown',
            })}
          </Alert>
        ) : null}
        {visibleCandidates.length > 0 ? (
          <RootCandidateCapsules candidates={visibleCandidates} onAdd={onAdd} />
        ) : null}
      </Stack>
    </Paper>
  );
}
