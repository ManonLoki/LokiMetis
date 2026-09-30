import { useQuery, type QueryKey } from "@tanstack/react-query";
import { useAtom } from "jotai";
import { useEffect, useMemo } from "react";

import { getMonitorCapabilities, getMonitorSettings } from "../api/monitor";
import {
  SKIN_CATALOG_QUERY_KEY,
  skinInstancesQueryKey,
  skinStatusQueryKey,
} from "../api/query-keys";
import { skinApi, skinHostAvailable, type SkinHostKind } from "../api/skins";
import { skinPageSessionAtom } from "../state/skin-page";

/** 以统一的启用、轮询与新鲜度策略订阅一个换肤宿主查询。 */
function useHostQuery<TData>(
  hostAvailable: boolean,
  queryKey: QueryKey,
  queryFn: () => Promise<TData>,
  intervalMs: number,
) {
  return useQuery({
    enabled: hostAvailable,
    queryFn,
    queryKey,
    refetchInterval: hostAvailable ? intervalMs : false,
    staleTime: intervalMs / 2,
  });
}

/** 汇总换肤页全部权威查询，并只在它们共同成功后开放宿主动作。 */
export function useSkinHostQueries() {
  const [session, setSession] = useAtom(skinPageSessionAtom);
  const capabilities = useQuery({
    queryFn: getMonitorCapabilities,
    queryKey: ["monitor-capabilities"],
  });
  const settings = useQuery({
    queryFn: getMonitorSettings,
    queryKey: ["monitor-settings"],
  });
  const enabledTools = useMemo(
    () => new Set(settings.data?.enabledAiTools ?? []),
    [settings.data?.enabledAiTools],
  );
  const hostOptions = useMemo(
    () =>
      (capabilities.data?.aiTools ?? []).filter(
        (item): item is typeof item & { skinHost: SkinHostKind } =>
          enabledTools.has(item.tool) && item.skinHost != null,
      ),
    [capabilities.data?.aiTools, enabledTools],
  );
  const activeHost =
    hostOptions.find((item) => item.skinHost === session.selectedHost)?.skinHost ??
    hostOptions[0]?.skinHost ??
    null;
  const host = activeHost ?? "codex";
  const hostAvailable = skinHostAvailable() && activeHost !== null;

  useEffect(() => {
    if (activeHost !== null && session.selectedHost !== activeHost) {
      setSession((value) => ({ ...value, selectedHost: activeHost }));
    }
  }, [activeHost, session.selectedHost, setSession]);

  const catalog = useHostQuery(hostAvailable, SKIN_CATALOG_QUERY_KEY, skinApi.list, 5_000);
  const instances = useHostQuery(
    hostAvailable,
    skinInstancesQueryKey(host),
    () => skinApi.instances(host),
    4_000,
  );
  const status = useHostQuery(
    hostAvailable,
    skinStatusQueryKey(host),
    () => skinApi.status(host),
    4_000,
  );
  const initialFailure = <T>(data: T | undefined, error: Error | null): Error | null =>
    data === undefined ? error : null;
  const refreshFailure = <T>(data: T | undefined, error: Error | null): Error | null =>
    data === undefined ? null : error;
  // 无可选宿主时忽略上一宿主的缓存错误；后台刷新失败则保留只读缓存但继续禁用写操作。
  const hostQueryFailure = hostAvailable
    ? (initialFailure(catalog.data, catalog.error) ??
      initialFailure(instances.data, instances.error) ??
      initialFailure(status.data, status.error))
    : null;
  const queryFailure =
    initialFailure(capabilities.data, capabilities.error) ??
    initialFailure(settings.data, settings.error) ??
    hostQueryFailure;
  const hostRefreshFailure = hostAvailable
    ? (refreshFailure(catalog.data, catalog.error) ??
      refreshFailure(instances.data, instances.error) ??
      refreshFailure(status.data, status.error))
    : null;
  const queryRefreshFailure =
    refreshFailure(capabilities.data, capabilities.error) ??
    refreshFailure(settings.data, settings.error) ??
    hostRefreshFailure;
  const hostStateReady =
    hostAvailable &&
    capabilities.isSuccess &&
    settings.isSuccess &&
    catalog.isSuccess &&
    instances.isSuccess &&
    status.isSuccess;

  return {
    activeHost,
    capabilities,
    catalog,
    host,
    hostAvailable,
    hostOptions,
    hostStateReady,
    instances,
    queryFailure,
    queryRefreshFailure,
    session,
    setSession,
    settings,
    status,
  };
}
