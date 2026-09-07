// 设置页唯一的 Agent 启用入口；各业务区域只消费保存后的能力子集。
import {
  Alert,
  Badge,
  Checkbox,
  Group,
  Paper,
  SimpleGrid,
  Stack,
  Title,
} from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAtomValue } from "jotai";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  getMonitorCapabilities,
  getMonitorSettings,
  saveEnabledAiSelection,
  type MonitorAiTool,
} from "../api/monitor";
import { synchronizeGlobalPrivacySettings } from "../api/usage-queries";
import {
  selectAvailableMonitorAiTools,
  selectEnabledAvailableMonitorTools,
} from "../ai-capabilities";
import { agentClientAtom } from "../state/agent-client";
import { visibleErrorMessage } from "../visible-error";

/** 渲染全产品唯一的 Agent 复选面板，并通过一个命令保存所有区域投影。 */
export function UnifiedAgentSelectionPanel(): ReactElement {
  const { t } = useTranslation();
  const client = useAtomValue(agentClientAtom);
  const queryClient = useQueryClient();
  const capabilities = useQuery({
    queryFn: getMonitorCapabilities,
    queryKey: ["monitor-capabilities"],
  });
  const settings = useQuery({
    queryFn: getMonitorSettings,
    queryKey: ["monitor-settings"],
  });
  const tools = selectAvailableMonitorAiTools(capabilities.data?.aiTools ?? []);
  const enabled = selectEnabledAvailableMonitorTools(
    settings.data?.enabledAiTools ?? [],
    tools,
  );
  const save = useMutation({
    mutationFn: (selected: MonitorAiTool[]) => saveEnabledAiSelection(client, selected),
    onSuccess: (result) => {
      queryClient.setQueryData(["monitor-settings"], result.monitorSettings);
      synchronizeGlobalPrivacySettings(queryClient, result.privacySettings);
    },
  });

  return (
    <Paper
      aria-labelledby="settings-enabled-agent-panel-title"
      className="surface-card settings-card"
      data-testid="settings-enabled-agent-panel"
      p="lg"
      radius="lg"
      role="region"
      withBorder
    >
      <Stack gap="sm">
        <Group gap="xs">
          <Title id="settings-enabled-agent-panel-title" order={3}>
            {t("settings.agent_panel_title")}
          </Title>
          <Badge color="blue" variant="light">
            {t("settings.agent_panel_badge")}
          </Badge>
        </Group>
        <SimpleGrid
          aria-labelledby="settings-enabled-agent-panel-title"
          cols={{ base: 2, sm: 3, md: 5 }}
          role="group"
          spacing="xs"
          verticalSpacing="xs"
        >
          {tools.map((item) => (
            <Checkbox
              checked={enabled.includes(item.tool)}
              disabled={capabilities.isPending || settings.isPending || save.isPending}
              key={item.tool}
              label={item.name}
              onChange={(event) => {
                const selected = new Set(enabled);
                if (event.currentTarget.checked) selected.add(item.tool);
                else selected.delete(item.tool);
                save.mutate(
                  tools.map((tool) => tool.tool).filter((tool) => selected.has(tool)),
                );
              }}
            />
          ))}
        </SimpleGrid>
        {capabilities.isError || settings.isError || save.isError ? (
          <Alert color="red" role="alert" title={t("ui.failureTitle")}>
            {visibleErrorMessage(capabilities.error ?? settings.error ?? save.error)}
          </Alert>
        ) : null}
      </Stack>
    </Paper>
  );
}
