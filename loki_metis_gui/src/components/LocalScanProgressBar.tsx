import { Box, Group, Progress, Text } from "@mantine/core";
import { useQueries, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { getLocalScanStatus, type AgentClientKind, type ScanState } from "../api/usage";
import {
  invalidateLocalUsageQueries,
  SCAN_STATUS_POLL_INTERVAL_MS,
} from "../api/usage-queries";
import { scanScopeLabel } from "../i18n/backend-labels";
import { agentClientLabel } from "../state/agent-client";

/** 在看板公共粘滞页头中观察全部已开放 Agent 的单一后台统计 worker。 */
export function LocalScanProgressBar({
  enabledAgents,
}: {
  enabledAgents: AgentClientKind[];
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const previousStates = useRef<Partial<Record<AgentClientKind, ScanState>>>({});
  const queries = useQueries({
    queries: enabledAgents.map((client) => ({
      queryFn: () => getLocalScanStatus(client),
      queryKey: ["scan-status", client],
      // 周期任务可在页面停留期间从 idle 启动，必须持续轻量轮询才能出现进度条。
      refetchInterval: SCAN_STATUS_POLL_INTERVAL_MS,
    })),
  });

  useEffect(() => {
    enabledAgents.forEach((client, index) => {
      const state = queries[index]?.data?.state;
      const previous = previousStates.current[client];
      if (previous === "running" && state && state !== "running") {
        void invalidateLocalUsageQueries(queryClient, client);
      }
      if (state) previousStates.current[client] = state;
    });
  }, [enabledAgents, queries, queryClient]);

  const active = enabledAgents
    .map((client, index) => ({ client, scan: queries[index]?.data }))
    .find((item) => item.scan?.state === "running");
  if (!active?.scan) return null;

  const progress = Math.min(100, Math.max(0, active.scan.progressBasisPoints / 100));
  return (
    <Box aria-live="polite" className="local-scan-progress" data-local-scan-progress="">
      <Group gap="sm" justify="space-between" wrap="nowrap">
        <Text fw={700} size="sm">
          {t("shell.localScanProgress.title", { client: agentClientLabel(active.client) })}
        </Text>
        <Text c="dimmed" size="xs">
          {scanScopeLabel(t, active.scan)}
        </Text>
      </Group>
      <Progress
        aria-label={t("shell.localScanProgress.aria", {
          client: agentClientLabel(active.client),
        })}
        mt={6}
        radius="xl"
        size="sm"
        value={progress}
      />
    </Box>
  );
}
