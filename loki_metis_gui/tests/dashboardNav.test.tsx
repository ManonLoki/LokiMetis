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
  PrivacySettingsDto,
  UsageViewKind,
} from "../src/api/usage-types";
import { DashboardLayout } from "../src/components/DashboardLayout";
import { DashboardToolbar } from "../src/components/DashboardToolbar";
import { CallsPage } from "../src/pages/CallsPage";
import { ChartsPage } from "../src/pages/ChartsPage";
import { UsagePage } from "../src/pages/UsagePage";
import { UsageSettingsPage } from "../src/pages/UsageSettingsPage";
import { WorkbuddyUsage } from "../src/pages/WorkbuddyUsage";
import { availableDashboardAiTypesFixture, TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 构造看板配置 IPC 快照。 */
function privacySettings(): PrivacySettingsDto {
  return {
    languagePreference: "system",
    scanIntervalMinutes: 5,
    retentionDays: 90,
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
  const settingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard/settings",
    component: () => null,
  });
  const callsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/dashboard/calls",
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
      settingsRoute,
      callsRoute,
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
  expect(nav.compareDocumentPosition(switcher) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(
    0,
  );
  expect(nav.parentElement).toBe(row);
  const progress = document.querySelector(".local-scan-progress");
  if (progress) {
    expect(row.contains(progress)).toBe(false);
  }
}

/** 用内存路由挂载真实看板布局，并保留侧栏公共设置路由以便对照。 */
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
  const settingsRoute = createRoute({
    getParentRoute: () => dashboardRoute,
    path: "/settings",
    component: UsageSettingsPage,
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
        usageRoute,
        sourcesRoute,
        settingsRoute,
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

  /** 物理 Agent 视图在数据源后紧接用量设置。 */
  test("physical_agent_view_shows_settings_after_sources", async () => {
    await renderToolbar("codex");
    const overview = screen.getByRole("link", { name: /Overview:/ });
    const usage = screen.getByRole("link", { name: /Usage:/ });
    const charts = screen.getByRole("link", { name: /Charts:/ });
    const sources = screen.getByRole("link", { name: /Data sources:/ });
    const settings = screen.getByRole("link", { name: /Settings:/ });
    expect(overview).toBeVisible();
    expect(usage).toBeVisible();
    expect(charts).toBeVisible();
    expect(sources).toBeVisible();
    expect(settings).toBeVisible();
    expect(
      overview.compareDocumentPosition(usage) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      usage.compareDocumentPosition(charts) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      charts.compareDocumentPosition(sources) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      sources.compareDocumentPosition(settings) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(screen.queryByRole("link", { name: /Calls:/ })).not.toBeInTheDocument();
    expect(screen.queryByText(/Time zone:/i)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("radiogroup", { name: "Time standard" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Local time")).not.toBeInTheDocument();
    expect(screen.queryByText("UTC time")).not.toBeInTheDocument();
    expectPageNavAndAgentSwitcherOnTheSameRow();
  });

  /** 「全部」只出现概览、调用，且不出现用量、数据源或设置。 */
  test("all_view_shows_overview_and_calls_without_usage_or_sources", async () => {
    await renderToolbar("all");
    const overview = screen.getByRole("link", { name: /Overview:/ });
    const calls = screen.getByRole("link", { name: /Calls:/ });
    expect(overview).toBeVisible();
    expect(calls).toBeVisible();
    expect(screen.queryByRole("link", { name: /Usage:/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Charts:/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Data sources:/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /Settings:/ })).not.toBeInTheDocument();
    expectPageNavAndAgentSwitcherOnTheSameRow();
  });

  /** WorkBuddy 开启时同样在数据源后显示设置，且不出现调用。 */
  test("workbuddy_view_shows_settings_after_sources_without_calls", async () => {
    await renderToolbar("workbuddy", true);
    const overview = screen.getByRole("link", { name: /Overview:/ });
    const usage = screen.getByRole("link", { name: /Usage:/ });
    const charts = screen.getByRole("link", { name: /Charts:/ });
    const sources = screen.getByRole("link", { name: /Data sources:/ });
    const settings = screen.getByRole("link", { name: /Settings:/ });
    expect(overview).toBeVisible();
    expect(usage).toBeVisible();
    expect(charts).toBeVisible();
    expect(sources).toBeVisible();
    expect(settings).toBeVisible();
    expect(
      usage.compareDocumentPosition(charts) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      charts.compareDocumentPosition(sources) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      sources.compareDocumentPosition(settings) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(screen.queryByRole("link", { name: /Calls:/ })).not.toBeInTheDocument();
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

describe("dashboard content surface", () => {
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
    expect(
      screen.queryByRole("navigation", { name: "Chart pages" }),
    ).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/dashboard/charts");
    expect(router.state.location.pathname).not.toBe("/settings");
    expect(screen.queryByText("sidebar-settings")).not.toBeInTheDocument();
  });

  /** 设置选项卡打开看板子路由，只展示扫描间隔与自动清理。 */
  test("settings_tab_opens_usage_settings_in_dashboard", async () => {
    const router = await renderDashboard();

    await userEvent.click(screen.getByRole("link", { name: /Settings:/ }));

    const settings = await screen.findByTestId("dashboard-settings");
    expect(within(settings).getByRole("heading", { name: "Usage settings" })).toBeVisible();
    expect(within(settings).getByText("Scan interval")).toBeVisible();
    expect(within(settings).getByText("Automatic cleanup")).toBeVisible();
    expect(router.state.location.pathname).toBe("/dashboard/settings");
    expect(screen.queryByText("sidebar-settings")).not.toBeInTheDocument();
  });

  /** 从用量设置切换到「全部」时回到概览，不暴露不适用子页。 */
  test("switching_to_all_leaves_usage_settings", async () => {
    const router = await renderDashboard("/dashboard/settings");
    expect(await screen.findByTestId("dashboard-settings")).toBeVisible();

    await userEvent.click(screen.getByText("All"));

    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/dashboard");
    });
    expect(screen.queryByTestId("dashboard-settings")).not.toBeInTheDocument();
  });
});
