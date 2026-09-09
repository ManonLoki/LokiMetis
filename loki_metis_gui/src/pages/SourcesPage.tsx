import { Alert, Button, Stack, Text } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAtomValue } from "jotai";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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
} from "../api/usage";
import {
  SCAN_STATUS_POLL_INTERVAL_MS,
  invalidateLocalUsageQueries,
} from "../api/usage-queries";
import { FailureState, ImplementationState, LoadingState } from "../components/UsageUi";
import { agentClientAtom, agentClientLabel, usageViewAtom } from "../state/agent-client";
import { timeStandardAtom } from "../state/page-session";
import { visibleErrorMessage } from "../visible-error";
import { uiMessageLabel } from "../i18n/backend-labels";
import { SourceDiscoveryPanel } from "./SourceDiscoveryPanel";
import { SourceRootTable } from "./SourceRootTable";
import { SourceRootDialogs } from "./SourceRootDialogs";
import { SourcePrimaryRootControls } from "./SourcePrimaryRootControls";
import { EmptySourcesPanel } from "./EmptySourcesPanel";
import {
  clearRootCandidateCache,
  ROOT_CANDIDATES_QUERY_KEY,
  useRootCandidateEvents,
} from "./useRootCandidateEvents";
import { selectVisibleDiscoveryCandidates } from "./visible-discovery-candidates";
import { useDiscoveryBatchIndex } from "./useDiscoveryBatchIndex";
import { WorkbuddySources } from "./WorkbuddySources";

/** 展示当前只读视图的数据源：WorkBuddy 走 project JSONL 独立只读探测，其余三个本机客户端
 * 走既有数据根登记、扫描与发现流程。 */
export function SourcesPage() {
  const client = useAtomValue(agentClientAtom);
  const view = useAtomValue(usageViewAtom);
  if (view === "workbuddy") {
    return <WorkbuddySources />;
  }
  return <LocalSourcesPage client={client} key={client} />;
}

/** 把一次链式索引写入绑定到来源页开始操作时的权威查询代次。 */
interface SourceAuthorityToken {
  client: AgentClientKind;
  epoch: number;
}

/** 为三个本机客户端装配数据源查询。 */
function LocalSourcesPage({ client }: { client: AgentClientKind }) {
  const { t } = useTranslation();
  const clientLabel = agentClientLabel(client);
  const timeStandard = useAtomValue(timeStandardAtom);
  const queryClient = useQueryClient();
  const [rootMutationMessageCode, setRootMutationMessageCode] =
    useState<UiMessageCode | null>(null);
  const [sourceActionFailure, setSourceActionFailure] = useState<unknown>(null);
  const [renameTarget, setRenameTarget] = useState<{
    alias: string;
    id: string;
    targetClient: AgentClientKind;
  } | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [removeTarget, setRemoveTarget] = useState<{
    alias: string;
    id: string;
    targetClient: AgentClientKind;
  } | null>(null);
  const invalidatedTerminal = useRef<string | null>(null);
  const overviewQuery = useQuery({
    queryFn: () => getUsageOverview(client, timeStandard),
    queryKey: ["usage-overview", client, timeStandard.mode, timeStandard.customTimeZone],
  });
  // 后台重读失败时 TanStack Query 会保留最近可信 data；下游查询继续展示该快照，
  // 但页面会冻结写操作，直到全部权威读取恢复。
  const businessReady =
    overviewQuery.data !== undefined && !overviewQuery.data.productDefinitionRequired;
  useRootCandidateEvents(businessReady);
  const sourcesQuery = useQuery({
    enabled: businessReady,
    queryFn: () => getSources(client),
    queryKey: ["usage-sources", client],
  });
  const scanQuery = useQuery({
    enabled: businessReady,
    queryFn: () => getLocalScanStatus(client),
    queryKey: ["scan-status", client],
    refetchInterval: SCAN_STATUS_POLL_INTERVAL_MS,
  });
  const discoveryQuery = useQuery({
    enabled: businessReady,
    queryFn: getRootDiscoveryStatus,
    queryKey: ["root-discovery-status"],
    refetchInterval: (query) =>
      query.state.data?.state === "running" ? SCAN_STATUS_POLL_INTERVAL_MS : false,
  });
  const candidatesQuery = useQuery({
    enabled: businessReady,
    queryFn: listRootCandidates,
    queryKey: ROOT_CANDIDATES_QUERY_KEY,
    refetchInterval: () =>
      discoveryQuery.data?.state === "running" ? SCAN_STATUS_POLL_INTERVAL_MS : false,
  });
  const queryRefreshError =
    (overviewQuery.data !== undefined ? overviewQuery.error : null) ??
    (sourcesQuery.data !== undefined ? sourcesQuery.error : null) ??
    (scanQuery.data !== undefined ? scanQuery.error : null) ??
    (discoveryQuery.data !== undefined ? discoveryQuery.error : null) ??
    (candidatesQuery.data !== undefined ? candidatesQuery.error : null);
  const discoveryBatchWritesReady =
    businessReady &&
    overviewQuery.error === null &&
    sourcesQuery.data !== undefined &&
    sourcesQuery.error === null &&
    scanQuery.data !== undefined &&
    scanQuery.error === null &&
    discoveryQuery.data !== undefined &&
    discoveryQuery.error === null &&
    candidatesQuery.data !== undefined &&
    candidatesQuery.error === null;
  const sourceAuthorityRef = useRef({
    client,
    epoch: 0,
    ready: discoveryBatchWritesReady,
  });
  if (
    sourceAuthorityRef.current.client !== client ||
    sourceAuthorityRef.current.ready !== discoveryBatchWritesReady
  ) {
    sourceAuthorityRef.current = {
      client,
      epoch: sourceAuthorityRef.current.epoch + 1,
      ready: discoveryBatchWritesReady,
    };
  }
  const captureSourceAuthority = (
    targetClient: AgentClientKind,
  ): SourceAuthorityToken | null => {
    const authority = sourceAuthorityRef.current;
    return authority.ready && authority.client === targetClient
      ? { client: targetClient, epoch: authority.epoch }
      : null;
  };
  const sourceAuthorityMatches = (token: SourceAuthorityToken): boolean => {
    const authority = sourceAuthorityRef.current;
    return (
      authority.ready &&
      authority.client === token.client &&
      authority.epoch === token.epoch
    );
  };
  const discoveryBatch = useDiscoveryBatchIndex({
    businessReady: discoveryBatchWritesReady,
    candidates: candidatesQuery.data,
    client,
    discovery: discoveryQuery.data,
  });
  /** 每次新动作先清除另一类旧失败，页面只展示最近一次真实动作的结果。 */
  const beginSourceAction = () => {
    setSourceActionFailure(null);
    setRootMutationMessageCode(null);
    discoveryBatch.resetError();
  };
  const discoveryMutation = useMutation({
    mutationFn: ({ scope }: { scope: RootDiscoveryScope; targetClient: AgentClientKind }) =>
      startRootDiscovery(scope),
    onMutate: ({ targetClient }) => {
      beginSourceAction();
      discoveryBatch.begin(targetClient);
      clearRootCandidateCache(queryClient);
    },
    onSuccess: async (status) => {
      queryClient.setQueryData(["root-discovery-status"], status);
      await queryClient.invalidateQueries({ queryKey: ROOT_CANDIDATES_QUERY_KEY });
    },
    onError: (cause) => {
      setSourceActionFailure(cause);
      discoveryBatch.abort();
    },
  });
  const cancelDiscoveryMutation = useMutation({
    mutationFn: cancelRootDiscovery,
    onError: setSourceActionFailure,
    onMutate: beginSourceAction,
    onSuccess: (status) => queryClient.setQueryData(["root-discovery-status"], status),
  });
  const directRefreshMutation = useMutation({
    mutationFn: ({
      authority,
      targetClient,
    }: {
      authority: SourceAuthorityToken;
      targetClient: AgentClientKind;
    }) => {
      if (!sourceAuthorityMatches(authority)) {
        throw new Error("authoritative-query-unavailable");
      }
      return refreshLocalIndexes([targetClient], "directManual");
    },
    onError: setSourceActionFailure,
    onSuccess: async (_statuses, { targetClient }) => {
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const manualAddMutation = useMutation({
    mutationFn: ({
      authority,
      targetClient,
    }: {
      authority: SourceAuthorityToken;
      targetClient: typeof client;
    }) => {
      if (!sourceAuthorityMatches(authority)) {
        throw new Error("authoritative-query-unavailable");
      }
      return manualAddSourceRoot(targetClient);
    },
    onError: setSourceActionFailure,
    onMutate: beginSourceAction,
    onSuccess: async (result, { authority, targetClient }) => {
      setRootMutationMessageCode(result.messageCode);
      if (
        (result.outcome === "registered" || result.outcome === "alreadyRegistered") &&
        sourceAuthorityMatches(authority)
      ) {
        await directRefreshMutation.mutateAsync({ authority, targetClient });
      }
      if (result.outcome === "deepSearchStarted") {
        discoveryBatch.begin(targetClient);
        clearRootCandidateCache(queryClient);
        if (result.discovery) {
          queryClient.setQueryData(["root-discovery-status"], result.discovery);
        }
        await queryClient.invalidateQueries({ queryKey: ROOT_CANDIDATES_QUERY_KEY });
      }
    },
  });
  const addCandidate = async (candidate: RootCandidateDto) => {
    const authority = captureSourceAuthority(candidate.client);
    if (!authority) throw new Error("authoritative-query-unavailable");
    beginSourceAction();
    await discoveryBatch.retryCandidate(candidate);
    if (!discoveryBatch.isActive && sourceAuthorityMatches(authority)) {
      await directRefreshMutation.mutateAsync({
        authority,
        targetClient: candidate.client,
      });
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
    onError: setSourceActionFailure,
    onMutate: beginSourceAction,
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
    onError: setSourceActionFailure,
    onMutate: beginSourceAction,
    onSuccess: async (result, { targetClient }) => {
      setRootMutationMessageCode(result.messageCode ?? null);
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const removeRootMutation = useMutation({
    mutationFn: ({
      rootId,
      targetClient,
    }: {
      rootId: string;
      targetClient: typeof client;
    }) => removeSourceRoot(targetClient, rootId),
    onError: setSourceActionFailure,
    onMutate: beginSourceAction,
    onSuccess: async (result, { targetClient }) => {
      setRootMutationMessageCode(result.messageCode ?? null);
      await invalidateLocalUsageQueries(queryClient, targetClient);
    },
  });
  const reindexRootMutation = useMutation({
    mutationFn: ({
      rootId,
      targetClient,
    }: {
      rootId: string;
      targetClient: typeof client;
    }) => reindexSourceRoot(targetClient, rootId),
    onError: setSourceActionFailure,
    onMutate: beginSourceAction,
    onSuccess: (status, { targetClient }) => {
      queryClient.setQueryData(["scan-status", targetClient], status);
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
    onError: setSourceActionFailure,
    onMutate: beginSourceAction,
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
  const sourceActionError = sourceActionFailure ?? discoveryBatch.error;
  const queryStateBlocked = queryRefreshError !== null;

  /** 新发现动作开始前清除同组旧错误，避免一次失败永久遮蔽后续恢复结果。 */
  const resetDiscoveryActionErrors = () => {
    beginSourceAction();
    discoveryMutation.reset();
    cancelDiscoveryMutation.reset();
    manualAddMutation.reset();
  };

  useEffect(() => {
    const scan = scanQuery.data;
    if (
      !scan ||
      scan.state === "idle" ||
      scan.state === "running" ||
      !scan.finishedAtEpochMs
    ) {
      if (scan?.state === "running") invalidatedTerminal.current = null;
      return;
    }
    const identity = `${scan.scanId}:${scan.finishedAtEpochMs}:${scan.state}`;
    if (invalidatedTerminal.current === identity) return;
    invalidatedTerminal.current = identity;
    void invalidateLocalUsageQueries(queryClient, client);
  }, [client, queryClient, scanQuery.data]);

  if (overviewQuery.data === undefined && overviewQuery.isPending) {
    return <LoadingState />;
  }
  if (overviewQuery.data === undefined) {
    return (
      <FailureState
        error={overviewQuery.error}
        onRetry={() => void overviewQuery.refetch()}
      />
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
    (sourcesQuery.data === undefined && sourcesQuery.isPending) ||
    (scanQuery.data === undefined && scanQuery.isPending) ||
    (discoveryQuery.data === undefined && discoveryQuery.isPending) ||
    (candidatesQuery.data === undefined && candidatesQuery.isPending)
  ) {
    return <LoadingState label={t("sources.page.loading")} />;
  }
  if (sourcesQuery.data === undefined) {
    return (
      <FailureState
        error={sourcesQuery.error}
        onRetry={() => void sourcesQuery.refetch()}
      />
    );
  }
  if (scanQuery.data === undefined) {
    return (
      <FailureState error={scanQuery.error} onRetry={() => void scanQuery.refetch()} />
    );
  }
  if (discoveryQuery.data === undefined || candidatesQuery.data === undefined) {
    return (
      <FailureState
        error={discoveryQuery.error || candidatesQuery.error}
        onRetry={() =>
          void Promise.all([discoveryQuery.refetch(), candidatesQuery.refetch()])
        }
      />
    );
  }
  const { roots } = sourcesQuery.data;
  const scan = scanQuery.data;
  const discovery = discoveryQuery.data;
  const candidates = candidatesQuery.data;
  const sourceWritesBlocked = rootMutationPending || queryStateBlocked;
  const scanStartBlocked = scan.state === "running" || sourceWritesBlocked;

  return (
    <Stack className="page-stack" gap="xl">
      {rootMutationMessageCode ? (
        <Alert aria-live="polite" color="green" title={t("sources.page.updatedTitle")}>
          {uiMessageLabel(t, rootMutationMessageCode)}
        </Alert>
      ) : null}
      {sourceActionError ? (
        <Alert color="red" title={t("sources.page.updateErrorTitle")}>
          {visibleErrorMessage(
            sourceActionError,
            t("sources.page.updateErrorBody", { client: clientLabel }),
          )}
        </Alert>
      ) : null}
      {queryRefreshError ? (
        <Alert color="red" role="alert" title={t("ui.failureTitle")}>
          <Stack align="flex-start" gap="sm">
            <Text size="sm">{visibleErrorMessage(queryRefreshError)}</Text>
            <Button
              onClick={() =>
                void Promise.all([
                  overviewQuery.refetch(),
                  sourcesQuery.refetch(),
                  scanQuery.refetch(),
                  discoveryQuery.refetch(),
                  candidatesQuery.refetch(),
                ])
              }
              size="xs"
              variant="light"
            >
              {t("common.retry")}
            </Button>
          </Stack>
        </Alert>
      ) : null}
      {scan.state === "running" ? (
        <Alert color="blue" title={t("sources.background.runningTitle")}>
          {t("sources.background.runningBody", { client: clientLabel })}
        </Alert>
      ) : null}
      {scan.state === "failed" ? (
        <Alert color="red" title={t("sources.background.failedTitle")}>
          {t("sources.background.failedBody", { client: clientLabel })}
        </Alert>
      ) : null}

      {roots.length === 0 && scan.state === "idle" ? (
        <EmptySourcesPanel clientLabel={clientLabel} />
      ) : null}

      <SourceRootTable
        currentRootId={
          scan.state === "running" && scan.currentScopeCode === "indexingRoots"
            ? (scan.scopeProgress?.currentRootId ?? null)
            : null
        }
        onRemove={(rootId, currentAlias) =>
          setRemoveTarget({ alias: currentAlias, id: rootId, targetClient: client })
        }
        onReindex={(rootId) => reindexRootMutation.mutate({ rootId, targetClient: client })}
        onRename={(rootId, currentAlias) => {
          setRenameTarget({ alias: currentAlias, id: rootId, targetClient: client });
          setRenameDraft(currentAlias);
        }}
        onToggleEnabled={(rootId, nextEnabled) =>
          rootEnabledMutation.mutate({ enabled: nextEnabled, rootId, targetClient: client })
        }
        removePending={rootMutationPending}
        reindexPendingRootId={
          reindexRootMutation.isPending
            ? (reindexRootMutation.variables?.rootId ?? null)
            : null
        }
        renamePending={rootMutationPending}
        roots={roots}
        scanRunning={scanStartBlocked}
        showPrimary={client === "codex"}
        togglePending={rootMutationPending}
      />

      {client === "codex" ? (
        <SourcePrimaryRootControls
          disabled={scanStartBlocked}
          onChange={(rootId) =>
            primaryRootMutation.mutate({ rootId, targetClient: client })
          }
          pending={primaryRootMutation.isPending}
          roots={roots}
        />
      ) : null}

      <SourceDiscoveryPanel
        cancelPending={cancelDiscoveryMutation.isPending}
        candidates={selectVisibleDiscoveryCandidates(candidates, [client]).filter(
          (candidate) => discoveryBatch.failedCandidateIds.has(candidate.id),
        )}
        discovery={discovery}
        discoveryPending={discoveryMutation.isPending}
        manualAddClients={[client]}
        manualAddPending={manualAddMutation.isPending}
        mutationBlocked={sourceWritesBlocked || scanStartBlocked}
        onAdd={addCandidate}
        onCancel={() => {
          resetDiscoveryActionErrors();
          cancelDiscoveryMutation.mutate();
        }}
        onManualAdd={(targetClient) => {
          resetDiscoveryActionErrors();
          const authority = captureSourceAuthority(targetClient);
          if (authority) manualAddMutation.mutate({ authority, targetClient });
        }}
        onStart={(scope) => {
          resetDiscoveryActionErrors();
          discoveryMutation.mutate({ scope, targetClient: client });
        }}
      />

      <SourceRootDialogs
        blocked={queryStateBlocked}
        clientLabel={agentClientLabel(
          renameTarget?.targetClient ?? removeTarget?.targetClient ?? client,
        )}
        onCloseRemove={() => setRemoveTarget(null)}
        onCloseRename={() => setRenameTarget(null)}
        onConfirmRemove={(rootId) => {
          if (!removeTarget || removeTarget.id !== rootId) return;
          removeRootMutation.mutate({
            rootId,
            targetClient: removeTarget.targetClient,
          });
          setRemoveTarget(null);
        }}
        onConfirmRename={(rootId, alias) => {
          if (!renameTarget || renameTarget.id !== rootId) return;
          renameRootMutation.mutate({
            alias,
            rootId,
            targetClient: renameTarget.targetClient,
          });
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
