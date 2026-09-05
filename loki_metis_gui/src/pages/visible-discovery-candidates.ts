import type { AgentClientKind, RootCandidateDto } from '../api/usage';

/**
 * 各 Agent 数据源列表只展示本客户端候选；计数与胶囊必须来自同一集合。
 * Cursor 仍不进入本机候选。
 */
export function selectVisibleDiscoveryCandidates(
  candidates: readonly RootCandidateDto[],
  allowedClients: readonly AgentClientKind[],
): RootCandidateDto[] {
  const allowed = new Set(allowedClients);
  return candidates.filter((candidate) => allowed.has(candidate.client));
}

/** 返回与可见胶囊一一对应的候选计数，避免全局计数配上空列表。 */
export function visibleDiscoveryCandidateCount(
  candidates: readonly RootCandidateDto[],
  allowedClients: readonly AgentClientKind[],
): number {
  return selectVisibleDiscoveryCandidates(candidates, allowedClients).length;
}
