// 公共 Agent 设置组合层：只组织既有配置视图，不复制其状态或业务规则。
import { Stack, Text, Title } from "@mantine/core";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { DashboardSettingsSection } from "../pages/DashboardSettingsSection";
import { UnifiedAgentSelectionPanel } from "./UnifiedAgentSelectionPanel";

/** 在公共设置页集中承载统一 Agent 选择与看板采集参数。 */
export function AgentSettings(): ReactElement {
  const { t } = useTranslation();

  return (
    <Stack
      aria-labelledby="settings-agent-title"
      component="section"
      data-testid="settings-agent-settings"
      gap="lg"
    >
      <Stack gap={4}>
        <Title id="settings-agent-title" order={2} size="h3">
          {t("settings.agent_title")}
        </Title>
        <Text c="dimmed" size="sm">
          {t("settings.agent_description")}
        </Text>
      </Stack>

      <UnifiedAgentSelectionPanel />
      <DashboardSettingsSection />
    </Stack>
  );
}
