import { render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { SettingsPage } from "../src/components/SettingsPage";
import { TestProviders } from "./testUtils";

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
      if (command === "get_system_notification_setting") {
        return false;
      }
      if (command === "get_autostart_enabled") {
        return false;
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 侧栏设置页只保留语言、主题、通知、自启等宿主控件，不再展示看板配置或设备信息。 */
  test("settings_page_keeps_host_controls_and_drops_dashboard_and_device_cards", async () => {
    render(
      <TestProviders>
        <SettingsPage />
      </TestProviders>,
    );

    expect(await screen.findByTestId("settings-page")).toBeVisible();
    expect(screen.getByText("Interface language")).toBeVisible();
    expect(screen.getByText("Appearance")).toBeVisible();
    expect(await screen.findByRole("switch", { name: "System notifications" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "Start at login" })).toBeVisible();

    expect(screen.queryByRole("checkbox", { name: "Codex" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Claude Code" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Grok" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "WorkBuddy" })).not.toBeInTheDocument();
    expect(screen.queryByText("Scan interval")).not.toBeInTheDocument();
    expect(screen.queryByText("Automatic cleanup")).not.toBeInTheDocument();
    expect(screen.queryByText("Device identity")).not.toBeInTheDocument();
    expect(screen.queryByTestId("dashboard-settings")).not.toBeInTheDocument();
  });
});
