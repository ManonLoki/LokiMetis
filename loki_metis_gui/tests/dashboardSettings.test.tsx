import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { SettingsPage } from "../src/components/SettingsPage";
import { UsageSettingsPage } from "../src/pages/UsageSettingsPage";
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

  /** 公共设置只保留统一 Agent 面板与宿主控件，不再嵌入用量参数。 */
  test("settings_page_keeps_agent_configuration_and_host_controls", async () => {
    render(
      <TestProviders>
        <SettingsPage />
      </TestProviders>,
    );

    expect(await screen.findByTestId("settings-page")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Settings" })).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "Manage Agent configuration, local interface preferences, and enabled system capabilities in one place.",
      ),
    ).not.toBeInTheDocument();
    const applicationSection = screen.getByTestId("settings-application-section");
    expect(
      within(applicationSection).getByRole("button", { name: "View release notes" }),
    ).toBeVisible();
    expect(screen.queryByTestId("settings-release-notes-section")).not.toBeInTheDocument();
    expect(
      screen.queryByText("View the local release notes bundled with a formal candidate."),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Interface language")).toBeVisible();
    expect(screen.getByText("Appearance")).toBeVisible();
    expect(
      screen.queryByText(
        "A saved choice takes priority over the system language and updates native menus.",
      ),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "Choose light, dark, or follow system. The preference is stored only on this device.",
      ),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole("switch", { name: "System notifications" }),
    ).toBeVisible();
    expect(screen.getByRole("switch", { name: "Start at login" })).toBeVisible();
    expect(
      screen.queryByText(
        "The app may send native notifications only after you explicitly enable them here.",
      ),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "Launch and show the app normally after sign-in. The operating system login item is authoritative.",
      ),
    ).not.toBeInTheDocument();

    const agentSettings = screen.getByTestId("settings-agent-settings");
    const agentPanel = await within(agentSettings).findByTestId(
      "settings-enabled-agent-panel",
    );
    expect(
      within(agentSettings).queryByRole("heading", { name: "Agent configuration" }),
    ).not.toBeInTheDocument();
    expect(
      within(agentSettings).queryByText(
        "Choose the Agents shared by the dashboard, monitor, desktop pet, and app skins.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(agentPanel).queryByText(
        "One selection drives the dashboard, Hooks, desktop pet, and skins; each page shows only the Agents it supports.",
      ),
    ).not.toBeInTheDocument();
    expect(within(agentPanel).getAllByRole("checkbox")).toHaveLength(5);
    for (const name of ["Codex", "Claude Code", "Cursor", "Grok", "WorkBuddy"]) {
      expect(within(agentPanel).getByRole("checkbox", { name })).toBeVisible();
    }
    expect(within(agentSettings).queryByText("Scan interval")).not.toBeInTheDocument();
    expect(within(agentSettings).queryByText("Automatic cleanup")).not.toBeInTheDocument();
    expect(
      within(agentSettings).queryByTestId("dashboard-settings"),
    ).not.toBeInTheDocument();
    expect(within(agentSettings).queryByTestId("monitor-settings")).not.toBeInTheDocument();
    await userEvent.click(within(agentPanel).getByRole("checkbox", { name: "Cursor" }));
    expect(invokeMock).toHaveBeenCalledWith("save_enabled_ai_selection", {
      client: "codex",
      tools: ["codex", "cursor"],
    });
    expect(screen.queryByText("Device identity")).not.toBeInTheDocument();
  });

  /** React Query 不得把查询上下文误传成更新日志 IPC 调用器。 */
  test("settings_page_calls_the_release_notes_loader_without_query_context", async () => {
    const releaseNotesLoader = vi.fn().mockResolvedValue({
      releases: [
        {
          bugFixes: [],
          featureOptimizations: [
            { "en-US": "Release smoke coverage", "zh-CN": "发布烟雾验收" },
          ],
          releaseDate: "2026-09-08",
          version: "v0.2.15",
        },
      ],
      schemaVersion: 2,
    });
    render(
      <TestProviders>
        <SettingsPage releaseNotesLoader={releaseNotesLoader} />
      </TestProviders>,
    );

    await userEvent.click(
      await screen.findByRole("button", { name: "View release notes" }),
    );

    expect(releaseNotesLoader).toHaveBeenCalledWith();
    expect(
      await screen.findByRole("heading", { name: "2026-09-08 · v0.2.15" }),
    ).toBeInTheDocument();
  });

  /** 用量看板设置页独立承载扫描间隔与自动清理。 */
  test("dashboard_settings_owns_scan_interval_and_cleanup", async () => {
    render(
      <TestProviders>
        <UsageSettingsPage />
      </TestProviders>,
    );

    const dashboardSettings = await screen.findByTestId("dashboard-settings");
    expect(
      within(dashboardSettings).getByRole("heading", { name: "Usage settings" }),
    ).toBeVisible();
    expect(within(dashboardSettings).getByText("Scan interval")).toBeVisible();
    expect(within(dashboardSettings).getByText("Automatic cleanup")).toBeVisible();
    expect(
      within(dashboardSettings).queryByText(
        "Enter an integer from 1 to 1,440 minutes; default 5. Local periodic quick scans use this interval. Saving only changes the cadence.",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(dashboardSettings).queryByText(
        "Enter an integer from 1 to 3,650 days; default 90. Only derived calls and cumulative snapshots older than this window are removed. Agent source files, data roots, and source checkpoints are kept. Saving does not clean immediately; cleanup runs in the background the next time the app opens.",
      ),
    ).not.toBeInTheDocument();
    expect(within(dashboardSettings).queryByRole("checkbox")).not.toBeInTheDocument();
  });
});
