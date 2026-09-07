// 公共 Agent 设置组合层：只组织既有配置视图，不复制其状态或业务规则。
import { Stack, Text, Title } from "@mantine/core";
import type { ReactElement } from "react";
import { useTranslation } from "react-i18next";

import { DashboardSettingsSection } from "../pages/DashboardSettingsSection";
import { MonitorSettingsPage } from "../pages/MonitorSettingsPage";

/** 在公共设置页集中承载看板采集与 Hooks 的全部 Agent 配置。 */
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

      <DashboardSettingsSection />
      <MonitorSettingsPage />
    </Stack>
  );
}
