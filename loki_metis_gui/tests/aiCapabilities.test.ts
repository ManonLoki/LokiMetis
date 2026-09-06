import { describe, expect, test } from "vitest";

import {
  findAvailableMonitorAiTool,
  selectAvailableMonitorAiTools,
  selectDashboardClientOptions,
  selectDashboardWorkbuddyOption,
  selectEnabledAvailableMonitorTools,
  selectEnabledDashboardClients,
} from "../src/ai-capabilities";
import type { MonitorAiToolDescriptor } from "../src/api/monitor";
import type { AvailableAiTypeDto } from "../src/api/usage-types";

describe("AI capability projection", () => {
  /** 监控公开项完全服从后端 capability 顺序，并丢弃重复与历史启用值。 */
  test("normalizes_monitor_tools_from_backend_capabilities", () => {
    const available: MonitorAiToolDescriptor[] = [
      { tool: "claudeCode", name: "Claude" },
      { tool: "codex", name: "Codex" },
      { tool: "claudeCode", name: "Duplicate Claude" },
    ];

    expect(selectAvailableMonitorAiTools(available)).toEqual([
      { tool: "claudeCode", name: "Claude" },
      { tool: "codex", name: "Codex" },
    ]);
    expect(
      selectEnabledAvailableMonitorTools(["codex", "openCode", "claudeCode"], available),
    ).toEqual(["claudeCode", "codex"]);
    expect(findAvailableMonitorAiTool(available, "openCode")).toBeNull();
  });

  /** 看板只消费后端提供且能映射到既有客户端或 WorkBuddy 视图的目录项。 */
  test("ignores_unmappable_dashboard_catalog_values", () => {
    const available = [
      { name: "Claude", value: "claudeCode" },
      { name: "Cursor", value: "cursor" },
      { name: "Codex", value: "codex" },
      { name: "Duplicate Codex", value: "codex" },
      { name: "WorkBuddy", value: "workbuddy" },
    ] as unknown as AvailableAiTypeDto[];

    expect(selectDashboardClientOptions(available)).toEqual([
      { label: "Claude", value: "claudeCode" },
      { label: "Codex", value: "codex" },
    ]);
    expect(
      selectEnabledDashboardClients(["codex", "grokBuildCli", "claudeCode"], available),
    ).toEqual(["claudeCode", "codex"]);
    expect(selectDashboardWorkbuddyOption(available)).toEqual({
      name: "WorkBuddy",
      value: "workbuddy",
    });
  });
});
