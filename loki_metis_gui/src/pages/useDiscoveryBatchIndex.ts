import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useRef, useState } from 'react';

import {
  addRootCandidate,
  listRootCandidates,
  refreshLocalIndexes,
  type AgentClientKind,
  type RootCandidateDto,
  type RootDiscoveryStatusDto,
} from '../api/usage';
import { invalidateLocalUsageQueries } from '../api/usage-queries';
import { ROOT_CANDIDATES_QUERY_KEY } from './useRootCandidateEvents';
import { selectVisibleDiscoveryCandidates } from './visible-discovery-candidates';

/** 保存一次有界发现批次的 Agent、候选与游标。 */
interface DiscoveryBatch {
  client: AgentClientKind;
  id: number;
}

/** 定义批量登记与索引 hook 的依赖和完成回调。 */
interface DiscoveryBatchIndexOptions {
  businessReady: boolean;
  candidates: RootCandidateDto[] | undefined;
  client: AgentClientKind;
  discovery: RootDiscoveryStatusDto | undefined;
}

/** 发现期间只登记候选；终态后等待全部登记完成，再统一执行一次近 30 日索引。 */
export function useDiscoveryBatchIndex({
  businessReady,
  candidates,
  client,
  discovery,
}: DiscoveryBatchIndexOptions) {
  const queryClient = useQueryClient();
  const sequenceRef = useRef(0);
  const activeBatchRef = useRef<DiscoveryBatch | null>(null);
  const registrationPromisesRef = useRef<Map<string, Promise<void>>>(new Map());
  const finalizedBatchIdsRef = useRef<Set<number>>(new Set());
  const recoveredTerminalKeysRef = useRef<Set<string>>(new Set());
  const [activeBatch, setActiveBatch] = useState<DiscoveryBatch | null>(null);
  const [failedCandidateIds, setFailedCandidateIds] = useState<Set<string>>(() => new Set());
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  const setCurrentBatch = useCallback((batch: DiscoveryBatch | null) => {
    activeBatchRef.current = batch;
    setActiveBatch(batch);
  }, []);

  const begin = useCallback(
    (targetClient: AgentClientKind) => {
      const batch = { client: targetClient, id: sequenceRef.current + 1 };
      sequenceRef.current = batch.id;
      setFailedCandidateIds(new Set());
      setError(null);
      setCurrentBatch(batch);
    },
    [setCurrentBatch],
  );

  const abort = useCallback(() => {
    setCurrentBatch(null);
  }, [setCurrentBatch]);

  const registerCandidate = useCallback(
    (candidate: RootCandidateDto): Promise<void> => {
      const current = registrationPromisesRef.current.get(candidate.id);
      if (current) return current;

      const registration = addRootCandidate(candidate.id)
        .then(async (result) => {
          await Promise.all([
            invalidateLocalUsageQueries(queryClient, result.client),
            queryClient.invalidateQueries({ queryKey: ROOT_CANDIDATES_QUERY_KEY }),
          ]);
          setFailedCandidateIds((failed) => {
            if (!failed.has(candidate.id)) return failed;
            const next = new Set(failed);
            next.delete(candidate.id);
            return next;
          });
        })
        .catch((cause: unknown) => {
          setFailedCandidateIds((failed) => new Set(failed).add(candidate.id));
          throw cause;
        })
        .finally(() => {
          registrationPromisesRef.current.delete(candidate.id);
        });
      registrationPromisesRef.current.set(candidate.id, registration);
      return registration;
    },
    [queryClient],
  );

  /** 发现运行期间可以持续登记，但这里绝不触发索引。 */
  useEffect(() => {
    if (!businessReady || !candidates) return;
    const targetClient = activeBatch?.client ?? client;
    for (const candidate of selectVisibleDiscoveryCandidates(candidates, [targetClient])) {
      if (failedCandidateIds.has(candidate.id)) continue;
      void registerCandidate(candidate).catch(() => undefined);
    }
  }, [
    activeBatch?.client,
    businessReady,
    candidates,
    client,
    failedCandidateIds,
    registerCandidate,
  ]);

  /** 页面重新挂载或切换 Agent 时，恢复运行中或尚有候选的终态批次。 */
  useEffect(() => {
    if (!businessReady || activeBatch || !discovery) return;
    if (discovery.state === 'running') {
      void Promise.resolve().then(() => begin(client));
      return;
    }
    if (!candidates || discovery.state === 'idle' || discovery.state === 'failed') return;
    if (selectVisibleDiscoveryCandidates(candidates, [client]).length === 0) return;
    const terminalKey = `${client}:${discovery.scope}:${discovery.state}`;
    if (recoveredTerminalKeysRef.current.has(terminalKey)) return;
    recoveredTerminalKeysRef.current.add(terminalKey);
    void Promise.resolve().then(() => begin(client));
  }, [activeBatch, begin, businessReady, candidates, client, discovery]);

  useEffect(() => {
    if (!activeBatch || !discovery) return;
    if (discovery.state === 'idle' || discovery.state === 'running') return;
    if (finalizedBatchIdsRef.current.has(activeBatch.id)) return;
    finalizedBatchIdsRef.current.add(activeBatch.id);

    if (discovery.state === 'failed') {
      void Promise.resolve().then(() => setCurrentBatch(null));
      return;
    }

    const batch = activeBatch;
    void Promise.resolve()
      .then(async () => {
        setIsRefreshing(true);
        const terminalCandidates = await queryClient.fetchQuery({
          queryFn: listRootCandidates,
          queryKey: ROOT_CANDIDATES_QUERY_KEY,
        });
        const registrations = selectVisibleDiscoveryCandidates(terminalCandidates, [
          batch.client,
        ]).map(registerCandidate);
        await Promise.allSettled(registrations);
        if (activeBatchRef.current?.id !== batch.id) return;
        await refreshLocalIndexes([batch.client], 'discoveryBatch');
        await invalidateLocalUsageQueries(queryClient, batch.client);
      })
      .catch((cause: unknown) => {
        setError(cause instanceof Error ? cause : new Error('batch-index-refresh-failed'));
      })
      .finally(() => {
        if (activeBatchRef.current?.id === batch.id) setCurrentBatch(null);
        setIsRefreshing(false);
      });
  }, [activeBatch, discovery, queryClient, registerCandidate, setCurrentBatch]);

  return {
    abort,
    begin,
    error,
    failedCandidateIds,
    isActive: activeBatch !== null,
    isRefreshing,
    retryCandidate: async (candidate: RootCandidateDto) => {
      setFailedCandidateIds((failed) => {
        const next = new Set(failed);
        next.delete(candidate.id);
        return next;
      });
      await registerCandidate(candidate);
    },
  };
}
