import { Alert, Stack } from '@mantine/core';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useAtomValue } from 'jotai';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import {
  cancelRootDiscovery,
  getRootDiscoveryStatus,
  getLocalScanStatus,
  getSources,
  getUsageOverview,
  listRootCandidates,
  manualAddSourceRoot,
  refreshLocalIndexes,
  reindexSourceRoot,
  removeSourceRoot,
  renameSourceRoot,
  setSourceRootEnabled,
  setPrimarySourceRoot,
  startRootDiscovery,
  type RootCandidateDto,
  type RootDiscoveryScope,
  type AgentClientKind,
  type UiMessageCode,
} from '../api/usage';
import { SCAN_STATUS_POLL_INTERVAL_MS, invalidateLocalUsageQueries } from '../api/usage-queries';
import { FailureState, ImplementationState, LoadingState } from '../components/UsageUi';
import { agentClientAtom, agentClientLabel, usageViewAtom } from '../state/agent-client';
import { timeStandardAtom } from '../state/page-session';
import { visibleErrorMessage } from '../visible-error';
import { uiMessageLabel } from '../i18n/backend-labels';
import { SourceDiscoveryPanel } from './SourceDiscoveryPanel';
import { SourceRootTable } from './SourceRootTable';
import { SourceRootDialogs } from './SourceRootDialogs';
import { SourcePrimaryRootControls } from './SourcePrimaryRootControls';
import { EmptySourcesPanel } from './EmptySourcesPanel';
import {
  clearRootCandidateCache,
  ROOT_CANDIDATES_QUERY_KEY,
  useRootCandidateEvents,
} from './useRootCandidateEvents';
import { selectVisibleDiscoveryCandidates } from './visible-discovery-candidates';
import { useDiscoveryBatchIndex } from './useDiscoveryBatchIndex';
import { WorkbuddySources } from './WorkbuddySources';

/** 展示当前只读视图的数据源：WorkBuddy 走 project JSONL 独立只读探测，其余三个本机客户端
 * 走既有数据根登记、扫描与发现流程。 */
export function SourcesPage() {
  const client = useAtomValue(agentClientAtom);
  const view = useAtomValue(usageViewAtom);
  if (view === 'workbuddy') {
    return <WorkbuddySources />;
  }
  return <LocalSourcesPage client={client} />;
}

/** 为三个本机客户端装配数据源查询。 */
function LocalSourcesPage({ client }: { client: AgentClientKind }) {
  const { t } = useTranslation();
  const clientLabel = agentClientLabel(client);
  const timeStandard = useAtomValue(timeStandardAtom);
  const queryClient = useQueryClient();
  const [rootMutationMessageCode, setRootMutationMessageCode] = useState<UiMessageCode | null>(
    null,
  );
  const [renameTarget, setRenameTarget] = useState<{ alias: string; id: string } | null>(null);
  const [renameDraft, setRenameDraft] = useState('');
  const [removeTarget, setRemoveTarget] = useState<{ alias: string; id: string } | null>(null);
  const invalidatedTerminal = useRef<string | null>(null);
  const overviewQuery = useQuery({
    queryFn: () => getUsageOverview(client, timeStandard),
    queryKey: ['usage-overview', client, timeStandard.mode, timeStandard.customTimeZone],
  });
  const businessReady = overviewQuery.isSuccess && !overviewQuery.data.productDefinitionRequired;
  useRootCandidateEvents(businessReady);
  const sourcesQuery = useQuery({
    enabled: businessReady,
    queryFn: () => getSources(client),
    queryKey: ['usage-sources', client],
  });
  const scanQuery = useQuery({
    enabled: businessReady,
    queryFn: () => getLocalScanStatus(client),
    queryKey: ['scan-status', client],
    refetchInterval: SCAN_STATUS_POLL_INTERVAL_MS,
  });
  const discoveryQuery = useQuery({
    enabled: businessReady,
    queryFn: getRootDiscoveryStatus,
    queryKey: ['root-discovery-status'],
    refetchInterval: (query) =>
      query.state.data?.state === 'running' ? SCAN_STATUS_POLL_INTERVAL_MS : false,
  });
  const candidatesQuery = useQuery({
    enabled: businessReady,
    queryFn: listRootCandidates,
    queryKey: ROOT_CANDIDATES_QUERY_KEY,
    refetchInterval: () =>
      discoveryQuery.data?.state === 'running' ? SCAN_STATUS_POLL_INTERVAL_MS : false,
  });
  const discoveryBatch = useDiscoveryBatchIndex({
    businessReady,
    candidates: candidatesQuery.data,
    client,
    discovery: discoveryQuery.data,
  });
  const discoveryMutation = useMutation({
    mutationFn: ({ scope }: { scope: RootDiscoveryScope; targetClient: AgentClientKind }) =>
      startRootDiscovery(scope),
    onMutate: ({ targetClient }) => {
      discoveryBatch.begin(targetClient);
      clearRootCandidateCache(queryClient);
    },
    onSuccess: async (status) => {
      queryClient.setQueryData(['root-discovery-status'], status);
      await queryClient.invalidateQueries({ queryKey: ROOT_CANDIDATES_QUERY_KEY });
    },
    onError: discoveryBatch.abort,
  });
  const cancelDiscoveryMutation = useMutation({
    mutationFn: cancelRootDiscovery,
    onSuccess: (status) => queryClient.setQueryData(['root-discovery-status'], status),
  });
  const directRefreshMutation = useMutation({
    mutationFn: (targetClient: AgentClientKind) =>
      refreshLocalIndexes([targetClient], 'directManual'),
    onSuccess: async (_statuses, targetClient) => {
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const manualAddMutation = useMutation({
    mutationFn: (targetClient: typeof client) => manualAddSourceRoot(targetClient),
    onSuccess: async (result, targetClient) => {
      setRootMutationMessageCode(result.messageCode);
      if (result.outcome === 'registered' || result.outcome === 'alreadyRegistered') {
        await directRefreshMutation.mutateAsync(targetClient);
      }
      if (result.outcome === 'deepSearchStarted') {
        discoveryBatch.begin(targetClient);
        clearRootCandidateCache(queryClient);
        if (result.discovery) {
          queryClient.setQueryData(['root-discovery-status'], result.discovery);
        }
        await queryClient.invalidateQueries({ queryKey: ROOT_CANDIDATES_QUERY_KEY });
      }
    },
  });
  const addCandidate = async (candidate: RootCandidateDto) => {
    await discoveryBatch.retryCandidate(candidate);
    if (!discoveryBatch.isActive) {
      await directRefreshMutation.mutateAsync(candidate.client);
    }
  };
  const rootEnabledMutation = useMutation({
    mutationFn: ({
      enabled,
      rootId,
      targetClient,
    }: {
      enabled: boolean;
      rootId: string;
      targetClient: typeof client;
    }) => setSourceRootEnabled(targetClient, rootId, enabled),
    onSuccess: async (result, { targetClient }) => {
      setRootMutationMessageCode(result.messageCode ?? null);
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const renameRootMutation = useMutation({
    mutationFn: ({
      alias,
      rootId,
      targetClient,
    }: {
      alias: string;
      rootId: string;
      targetClient: typeof client;
    }) => renameSourceRoot(targetClient, rootId, alias),
    onSuccess: async (result, { targetClient }) => {
      setRootMutationMessageCode(result.messageCode ?? null);
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const removeRootMutation = useMutation({
    mutationFn: ({ rootId, targetClient }: { rootId: string; targetClient: typeof client }) =>
      removeSourceRoot(targetClient, rootId),
    onSuccess: async (result, { targetClient }) => {
      setRootMutationMessageCode(result.messageCode ?? null);
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const reindexRootMutation = useMutation({
    mutationFn: ({ rootId, targetClient }: { rootId: string; targetClient: typeof client }) =>
      reindexSourceRoot(targetClient, rootId),
    onSuccess: (status, { targetClient }) => {
      queryClient.setQueryData(['scan-status', targetClient], status);
    },
  });
  const primaryRootMutation = useMutation({
    mutationFn: ({
      rootId,
      targetClient,
    }: {
      rootId: string | null;
      targetClient: typeof client;
    }) => setPrimarySourceRoot(targetClient, rootId),
    onSuccess: async (result, { targetClient }) => {
      setRootMutationMessageCode(result.messageCode ?? null);
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const rootMutationPending =
    rootEnabledMutation.isPending ||
    renameRootMutation.isPending ||
    removeRootMutation.isPending ||
    reindexRootMutation.isPending ||
    primaryRootMutation.isPending ||
    directRefreshMutation.isPending ||
    discoveryBatch.isRefreshing;

  useEffect(() => {
    const scan = scanQuery.data;
    if (!scan || scan.state === 'idle' || scan.state === 'running' || !scan.finishedAtEpochMs) {
      if (scan?.state === 'running') invalidatedTerminal.current = null;
      return;
    }
    const identity = `${scan.scanId}:${scan.finishedAtEpochMs}:${scan.state}`;
    if (invalidatedTerminal.current === identity) return;
    invalidatedTerminal.current = identity;
    void invalidateLocalUsageQueries(queryClient, client);
  }, [client, queryClient, scanQuery.data]);

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
  if (
    sourcesQuery.isPending ||
    scanQuery.isPending ||
    discoveryQuery.isPending ||
    candidatesQuery.isPending
  ) {
    return <LoadingState label={t('sources.page.loading')} />;
  }
  if (sourcesQuery.isError) {
    return <FailureState error={sourcesQuery.error} onRetry={() => void sourcesQuery.refetch()} />;
  }
  if (scanQuery.isError) {
    return <FailureState error={scanQuery.error} onRetry={() => void scanQuery.refetch()} />;
  }
  if (discoveryQuery.isError || candidatesQuery.isError) {
    return (
      <FailureState
        error={discoveryQuery.error || candidatesQuery.error}
        onRetry={() => void discoveryQuery.refetch()}
      />
    );
  }
  const { roots } = sourcesQuery.data;
  const scan = scanQuery.data;
  const discovery = discoveryQuery.data;
  const candidates = candidatesQuery.data;
  const scanStartBlocked = scan.state === 'running' || rootMutationPending;

  return (
    <Stack className="page-stack" gap="xl">
      {rootMutationMessageCode ? (
        <Alert aria-live="polite" color="green" title={t('sources.page.updatedTitle')}>
          {uiMessageLabel(t, rootMutationMessageCode)}
        </Alert>
      ) : null}
      {rootEnabledMutation.isError ||
      renameRootMutation.isError ||
      removeRootMutation.isError ||
      reindexRootMutation.isError ||
      primaryRootMutation.isError ||
      directRefreshMutation.isError ||
      discoveryBatch.error ? (
        <Alert color="red" title={t('sources.page.updateErrorTitle')}>
          {visibleErrorMessage(
            rootEnabledMutation.error ||
              renameRootMutation.error ||
              removeRootMutation.error ||
              reindexRootMutation.error ||
              primaryRootMutation.error ||
              directRefreshMutation.error ||
              discoveryBatch.error,
            t('sources.page.updateErrorBody', { client: clientLabel }),
          )}
        </Alert>
      ) : null}
      {scan.state === 'running' ? (
        <Alert color="blue" title={t('sources.background.runningTitle')}>
          {t('sources.background.runningBody', { client: clientLabel })}
        </Alert>
      ) : null}
      {scan.state === 'failed' ? (
        <Alert color="red" title={t('sources.background.failedTitle')}>
          {t('sources.background.failedBody', { client: clientLabel })}
        </Alert>
      ) : null}

      {roots.length === 0 && scan.state === 'idle' ? (
        <EmptySourcesPanel clientLabel={clientLabel} />
      ) : null}

      <SourceRootTable
        currentRootId={
          scan.state === 'running' && scan.currentScopeCode === 'indexingRoots'
            ? (scan.scopeProgress?.currentRootId ?? null)
            : null
        }
        onRemove={(rootId, currentAlias) => setRemoveTarget({ alias: currentAlias, id: rootId })}
        onReindex={(rootId) => reindexRootMutation.mutate({ rootId, targetClient: client })}
        onRename={(rootId, currentAlias) => {
          setRenameTarget({ alias: currentAlias, id: rootId });
          setRenameDraft(currentAlias);
        }}
        onToggleEnabled={(rootId, nextEnabled) =>
          rootEnabledMutation.mutate({ enabled: nextEnabled, rootId, targetClient: client })
        }
        removePending={rootMutationPending}
        reindexPendingRootId={
          reindexRootMutation.isPending ? (reindexRootMutation.variables?.rootId ?? null) : null
        }
        renamePending={rootMutationPending}
        roots={roots}
        scanRunning={scanStartBlocked}
        showPrimary={client === 'codex'}
        togglePending={rootMutationPending}
      />

      {client === 'codex' ? (
        <SourcePrimaryRootControls
          disabled={scanStartBlocked}
          onChange={(rootId) => primaryRootMutation.mutate({ rootId, targetClient: client })}
          pending={primaryRootMutation.isPending}
          roots={roots}
        />
      ) : null}

      <SourceDiscoveryPanel
        cancelPending={cancelDiscoveryMutation.isPending}
        candidates={selectVisibleDiscoveryCandidates(candidates, [client]).filter((candidate) =>
          discoveryBatch.failedCandidateIds.has(candidate.id),
        )}
        discovery={discovery}
        discoveryPending={discoveryMutation.isPending}
        manualAddClients={[client]}
        manualAddPending={manualAddMutation.isPending}
        mutationBlocked={rootMutationPending || scanStartBlocked}
        onAdd={addCandidate}
        onCancel={() => cancelDiscoveryMutation.mutate()}
        onManualAdd={(targetClient) => manualAddMutation.mutate(targetClient)}
        onStart={(scope) => discoveryMutation.mutate({ scope, targetClient: client })}
      />

      <SourceRootDialogs
        clientLabel={clientLabel}
        onCloseRemove={() => setRemoveTarget(null)}
        onCloseRename={() => setRenameTarget(null)}
        onConfirmRemove={(rootId) => {
          removeRootMutation.mutate({ rootId, targetClient: client });
          setRemoveTarget(null);
        }}
        onConfirmRename={(rootId, alias) => {
          renameRootMutation.mutate({ alias, rootId, targetClient: client });
          setRenameTarget(null);
        }}
        onRenameDraftChange={setRenameDraft}
        removePending={removeRootMutation.isPending}
        removeTarget={removeTarget}
        renameDraft={renameDraft}
        renamePending={renameRootMutation.isPending}
        renameTarget={renameTarget}
      />
    </Stack>
  );
}
