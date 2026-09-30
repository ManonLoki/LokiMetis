import { Stack } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import {
  getWorkbuddySourceStatus,
  type RootDiscoveryScope,
  type RootDiscoveryStatusDto,
} from "../api/usage";
import { FailureState, LoadingState } from "../components/StatePanels";
import { SourceDiscoveryPanel } from "./SourceDiscoveryPanel";
import { SourceRootTable } from "./SourceRootTable";

/** WorkBuddy 数据源查询的稳定缓存键；开关切换后据此失效重取。 */
const WORKBUDDY_SOURCE_STATUS_QUERY_KEY = ["workbuddy-source-status"];

/** 构造 WorkBuddy 发现面板的空闲或刚完成状态；不走三个物理 Agent 的全盘发现。 */
function workbuddyDiscoveryStatus(
  state: "idle" | "complete",
  scope: RootDiscoveryScope,
  rootCount: number,
): RootDiscoveryStatusDto {
  const complete = state === "complete";
  const found = complete ? rootCount : 0;
  return {
    candidatesFound: found,
    directoriesChecked: found,
    errorCode: null,
    fallbackPerformed: false,
    fileNamesChecked: found,
    ioErrors: 0,
    platform: "other",
    permissionDenied: 0,
    scope,
    skipped: 0,
    state,
    strategy: "metadataTraversal",
    systemIndexAvailable: false,
    volumesCompleted: found,
    volumesTotal: found,
  };
}

/** WorkBuddy 数据源页：与 Codex/Claude Code/Grok 同一横向菜单位置，复用根表
 * 与发现面板，但只探测固定的 `~/.workbuddy` 与 `~/.workbuddy-ai` 两个只读根，
 * 不登记产品数据根、不建索引。 */
export function WorkbuddySources() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [discovery, setDiscovery] = useState<RootDiscoveryStatusDto>(() =>
    workbuddyDiscoveryStatus("idle", "userPriority", 0),
  );
  const statusQuery = useQuery({
    queryFn: getWorkbuddySourceStatus,
    queryKey: WORKBUDDY_SOURCE_STATUS_QUERY_KEY,
  });
  const reindexMutation = useMutation({
    mutationFn: async () => {
      await queryClient.invalidateQueries({ queryKey: ["workbuddy-usage-statistics"] });
      await queryClient.invalidateQueries({ queryKey: ["workbuddy-statistics"] });
    },
  });

  if (statusQuery.isPending) {
    return <LoadingState label={t("workbuddySources.loading")} />;
  }
  if (statusQuery.isError) {
    return (
      <FailureState error={statusQuery.error} onRetry={() => void statusQuery.refetch()} />
    );
  }

  const status = statusQuery.data;
  const roots = status.roots;

  return (
    <Stack className="page-stack" data-testid="workbuddy-sources" gap="xl">
      <SourceRootTable
        allowRenameRemove={false}
        allowToggle={false}
        currentRootId={null}
        onRemove={() => undefined}
        onRename={() => undefined}
        onReindex={() => reindexMutation.mutate()}
        onToggleEnabled={() => undefined}
        removePending={false}
        reindexPendingRootId={
          reindexMutation.isPending ? (roots[0]?.id ?? "workbuddy") : null
        }
        renamePending={false}
        roots={roots}
        scanRunning={statusQuery.isFetching}
        togglePending={false}
      />

      <SourceDiscoveryPanel
        cancelPending={false}
        candidates={[]}
        discovery={discovery}
        discoveryPending={statusQuery.isFetching}
        manualAddClients={[]}
        manualAddPending={false}
        mutationBlocked={false}
        onAdd={async () => undefined}
        onCancel={() => undefined}
        onManualAdd={() => undefined}
        onStart={(scope) => {
          setDiscovery(workbuddyDiscoveryStatus("idle", scope, 0));
          void statusQuery.refetch().then((result) => {
            setDiscovery(
              workbuddyDiscoveryStatus("complete", scope, result.data?.roots.length ?? 0),
            );
          });
        }}
      />
    </Stack>
  );
}
