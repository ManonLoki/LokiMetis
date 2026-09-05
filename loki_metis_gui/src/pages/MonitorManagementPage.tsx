import { Alert, Button, Group, Paper, Stack, Text, TextInput, Title } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import {
  getMonitorCapabilities,
  getMonitorSettings,
  listMonitorHookLocations,
  writeMonitorHookConfig,
  type MonitorAiTool,
} from "../api/monitor";

/** 监控管理：为四项 Agent 写入本机 Hooks，无设备门禁。 */
export function MonitorManagementPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const capabilities = useQuery({
    queryFn: getMonitorCapabilities,
    queryKey: ["monitor-capabilities"],
  });
  const settings = useQuery({ queryFn: getMonitorSettings, queryKey: ["monitor-settings"] });
  const locations = useQuery({
    queryFn: listMonitorHookLocations,
    queryKey: ["monitor-hook-locations"],
  });
  const write = useMutation({
    mutationFn: (tool: MonitorAiTool) => writeMonitorHookConfig(tool),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["monitor-hook-locations"] });
    },
  });
  const enabled = settings.data?.enabledAiTools ?? [];
  const tools = (capabilities.data?.aiTools ?? []).filter((item) => enabled.includes(item.tool));
  return (
    <Stack data-testid="monitor-management" gap="md">
      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="sm">
          <Title order={3}>{t("monitor.management.title")}</Title>
          <Text c="dimmed" size="sm">
            {t("monitor.management.description")}
          </Text>
          {write.error ? <Alert color="red">{String(write.error)}</Alert> : null}
          {write.data ? (
            <Alert color="green">
              {t("monitor.management.written", {
                file: write.data.filename,
                outcome: t(`monitor.outcome.${write.data.outcome}`),
              })}
            </Alert>
          ) : null}
          {tools.map((item) => {
            const location = locations.data?.find((entry) => entry.tool === item.tool);
            return (
              <Paper key={item.tool} p="md" radius="md" withBorder>
                <Stack gap="xs">
                  <Group justify="space-between">
                    <Text fw={700}>{item.name}</Text>
                    <Button
                      loading={write.isPending && write.variables === item.tool}
                      onClick={() => write.mutate(item.tool)}
                      size="xs"
                    >
                      {t("monitor.management.write")}
                    </Button>
                  </Group>
                  <TextInput
                    label={t("monitor.management.directory")}
                    readOnly
                    value={location?.directory ?? ""}
                  />
                  <Text c="dimmed" size="xs">
                    {t("monitor.management.location", { path: location?.configPath ?? "" })}
                  </Text>
                </Stack>
              </Paper>
            );
          })}
        </Stack>
      </Paper>
    </Stack>
  );
}
