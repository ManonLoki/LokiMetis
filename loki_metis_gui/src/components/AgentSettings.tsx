// 公共 Agent 设置组合层：只承载全局 Agent 启用选择，不复制业务区配置。
import { Stack } from "@mantine/core";
import type { ReactElement } from "react";

import { UnifiedAgentSelectionPanel } from "./UnifiedAgentSelectionPanel";

/** 在公共设置页集中承载唯一 Agent 启用选择。 */
export function AgentSettings(): ReactElement {
  return (
    <Stack component="section" data-testid="settings-agent-settings">
      <UnifiedAgentSelectionPanel />
    </Stack>
  );
}
