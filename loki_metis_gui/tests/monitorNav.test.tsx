import { render, screen, waitFor } from "@testing-library/react";
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

import { MonitorLayout } from "../src/components/MonitorLayout";
import { MonitorManagementPage } from "../src/pages/MonitorManagementPage";
import { MonitorWorkbenchPage } from "../src/pages/MonitorWorkbenchPage";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 用内存路由挂载真实监控布局。 */
async function renderMonitor(initialPath = "/monitor") {
  const rootRoute = createRootRoute({
    component: () => <Outlet />,
  });
  const monitorRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/monitor",
    component: MonitorLayout,
  });
  const workbenchRoute = createRoute({
    getParentRoute: () => monitorRoute,
    path: "/",
    component: MonitorWorkbenchPage,
  });
  const managementRoute = createRoute({
    getParentRoute: () => monitorRoute,
    path: "/management",
    component: MonitorManagementPage,
  });
  const imagesRoute = createRoute({
    getParentRoute: () => monitorRoute,
    path: "/images",
    component: () => <div>images-body</div>,
  });
  const monitorSettingsRoute = createRoute({
    getParentRoute: () => monitorRoute,
    path: "/settings",
    component: () => <div>monitor-settings-body</div>,
  });
  const appSettingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings",
    component: () => <div>app-settings</div>,
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: [initialPath] }),
    routeTree: rootRoute.addChildren([
      monitorRoute.addChildren([
        workbenchRoute,
        managementRoute,
        imagesRoute,
        monitorSettingsRoute,
      ]),
      appSettingsRoute,
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

describe("monitor header tabs", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_hook_relay_status") {
        return {
          listening: true,
          bindAddress: "127.0.0.1:10240",
          receivedCount: 0,
          failedCount: 0,
          lastEvent: null,
          lastError: null,
        };
      }
      if (command === "get_monitor_capabilities") {
        return {
          aiTools: [
            { tool: "claudeCode", name: "Claude Code" },
            { tool: "codex", name: "Codex" },
            { tool: "grok", name: "Grok Build" },
            { tool: "workBuddy", name: "WorkBuddy" },
          ],
          hookBehaviors: ["idle", "running", "asking", "error"],
        };
      }
      if (command === "get_monitor_settings") {
        return {
          enabledAiTools: ["codex", "claudeCode", "grok", "workBuddy"],
          hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
        };
      }
      if (command === "list_monitor_hook_locations") {
        return [
          { tool: "codex", directory: "/tmp/codex", configPath: "/tmp/codex/hooks.json", isCustom: true },
        ];
      }
      if (command === "open_pet_overlay") {
        return { label: "pet", hideMainWindow: false, stopHookListener: false };
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 二级选项卡顺序为工作台、监控管理、图片管理、设置，且监控设置不是应用设置。 */
  test("monitor_tabs_follow_workbench_management_images_settings_order", async () => {
    const router = await renderMonitor();
    const workbench = screen.getByRole("link", { name: /Workbench:/ });
    const management = screen.getByRole("link", { name: /Monitor management:/ });
    const images = screen.getByRole("link", { name: /Image management:/ });
    const settings = screen.getByRole("link", { name: /Settings:/ });
    expect(workbench).toBeVisible();
    expect(management).toBeVisible();
    expect(images).toBeVisible();
    expect(settings).toBeVisible();
    expect(workbench.compareDocumentPosition(management) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(management.compareDocumentPosition(images) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    expect(images.compareDocumentPosition(settings) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
    await userEvent.click(settings);
    expect(router.state.location.pathname).toBe("/monitor/settings");
    expect(screen.getByText("monitor-settings-body")).toBeVisible();
    expect(screen.queryByText("app-settings")).not.toBeInTheDocument();
  });

  /** 工作台挂载真实中继查询；监控管理只出现四项 Agent。 */
  test("workbench_and_management_use_four_approved_agents_and_relay_query", async () => {
    const router = await renderMonitor();
    expect(await screen.findByText(/Listening on 127.0.0.1:10240/)).toBeVisible();
    expect(invokeMock).toHaveBeenCalledWith("get_hook_relay_status");
    await userEvent.click(screen.getByRole("link", { name: /Monitor management:/ }));
    expect(router.state.location.pathname).toBe("/monitor/management");
    expect(await screen.findByText("Codex")).toBeVisible();
    expect(screen.getByText("Claude Code")).toBeVisible();
    expect(screen.getByText("Grok Build")).toBeVisible();
    expect(screen.getByText("WorkBuddy")).toBeVisible();
    expect(screen.queryByText(/Cursor/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/OpenCode/i)).not.toBeInTheDocument();
  });

  /** 监控区用真实按钮打开桌宠悬浮窗，不是侧栏新项。 */
  test("monitor_toolbar_opens_desktop_pet_overlay_via_command", async () => {
    await renderMonitor();
    const open = screen.getByTestId("open-pet-overlay");
    expect(open).toBeVisible();
    expect(open).toHaveTextContent(/Desktop pet overlay/i);
    await userEvent.click(open);
    expect(invokeMock).toHaveBeenCalledWith("open_pet_overlay");
    expect(screen.queryByRole("button", { name: "Monitor" })).not.toBeInTheDocument();
  });
});
