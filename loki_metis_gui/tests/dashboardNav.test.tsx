import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  Outlet,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import type {
  AgentClientKind,
  AvailableAiTypeDto,
  UsageViewKind,
} from "../src/api/usage-types";
import { DashboardLayout } from "../src/components/DashboardLayout";
import { DashboardToolbar } from "../src/components/DashboardToolbar";
import { CallsPage } from "../src/pages/CallsPage";
import { ChartsPage } from "../src/pages/ChartsPage";
import { DashboardSettingsSection } from "../src/pages/DashboardSettingsSection";
import { UsagePage } from "../src/pages/UsagePage";
import { WorkbuddyUsage } from "../src/pages/WorkbuddyUsage";
import { availableDashboardAiTypesFixture, TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 构造看板配置 IPC 快照，含设备字段但配置面不得展示它们。 */
function privacySettings() {
  return {
    languagePreference: "system",
    localOnly: true,
    deviceUsername: "alice",
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
    enabledAgents: ["codex", "claudeCode", "grokBuildCli"],
    workbuddyStatsEnabled: true,
  };
}

/** 用内存路由渲染真实看板页头，供横向子页断言。 */
async function renderToolbar(
  view: UsageViewKind,
  workbuddyStatsEnabled = false,
  availableAiTypes: AvailableAiTypeDto[] = availableDashboardAiTypesFixture,
  enabledAgents: AgentClientKind[] = ["codex", "claudeCode", "grokBuildCli"],
) {
  const rootRoute = createRootRoute({
    component: () => (
      <DashboardToolbar
        availableAiTypes={availableAiTypes}
        enabledAgents={enabledAgents}
        onViewChange={vi.fn()}
        view={view}
        workbuddyStatsEnabled={workbuddyStatsEnabled}
      />
    ),
  });
  const placeholderRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => null,
  });
  const dashboardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard",
    component: () => null,
  });
  const usageRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard/usage",
    component: () => null,
  });
  const sourcesRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard/sources",
    component: () => null,
  });
  const callsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard/calls",
    component: () => null,
  });
  const dashboardSettingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard/settings",
    component: () => null,
  });
  const chartsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard/charts",
    component: () => null,
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute.addChildren([
      placeholderRoute,
      dashboardRoute,
      usageRoute,
      sourcesRoute,
      callsRoute,
      dashboardSettingsRoute,
      chartsRoute,
    ]),
  });
  await router.load();
  render(
    <TestProviders>
      <RouterProvider router={router} />
    </TestProviders>,
  );
  await waitFor(() => {
    expect(screen.getByRole("navigation")).toBeVisible();
  });
}

/** 断言页面导航与 Agent 切换器在同一行容器内，导航在左、切换器在右。 */
function expectPageNavAndAgentSwitcherOnTheSameRow() {
  const nav = screen.getByRole("navigation", { name: "Dashboard pages" });
  const switcher = screen.getByRole("radiogroup", {
    name: "Agent client currently being viewed",
  });
  const row = screen.getByTestId("dashboard-header-row");
  expect(row).toContainElement(nav);
  expect(row).toContainElement(switcher);
  expect(nav.compareDocumentPosition(switcher) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
  expect(nav.parentElement).toBe(row);
  const progress = document.querySelector(".local-scan-progress");
  if (progress) {
    expect(row.contains(progress)).toBe(false);
  }
}

/** 用内存路由挂载真实看板布局与配置子页，并保留侧栏设置路由以便对照。 */
async function renderDashboard(initialPath = "/dashboard") {
  const rootRoute = createRootRoute({
    component: () => <Outlet />,
  });
  const dashboardRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard",
    component: DashboardLayout,
  });
  const dashboardIndexRoute = createRoute({
    getParentRoute: () => dashboardRoute,
    path: "/",
    component: () => <div>overview-body</div>,
  });
  const dashboardSettingsRoute = createRoute({
    getParentRoute: () => dashboardRoute,
    path: "/settings",
    component: DashboardSettingsSection,
  });
  const usageRoute = createRoute({
    getParentRoute: () => dashboardRoute,
    path: "/usage",
    component: () => null,
  });
  const sourcesRoute = createRoute({
    getParentRoute: () => dashboardRoute,
    path: "/sources",
    component: () => null,
  });
  const callsRoute = createRoute({
    getParentRoute: () => dashboardRoute,
    path: "/calls",
    component: () => null,
  });
  const chartsRoute = createRoute({
    getParentRoute: () => dashboardRoute,
    path: "/charts",
    component: ChartsPage,
  });
  const sidebarSettingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings",
    component: () => <div>sidebar-settings</div>,
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: [initialPath] }),
    routeTree: rootRoute.addChildren([
      dashboardRoute.addChildren([
        dashboardIndexRoute,
        dashboardSettingsRoute,
        usageRoute,
        sourcesRoute,
        callsRoute,
        chartsRoute,
      ]),
      sidebarSettingsRoute,
    ]),
  });
  await router.load();
  render(
    <TestProviders>
      <RouterProvider router={router} />
    </TestProviders>,
  );
  await waitFor(() => {
    expect(screen.getByRole("navigation")).toBeVisible();
  });
  return router;
}

describe("dashboard header subpages", () => {
  beforeEach(() => {
    invokeMock.mockRejectedValue(new Error("ipc unavailable"));
  });

  /** 物理 Agent 视图选项卡顺序为概览、用量、图表、数据源、看板设置。 */
  test("physical_agent_view_shows_overview_usage_and_sources", async () => {
    await renderToolbar("codex");
    const overview = screen.getByRole("link", { name: /Overview:/ });
    const usage = screen.getByRole("link", { name: /Usage:/ });
    const charts = screen.getByRole("link", { name: /Charts:/ });
    const sources = screen.getByRole("link", { name: /Data sources:/ });
    const settings = screen.getByRole("link", { name: /Dashboard settings:/ });
    expect(overview).toBeVisible();
    expect(usage).toBeVisible();
    expect(charts).toBeVisible();
    expect(sources).toBeVisible();
    expect(settings).toBeVisible();
    expect(screen.getByText("Dashboard settings")).toBeVisible();
    expect(overview.compareDocumentPosition(usage) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(usage.compareDocumentPosition(charts) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(charts.compareDocumentPosition(sources) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(sources.compareDocumentPosition(settings) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(screen.queryByRole("button", { name: "Dashboard settings" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Dashboard settings" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Calls:/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/Time zone:/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("radiogroup", { name: "Time standard" })).not.toBeInTheDocument();
    expect(screen.queryByText("Local time")).not.toBeInTheDocument();
    expect(screen.queryByText("UTC time")).not.toBeInTheDocument();
    expectPageNavAndAgentSwitcherOnTheSameRow();
  });

  /** 「全部」出现概览、调用，看板设置紧挨最后一项，且不出现用量/数据源。 */
  test("all_view_shows_overview_and_calls_without_usage_or_sources", async () => {
    await renderToolbar("all");
    const overview = screen.getByRole("link", { name: /Overview:/ });
    const calls = screen.getByRole("link", { name: /Calls:/ });
    const settings = screen.getByRole("link", { name: /Dashboard settings:/ });
    expect(overview).toBeVisible();
    expect(calls).toBeVisible();
    expect(settings).toBeVisible();
    expect(screen.getByText("Dashboard settings")).toBeVisible();
    expect(calls.compareDocumentPosition(settings) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(screen.queryByRole("link", { name: /Usage:/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Charts:/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Data sources:/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Dashboard settings" })).not.toBeInTheDocument();
    expectPageNavAndAgentSwitcherOnTheSameRow();
  });

  /** WorkBuddy 开启时出现概览、用量、图表、数据源、看板设置且不出现调用。 */
  test("workbuddy_view_shows_overview_usage_and_sources_without_calls", async () => {
    await renderToolbar("workbuddy", true);
    const overview = screen.getByRole("link", { name: /Overview:/ });
    const usage = screen.getByRole("link", { name: /Usage:/ });
    const charts = screen.getByRole("link", { name: /Charts:/ });
    const sources = screen.getByRole("link", { name: /Data sources:/ });
    const settings = screen.getByRole("link", { name: /Dashboard settings:/ });
    expect(overview).toBeVisible();
    expect(usage).toBeVisible();
    expect(charts).toBeVisible();
    expect(sources).toBeVisible();
    expect(settings).toBeVisible();
    expect(screen.getByText("Dashboard settings")).toBeVisible();
    expect(usage.compareDocumentPosition(charts) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(charts.compareDocumentPosition(sources) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(sources.compareDocumentPosition(settings) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(screen.queryByRole("link", { name: /Calls:/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Dashboard settings" })).not.toBeInTheDocument();
    expectPageNavAndAgentSwitcherOnTheSameRow();
  });

  /** Agent 切换器仅展示统一目录与用户启用集合都包含的看板映射。 */
  test("agent_switcher_intersects_available_and_enabled_dashboard_agents", async () => {
    await renderToolbar(
      "all",
      true,
      [
        { name: "Claude", value: "claudeCode" },
        { name: "WorkBuddy", value: "workbuddy" },
      ],
      ["codex", "claudeCode", "grokBuildCli"],
    );

    const switcher = screen.getByRole("radiogroup", {
      name: "Agent client currently being viewed",
    });
    expect(within(switcher).getByText("All")).toBeVisible();
    expect(within(switcher).getByText("Claude")).toBeVisible();
    expect(within(switcher).getByText("WorkBuddy")).toBeVisible();
    expect(within(switcher).queryByText("Codex")).not.toBeInTheDocument();
    expect(within(switcher).queryByText("Grok")).not.toBeInTheDocument();
  });

  /** 真实加载用量、调用与 WorkBuddy 用量页面模块，证明已发布入口可解析。 */
  test("usage_calls_and_workbuddy_pages_resolve_shipped_modules", async () => {
    invokeMock.mockRejectedValue(new Error("ipc unavailable"));
    const pages = [
      { name: "usage", node: <UsagePage /> },
      { name: "calls", node: <CallsPage /> },
      { name: "workbuddy-usage", node: <WorkbuddyUsage /> },
    ];
    for (const page of pages) {
      const rendered = render(<TestProviders>{page.node}</TestProviders>);
      expect(
        await screen.findByRole("alert"),
        `${page.name} must mount the shipped page instead of failing at import`,
      ).toBeVisible();
      rendered.unmount();
    }
  });
});

describe("dashboard header settings surface", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_privacy_settings") {
        return privacySettings();
      }
      if (command === "get_local_scan_status") {
        return { state: "idle" };
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 点击页头看板设置后，页头下方切到配置面且不进入侧栏设置。 */
  test("dashboard_settings_link_replaces_body_without_leaving_dashboard", async () => {
    const router = await renderDashboard();
    expect(screen.getByRole("link", { name: /Dashboard settings:/ })).toBeVisible();
    expect(screen.getByText("overview-body")).toBeVisible();
    expect(screen.queryByTestId("dashboard-settings")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("link", { name: /Dashboard settings:/ }));

    expect(await screen.findByTestId("dashboard-settings")).toBeVisible();
    const agentOptions = screen.getByTestId("dashboard-enabled-agent-options");
    expect(agentOptions).toHaveAccessibleName("AI agents to monitor and report");
    expect(within(agentOptions).getByRole("checkbox", { name: "Codex" })).toBeVisible();
    expect(within(agentOptions).getByRole("checkbox", { name: "Claude Code" })).toBeVisible();
    expect(within(agentOptions).getByRole("checkbox", { name: "Grok" })).toBeVisible();
    expect(within(agentOptions).getByRole("checkbox", { name: "WorkBuddy" })).toBeVisible();
    expect(within(agentOptions).queryByRole("checkbox", { name: "Cursor" })).not.toBeInTheDocument();
    expect(
      screen.queryByText(/Codex is selected by default in first-time setup/),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/When enabled, this parses top-level and subagent/),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Scan interval")).toBeVisible();
    expect(screen.getByText("Automatic cleanup")).toBeVisible();
    expect(screen.queryByText("Device identity")).not.toBeInTheDocument();
    expect(screen.queryByText("Device username")).not.toBeInTheDocument();
    expect(screen.queryByText("Device name")).not.toBeInTheDocument();
    expect(screen.queryByText("Unique device ID")).not.toBeInTheDocument();
    expect(screen.queryByText("sidebar-settings")).not.toBeInTheDocument();
    expect(screen.getByRole("navigation")).toBeVisible();
    expect(router.state.location.pathname).toBe("/dashboard/settings");
  });

  /** 点击图表选项卡后，同一页同时出现趋势图与用量分布，且仍在看板路由下。 */
  test("charts_tab_opens_merged_trend_and_distribution_page", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_privacy_settings") {
        return privacySettings();
      }
      if (command === "get_local_scan_status") {
        return { state: "idle" };
      }
      if (command === "get_usage_charts") {
        return {
          window: "today",
          dimension: "model",
          granularity: "hour",
          indexState: "ready",
          lowerBoundEpochMs: 0,
          observedAtEpochMs: 0,
          fact: {
            completeness: "complete",
            confidence: "exact",
            freshness: "fresh",
            observedAtEpochMs: 0,
            provider: "rolloutJsonl",
            scope: "deviceObserved",
            sourceVersion: null,
            value: {
              cacheReadBasisPoints: null,
              cachedReadCallCount: 0,
              callCount: 1,
              confidence: "exact",
              crossRootDuplicateSourceCount: 0,
              duplicateSourceCount: 0,
              rootCount: 1,
              sourceCount: 1,
              threadCount: 1,
              tokens: {
                cacheWriteInputTokens: null,
                cachedInputTokens: null,
                inputTokens: 10,
                outputTokens: 5,
                reasoningOutputTokens: null,
                totalIsDerived: false,
                totalTokens: 15,
              },
            },
          },
          buckets: [
            {
              key: "2026-09-05T00",
              label: "00:00",
              inProgress: false,
              measure: {
                cacheReadBasisPoints: null,
                cachedReadCallCount: 0,
                callCount: 1,
                confidence: "exact",
                duplicateSourceCount: 0,
                tokens: {
                  cacheWriteInputTokens: null,
                  cachedInputTokens: null,
                  inputTokens: 10,
                  outputTokens: 5,
                  reasoningOutputTokens: null,
                  totalIsDerived: false,
                  totalTokens: 15,
                },
              },
            },
          ],
          groups: [
            {
              id: "model-gpt",
              label: "gpt",
              labelCode: "literal",
              disambiguationIndex: null,
              remainder: false,
              totalTokenShareBasisPoints: 10000,
              measure: {
                cacheReadBasisPoints: null,
                cachedReadCallCount: 0,
                callCount: 1,
                confidence: "exact",
                duplicateSourceCount: 0,
                tokens: {
                  cacheWriteInputTokens: null,
                  cachedInputTokens: null,
                  inputTokens: 10,
                  outputTokens: 5,
                  reasoningOutputTokens: null,
                  totalIsDerived: false,
                  totalTokens: 15,
                },
              },
            },
          ],
          remainder: null,
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    const router = await renderDashboard();
    await userEvent.click(screen.getByRole("link", { name: /Charts:/ }));
    expect(
      await screen.findByRole("img", { name: "Token trends across complete time buckets" }),
    ).toBeVisible();
    expect(
      screen.getByRole("img", { name: "Call-count trend on a separate vertical axis" }),
    ).toBeVisible();
    expect(screen.getByRole("img", { name: "Usage distribution bar chart" })).toBeVisible();
    expect(screen.getByText("Token trends")).toBeVisible();
    expect(screen.getByText("Call trend")).toBeVisible();
    expect(screen.queryByRole("link", { name: /Charts:/ })).toBeVisible();
    expect(screen.queryByRole("navigation", { name: "Chart pages" })).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/dashboard/charts");
    expect(router.state.location.pathname).not.toBe("/settings");
    expect(screen.queryByText("sidebar-settings")).not.toBeInTheDocument();
  });
});
