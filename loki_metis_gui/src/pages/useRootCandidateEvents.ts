import { listen } from "@tauri-apps/api/event";
import type { QueryClient } from "@tanstack/react-query";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import type { RootCandidateDto } from "../api/usage";

export const ROOT_CANDIDATES_QUERY_KEY = ["root-candidates"] as const;
export const ROOT_DISCOVERY_CANDIDATE_EVENT = "root-discovery-candidate";

/** 将实时候选按临时 ID 合并到查询缓存，重复事件保留首次结果。 */
export function mergeRootCandidate(
  current: RootCandidateDto[] | undefined,
  candidate: RootCandidateDto,
): RootCandidateDto[] {
  if (current?.some((item) => item.id === candidate.id)) return current;
  return [...(current ?? []), candidate];
}

/** 订阅 Tauri 候选事件；轮询查询仍作为晚订阅或丢事件时的恢复通道。 */
export function useRootCandidateEvents(enabled: boolean) {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!enabled) return undefined;
    let disposed = false;
    let stopListening: (() => void) | undefined;
    void listen<RootCandidateDto>(ROOT_DISCOVERY_CANDIDATE_EVENT, ({ payload }) => {
      queryClient.setQueryData<RootCandidateDto[]>(ROOT_CANDIDATES_QUERY_KEY, (current) =>
        mergeRootCandidate(current, payload),
      );
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stopListening = unlisten;
      })
      .catch(() => {
        // 普通浏览器预览没有 Tauri 事件通道，保留查询轮询即可。
      });
    return () => {
      disposed = true;
      stopListening?.();
    };
  }, [enabled, queryClient]);
}

/** 新任务启动前清空上一次仅存于运行时的候选。 */
export function clearRootCandidateCache(queryClient: QueryClient) {
  queryClient.setQueryData<RootCandidateDto[]>(ROOT_CANDIDATES_QUERY_KEY, []);
}
