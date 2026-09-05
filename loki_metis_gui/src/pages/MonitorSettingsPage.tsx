import { Alert, Checkbox, Paper, Stack, Text, Title } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import {
  getMonitorCapabilities,
  getMonitorSettings,
  saveMonitorEnabledTools,
  type MonitorAiTool,
} from "../api/monitor";

/** 监控设置：选择要写入 Hooks 的四项 Agent，不是侧栏应用设置。 */
export function MonitorSettingsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const capabilities = useQuery({
    queryFn: getMonitorCapabilities,
    queryKey: ["monitor-capabilities"],
  });
  const settings = useQuery({ queryFn: getMonitorSettings, queryKey: ["monitor-settings"] });
  const save = useMutation({
    mutationFn: (tools: MonitorAiTool[]) => saveMonitorEnabledTools(tools),
    onSuccess: (next) => {
      queryClient.setQueryData(["monitor-settings"], next);
    },
  });
  const enabled = settings.data?.enabledAiTools ?? [];
  return (
    <Stack data-testid="monitor-settings" gap="md">
      <Paper p="lg" radius="lg" withBorder>
        <Stack gap="sm">
          <Title order={3}>{t("monitor.settings.title")}</Title>
          <Text c="dimmed" size="sm">
            {t("monitor.settings.description")}
          </Text>
          {save.error ? <Alert color="red">{String(save.error)}</Alert> : null}
          <Text fw={600}>{t("monitor.settings.enabled")}</Text>
          {(capabilities.data?.aiTools ?? []).map((item) => (
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
    </Stack>
  );
}
