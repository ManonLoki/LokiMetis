import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
            { tool: "codex", name: "Codex", dashboardClient: "codex", skinHost: "codex" },
            { tool: "claudeCode", name: "Claude Code", dashboardClient: "claudeCode" },
            { tool: "cursor", name: "Cursor" },
            { tool: "grok", name: "Grok", dashboardClient: "grokBuildCli" },
            {
              tool: "workBuddy",
              name: "WorkBuddy",
              dashboardClient: "workbuddy",
              skinHost: "workBuddy",
            },
          ],
        });
      }
      if (command === "get_monitor_settings") {
        return { enabledAiTools: ["codex"], hookDirectories: {} };
      }
      if (command === "save_enabled_ai_selection") {
        return {
          monitorSettings: { enabledAiTools: ["codex", "cursor"], hookDirectories: {} },
          privacySettings: privacySettings(),
        };
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

  /** 公共设置保留统一 Agent 面板与看板参数，不再嵌入 Hooks 配置。 */
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
    const dashboardSettings =
      await within(agentSettings).findByTestId("dashboard-settings");
    const agentPanel = await within(agentSettings).findByTestId(
      "settings-enabled-agent-panel",
    );
    expect(within(agentSettings).getByText("Agent configuration")).toBeVisible();
    expect(within(agentPanel).getAllByRole("checkbox")).toHaveLength(5);
    for (const name of ["Codex", "Claude Code", "Cursor", "Grok", "WorkBuddy"]) {
      expect(within(agentPanel).getByRole("checkbox", { name })).toBeVisible();
    }
    expect(within(dashboardSettings).getByText("Scan interval")).toBeVisible();
    expect(within(dashboardSettings).getByText("Automatic cleanup")).toBeVisible();
    expect(within(dashboardSettings).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(within(agentSettings).queryByTestId("monitor-settings")).not.toBeInTheDocument();
    await userEvent.click(within(agentPanel).getByRole("checkbox", { name: "Cursor" }));
    expect(invokeMock).toHaveBeenCalledWith("save_enabled_ai_selection", {
      client: "codex",
      tools: ["codex", "cursor"],
    });
    expect(screen.queryByText("Device identity")).not.toBeInTheDocument();
  });
});
