import { render, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { SettingsPage } from "../src/components/SettingsPage";
import {
  availableDashboardAiTypesFixture,
  monitorCapabilitiesFixture,
  TestProviders,
} from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 构造设置页看板配置 IPC 快照。 */
function privacySettings() {
  return {
    languagePreference: "system",
    localOnly: true,
    deviceUsername: null,
    deviceName: "test-host",
    deviceUniqueId: "11111111-2222-4333-8444-555555555555",
    scanIntervalMinutes: 5,
    retentionDays: 90,
    deviceTimeZone: "UTC",
    indexLocationLabel: "Codex index",
    indexLocationCode: "codex",
    indexSizeBytes: null,
    lastClearedAtEpochMs: null,
    availableAiTypes: availableDashboardAiTypesFixture,
    enabledAgents: [],
    workbuddyStatsEnabled: false,
  };
}

describe("dashboard settings capabilities", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_app_metadata") {
        return {
          applicationName: "LokiMetis",
          productDefinitionRequired: true,
          title: "LokiMetis",
          version: "0.1.0",
        };
      }
      if (command === "get_privacy_settings") {
        return privacySettings();
      }
      if (command === "get_monitor_capabilities") {
        return monitorCapabilitiesFixture({
          aiTools: [
            { tool: "codex", name: "Codex" },
            { tool: "cursor", name: "Cursor" },
          ],
        });
      }
      if (command === "get_monitor_settings") {
        return { enabledAiTools: ["codex"], hookDirectories: {} };
      }
      if (command === "list_monitor_hook_locations") {
        return [
          {
            tool: "codex",
            directory: "/tmp/codex",
            configPath: "/tmp/codex/hooks.json",
            isCustom: true,
          },
          {
            tool: "cursor",
            directory: "/tmp/cursor",
            configPath: "/tmp/cursor/hooks.json",
            isCustom: true,
          },
        ];
      }
      if (command === "get_system_notification_setting") {
        return false;
      }
      if (command === "get_autostart_enabled") {
        return false;
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 侧栏公共设置集中展示全部 Agent 配置，并继续保留宿主控件。 */
  test("settings_page_centralizes_agent_configuration_and_keeps_host_controls", async () => {
    render(
      <TestProviders>
        <SettingsPage />
      </TestProviders>,
    );

    expect(await screen.findByTestId("settings-page")).toBeVisible();
    expect(screen.getByText("Interface language")).toBeVisible();
    expect(screen.getByText("Appearance")).toBeVisible();
    expect(
      await screen.findByRole("switch", { name: "System notifications" }),
    ).toBeVisible();
    expect(screen.getByRole("switch", { name: "Start at login" })).toBeVisible();

    const agentSettings = screen.getByTestId("settings-agent-settings");
    const dashboardSettings = await within(agentSettings).findByTestId("dashboard-settings");
    const dashboardAgents = await within(dashboardSettings).findByTestId(
      "dashboard-enabled-agent-options",
    );
    const monitorSettings = within(agentSettings).getByTestId("monitor-settings");
    expect(within(agentSettings).getByText("Agent configuration")).toBeVisible();
    expect(within(dashboardAgents).getByRole("checkbox", { name: "Codex" })).toBeVisible();
    expect(
      within(dashboardAgents).getByRole("checkbox", { name: "Claude Code" }),
    ).toBeVisible();
    expect(within(dashboardAgents).getByRole("checkbox", { name: "Grok" })).toBeVisible();
    expect(within(dashboardAgents).getByRole("checkbox", { name: "WorkBuddy" })).toBeVisible();
    expect(within(dashboardSettings).getByText("Scan interval")).toBeVisible();
    expect(within(dashboardSettings).getByText("Automatic cleanup")).toBeVisible();
    expect(within(monitorSettings).getByText("Hooks settings")).toBeVisible();
    expect(
      await within(monitorSettings).findByRole("checkbox", { name: "Cursor" }),
    ).toBeVisible();
    expect(
      await within(monitorSettings).findByRole("button", { name: "Write Hooks" }),
    ).toBeVisible();
    expect(screen.queryByText("Device identity")).not.toBeInTheDocument();
  });
});
