import { Alert, Paper, Stack, Text, Title } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { getHookRelayStatus } from "../api/monitor";

/** 工作台：展示本机 Hook 中继状态，不含局域网设备列表。 */
export function MonitorWorkbenchPage() {
  const { t } = useTranslation();
  const relay = useQuery({
    queryFn: getHookRelayStatus,
    queryKey: ["hook-relay-status"],
    refetchInterval: 3000,
  });
  const status = relay.data;
  return (
    <Stack data-testid="monitor-workbench" gap="md">
      {relay.error ? <Alert color="red">{String(relay.error)}</Alert> : null}
      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="sm">
          <Title order={3}>{t("monitor.workbench.title")}</Title>
          <Text c="dimmed" size="sm">
            {t("monitor.workbench.description")}
          </Text>
          <Text>
            {status?.listening
              ? t("monitor.workbench.listening", { address: status.bindAddress })
              : t("monitor.workbench.offline")}
          </Text>
          <Text>{t("monitor.workbench.received", { count: status?.receivedCount ?? 0 })}</Text>
          <Text>{t("monitor.workbench.failed", { count: status?.failedCount ?? 0 })}</Text>
          <Text>
            {status?.lastEvent
              ? t("monitor.workbench.lastEvent", {
                  tool: status.lastEvent.tool,
                  hookType: status.lastEvent.hookType,
                })
              : t("monitor.workbench.noEvent")}
          </Text>
          {status?.lastError ? (
            <Alert color="red">{t("monitor.workbench.lastError", { error: status.lastError })}</Alert>
          ) : null}
        </Stack>
      </Paper>
    </Stack>
  );
}
