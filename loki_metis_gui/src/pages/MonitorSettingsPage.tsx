import { Alert, Button, Checkbox, Group, Paper, Stack, Text, TextInput, Title } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import {
  getMonitorCapabilities,
  getMonitorSettings,
  listMonitorHookLocations,
  saveMonitorEnabledTools,
  writeMonitorHookConfig,
  type MonitorAiTool,
} from "../api/monitor";

/** Hooks 设置：启用 Agent、查看配置目录并写入本机 Hooks。 */
export function MonitorSettingsPage() {
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
  const save = useMutation({
    mutationFn: (tools: MonitorAiTool[]) => saveMonitorEnabledTools(tools),
    onSuccess: (next) => {
      queryClient.setQueryData(["monitor-settings"], next);
    },
  });
  const write = useMutation({
    mutationFn: (tool: MonitorAiTool) => writeMonitorHookConfig(tool),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["monitor-hook-locations"] });
    },
  });
  const enabled = settings.data?.enabledAiTools ?? [];
  const tools = capabilities.data?.aiTools ?? [];
  return (
    <Stack data-testid="monitor-settings" gap="md">
      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="sm">
          <Title order={3}>{t("monitor.settings.title")}</Title>
          <Text c="dimmed" size="sm">
            {t("monitor.settings.description")}
          </Text>
          {save.error ? <Alert color="red">{String(save.error)}</Alert> : null}
          {write.error ? <Alert color="red">{String(write.error)}</Alert> : null}
          {write.data ? (
            <Alert color="green">
              {t("monitor.settings.written", {
                file: write.data.filename,
                outcome: t(`monitor.outcome.${write.data.outcome}`),
              })}
            </Alert>
          ) : null}
          <Text fw={600}>{t("monitor.settings.enabled")}</Text>
          {tools.map((item) => (
            <Checkbox
              checked={enabled.includes(item.tool)}
              key={item.tool}
              label={item.name}
              onChange={(event) => {
                const next = event.currentTarget.checked
                  ? [...enabled, item.tool]
                  : enabled.filter((tool) => tool !== item.tool);
                save.mutate(next);
              }}
            />
          ))}
        </Stack>
      </Paper>
      {tools
        .filter((item) => enabled.includes(item.tool))
        .map((item) => {
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
                    {t("monitor.settings.write")}
                  </Button>
                </Group>
                <TextInput
                  label={t("monitor.settings.directory")}
                  readOnly
                  value={location?.directory ?? ""}
                />
                <Text c="dimmed" size="xs">
                  {t("monitor.settings.location", { path: location?.configPath ?? "" })}
                </Text>
              </Stack>
            </Paper>
          );
        })}
    </Stack>
  );
}
