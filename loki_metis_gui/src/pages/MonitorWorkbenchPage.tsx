import {
  Alert,
  Badge,
  Card,
  Code,
  Group,
  SimpleGrid,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { getHookRelayStatus } from "../api/monitor";

/** 中继指标块：以稳定的标签和值展示一项本机统计。 */
function RelayMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="endpoint-preview">
      <Text c="dimmed" size="xs">
        {label}
      </Text>
      <Text fw={700} size="xl">
        {value}
      </Text>
    </div>
  );
}

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
      <Card className="surface-card" p="md" radius="lg" withBorder>
        <Stack gap="md">
          <Group align="flex-start" justify="space-between" wrap="wrap">
            <div>
              <Title order={3}>{t("monitor.workbench.title")}</Title>
              <Text c="dimmed" mt={4} size="sm">
                {t("monitor.workbench.description")}
              </Text>
            </div>
            <Badge color={status?.listening ? "green" : "red"} variant="light">
              {status?.listening
                ? t("monitor.workbench.listening", { address: status.bindAddress })
                : t("monitor.workbench.offline")}
            </Badge>
          </Group>
          <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
            <RelayMetric
              label={t("monitor.workbench.receivedLabel")}
              value={status?.receivedCount ?? 0}
            />
            <RelayMetric
              label={t("monitor.workbench.failedLabel")}
              value={status?.failedCount ?? 0}
            />
          </SimpleGrid>
          {status?.lastEvent ? (
            <Text size="sm">
              {t("monitor.workbench.lastEventLabel")} <Code>{status.lastEvent.tool}</Code> /{" "}
              <Code>{status.lastEvent.hookType}</Code>
            </Text>
          ) : (
            <Text c="dimmed" size="sm">
              {t("monitor.workbench.noEvent")}
            </Text>
          )}
          {status?.lastError ? (
            <Alert color="red">
              {t("monitor.workbench.lastError", { error: status.lastError })}
            </Alert>
          ) : null}
        </Stack>
      </Card>
    </Stack>
  );
}
