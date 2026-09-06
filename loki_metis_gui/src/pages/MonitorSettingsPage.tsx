import {
  Alert,
  Button,
  Card,
  Checkbox,
  Group,
  SimpleGrid,
  Stack,
  Tabs,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
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
  const settings = useQuery({
    queryFn: getMonitorSettings,
    queryKey: ["monitor-settings"],
  });
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
  const visibleTools = tools.filter((item) => enabled.includes(item.tool));
  const [selectedTool, setSelectedTool] = useState<MonitorAiTool | null>(null);
  const activeTool = visibleTools.some((item) => item.tool === selectedTool)
    ? selectedTool
    : (visibleTools[0]?.tool ?? null);
  return (
    <Stack className="settings-page" data-testid="monitor-settings" gap="sm">
      <Card
        aria-describedby="monitor-hooks-settings-description"
        aria-labelledby="monitor-hooks-settings-title"
        className="surface-card settings-card"
        data-testid="monitor-enabled-agents"
        p="sm"
        radius="lg"
        role="region"
        withBorder
      >
        <Stack gap="sm">
          <div>
            <Title id="monitor-hooks-settings-title" order={3}>
              {t("monitor.settings.title")}
            </Title>
            <Text c="dimmed" id="monitor-hooks-settings-description" mt={2} size="xs">
              {t("monitor.settings.description")}
            </Text>
          </div>
          {save.error ? <Alert color="red">{String(save.error)}</Alert> : null}
          <Text fw={600}>{t("monitor.settings.enabled")}</Text>
          <SimpleGrid cols={{ base: 2, sm: 3, md: 4 }} spacing="xs" verticalSpacing="xs">
            {tools.map((item) => (
              <Checkbox
                checked={enabled.includes(item.tool)}
                disabled={capabilities.isPending || settings.isPending || save.isPending}
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
          </SimpleGrid>
        </Stack>
      </Card>

      <Card
        aria-describedby="monitor-hooks-management-description"
        aria-labelledby="monitor-hooks-management-title"
        className="surface-card settings-card hooks-management-card"
        data-testid="monitor-hooks-management"
        p="sm"
        radius="lg"
        role="region"
        withBorder
      >
        <Stack gap="sm">
          <div>
            <Title id="monitor-hooks-management-title" order={4}>
              {t("monitor.settings.managementTitle")}
            </Title>
            <Text c="dimmed" id="monitor-hooks-management-description" mt={2} size="xs">
              {t("monitor.settings.managementDescription")}
            </Text>
          </div>
          {locations.error ? <Alert color="red">{String(locations.error)}</Alert> : null}
          {visibleTools.length === 0 ? (
            <Alert color="blue" variant="light">
              {t("monitor.settings.chooseAgentFirst")}
            </Alert>
          ) : (
            <Tabs
              className="ai-tool-tabs"
              keepMounted={false}
              onChange={(value) => {
                if (value) setSelectedTool(value as MonitorAiTool);
              }}
              value={activeTool}
            >
              <Tabs.List grow>
                {visibleTools.map((item) => (
                  <Tabs.Tab key={item.tool} value={item.tool}>
                    {item.name}
                  </Tabs.Tab>
                ))}
              </Tabs.List>
              {visibleTools.map((item) => {
                const location = locations.data?.find((entry) => entry.tool === item.tool);
                const isCurrentWrite = write.variables === item.tool;
                return (
                  <Tabs.Panel key={item.tool} pt="xs" value={item.tool}>
                    <Stack gap="sm">
                      <TextInput
                        label={t("monitor.settings.directory")}
                        readOnly
                        size="xs"
                        value={location?.directory ?? ""}
                      />
                      <TextInput
                        label={t("monitor.settings.configFile")}
                        readOnly
                        size="xs"
                        value={location?.configPath ?? ""}
                      />
                      <Group justify="flex-end">
                        <Button
                          loading={write.isPending && isCurrentWrite}
                          onClick={() => write.mutate(item.tool)}
                          size="xs"
                        >
                          {t("monitor.settings.write")}
                        </Button>
                      </Group>
                      {write.error && isCurrentWrite ? (
                        <Alert color="red">{String(write.error)}</Alert>
                      ) : null}
                      {write.data?.tool === item.tool ? (
                        <Alert aria-live="polite" color="green">
                          {t("monitor.settings.written", {
                            file: write.data.filename,
                            outcome: t(`monitor.outcome.${write.data.outcome}`),
                          })}
                        </Alert>
                      ) : null}
                    </Stack>
                  </Tabs.Panel>
                );
              })}
            </Tabs>
          )}
        </Stack>
      </Card>
    </Stack>
  );
}
