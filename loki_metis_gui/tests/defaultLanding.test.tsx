import { render, screen, waitFor } from "@testing-library/react";
import {
  RouterProvider,
  createMemoryHistory,
  createRouter,
} from "@tanstack/react-router";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import {
  isDashboardLandingPath,
  resolveDefaultLandingPath,
} from "../src/default-landing";
import { routeTree } from "../src/routeTree.gen";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 根路径默认落地到看板所需的最小 IPC 快照。 */
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
    enabledAgents: ["codex", "claudeCode", "grokBuildCli"],
    workbuddyStatsEnabled: false,
  };
}

describe("default application landing", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_app_metadata") {
        return {
          applicationName: "LokiMetis",
          productDefinitionRequired: true,
          title: "LokiMetis",
          version: "0.2.0",
        };
      }
      if (command === "get_privacy_settings") {
        return privacySettings();
      }
      if (command === "get_local_scan_status") {
        return { state: "idle" };
      }
      if (command === "get_usage_overview") {
        throw new Error("overview not needed for landing");
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 打开根路径应立即进入看板，而不是中性首页。 */
  test("root_path_opens_dashboard_instead_of_home", async () => {
    const router = createRouter({
      history: createMemoryHistory({ initialEntries: ["/"] }),
      routeTree,
    });
    await router.load();
    render(
      <TestProviders>
        <RouterProvider router={router} />
      </TestProviders>,
    );

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/dashboard");
    });
    expect(await screen.findByTestId("dashboard-page")).toBeVisible();
    expect(screen.queryByTestId("home-page")).not.toBeInTheDocument();
    expect(await screen.findByTestId("navigation-label-dashboard")).toBeVisible();
  });

  /** 冷启动入口路径一律解析为看板，已有业务路径保持不变。 */
  test("cold_start_paths_resolve_to_dashboard", () => {
    expect(resolveDefaultLandingPath("/")).toBe("/dashboard");
    expect(resolveDefaultLandingPath("")).toBe("/dashboard");
    expect(resolveDefaultLandingPath("/index.html")).toBe("/dashboard");
    expect(resolveDefaultLandingPath("/monitor")).toBe("/monitor");
    expect(resolveDefaultLandingPath("/settings")).toBe("/settings");
    expect(resolveDefaultLandingPath("/dashboard/usage")).toBe("/dashboard/usage");
    expect(isDashboardLandingPath("/")).toBe(true);
    expect(isDashboardLandingPath("/index.html")).toBe(true);
    expect(isDashboardLandingPath("/dashboard/charts")).toBe(true);
    expect(isDashboardLandingPath("/monitor")).toBe(false);
  });
});
