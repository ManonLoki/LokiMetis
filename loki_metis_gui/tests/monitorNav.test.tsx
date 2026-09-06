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

import { MonitorLayout } from "../src/components/MonitorLayout";
import { MonitorImagesPage } from "../src/pages/MonitorImagesPage";
import { MonitorManagementPage } from "../src/pages/MonitorManagementPage";
import { MonitorSettingsPage } from "../src/pages/MonitorSettingsPage";
import { MonitorWorkbenchPage } from "../src/pages/MonitorWorkbenchPage";
import { monitorCapabilitiesFixture, TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 四项已批准 Agent 的空白展示草稿。 */
function emptyDrafts() {
  const behaviors = ["idle", "running", "asking", "error"] as const;
  return {
    drafts: (["codex", "claudeCode", "grok", "workBuddy"] as const).map((tool) => ({
      tool,
      slot: 1,
      hooks: behaviors.map((behavior) => ({ behavior, content: "", image: "" })),
    })),
  };
}

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
    component: MonitorImagesPage,
  });
  const monitorSettingsRoute = createRoute({
    getParentRoute: () => monitorRoute,
    path: "/settings",
    component: MonitorSettingsPage,
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
        return monitorCapabilitiesFixture({
          aiTools: [
            { tool: "claudeCode", name: "Claude Code" },
            { tool: "codex", name: "Codex" },
            { tool: "grok", name: "Grok Build" },
            { tool: "workBuddy", name: "WorkBuddy" },
          ],
        });
      }
      if (command === "get_monitor_settings") {
        return {
          enabledAiTools: ["codex", "claudeCode", "grok", "workBuddy"],
          hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
        };
      }
      if (command === "list_monitor_hook_locations") {
        return [
          {
            tool: "codex",
            directory: "/tmp/codex",
            configPath: "/tmp/codex/hooks.json",
            isCustom: true,
          },
        ];
      }
      if (command === "list_monitor_images_cmd") {
        return { images: [], counts: { jpeg: 0, png: 0, gif: 0 } };
      }
      if (command === "list_monitor_profile_drafts") {
        return emptyDrafts();
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 二级选项卡只保留工作台、监控管理、图片管理、Hooks 设置，且不是应用设置。 */
  test("monitor_tabs_follow_workbench_management_images_settings_order", async () => {
    const router = await renderMonitor();
    const workbench = screen.getByRole("link", { name: /Workbench:/ });
    const management = screen.getByRole("link", { name: /Monitor management:/ });
    const images = screen.getByRole("link", { name: /Image management:/ });
    const hooks = screen.getByRole("link", { name: /Hooks settings:/ });
    expect(workbench).toBeVisible();
    expect(management).toBeVisible();
    expect(images).toBeVisible();
    expect(hooks).toBeVisible();
    expect(
      within(screen.getByRole("navigation", { name: "Monitor pages" })).getAllByRole(
        "link",
      ),
    ).toHaveLength(4);
    expect(
      screen.queryByRole("link", { name: /Monitor settings:/ }),
    ).not.toBeInTheDocument();
    expect(
      workbench.compareDocumentPosition(management) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      management.compareDocumentPosition(images) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      images.compareDocumentPosition(hooks) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(screen.queryByRole("link", { name: /^Settings:/ })).not.toBeInTheDocument();
    await userEvent.click(hooks);
    expect(router.state.location.pathname).toBe("/monitor/settings");
    expect(router.state.location.pathname).not.toBe("/settings");
    expect(await screen.findByTestId("monitor-settings")).toBeVisible();
    expect(screen.queryByText("app-settings")).not.toBeInTheDocument();
    const enabledAgents = screen.getByTestId("monitor-enabled-agents");
    const hooksManagement = screen.getByTestId("monitor-hooks-management");
    expect(
      within(enabledAgents).getByRole("heading", { name: "Hooks settings" }),
    ).toBeVisible();
    expect(within(enabledAgents).getByRole("checkbox", { name: "Codex" })).toBeVisible();
    expect(
      within(hooksManagement).getByRole("heading", { name: "Hooks management" }),
    ).toBeVisible();
    expect(within(hooksManagement).getByRole("tab", { name: "Codex" })).toBeVisible();
    expect(
      within(hooksManagement).getByRole("button", { name: "Write Hooks" }),
    ).toBeVisible();
  });

  /** 工作台挂载真实中继查询；监控管理按已启用 Agent 分 Tab，不再以写入 Hooks 为主。 */
  test("workbench_and_management_use_four_approved_agents_and_relay_query", async () => {
    const router = await renderMonitor();
    expect(await screen.findByText(/Listening on 127.0.0.1:10240/)).toBeVisible();
    expect(screen.getByText("Received events")).toBeVisible();
    expect(screen.getByText("Failed events")).toBeVisible();
    expect(screen.getAllByText("0")).toHaveLength(2);
    expect(invokeMock).toHaveBeenCalledWith("get_hook_relay_status");
    await userEvent.click(screen.getByRole("link", { name: /Monitor management:/ }));
    expect(router.state.location.pathname).toBe("/monitor/management");
    expect(await screen.findByRole("tab", { name: "Codex" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Claude Code" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Grok Build" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "WorkBuddy" })).toBeVisible();
    expect(screen.getByText("Display position")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Write Hooks" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Cursor/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/OpenCode/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/LAN/i)).not.toBeInTheDocument();
  });

  /** Hooks 选项卡可启用 Agent 并写入/定位 Hook 配置。 */
  test("hooks_settings_tab_enables_agents_and_writes_local_hook_config", async () => {
    invokeMock.mockImplementation(
      async (command: string, payload?: { tools?: string[]; tool?: string }) => {
        if (command === "get_monitor_capabilities") {
          return monitorCapabilitiesFixture({
            aiTools: [
              { tool: "codex", name: "Codex" },
              { tool: "claudeCode", name: "Claude Code" },
              { tool: "grok", name: "Grok Build" },
              { tool: "workBuddy", name: "WorkBuddy" },
            ],
            imageUploadAccept: { mimeTypes: ["image/png"], extensions: [".png"] },
          });
        }
        if (command === "get_monitor_settings") {
          return {
            enabledAiTools: ["codex"],
            hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
          };
        }
        if (command === "list_monitor_hook_locations") {
          return [
            {
              tool: "codex",
              directory: "/tmp/codex",
              configPath: "/tmp/codex/hooks.json",
              isCustom: true,
            },
          ];
        }
        if (command === "save_monitor_enabled_tools") {
          return {
            enabledAiTools: payload?.tools ?? ["codex", "grok"],
            hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
          };
        }
        if (command === "write_monitor_hook_config") {
          return {
            tool: payload?.tool ?? "codex",
            filename: "hooks.json",
            outcome: "active",
            configChanged: true,
            requiresReview: false,
            restartRequired: false,
          };
        }
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
        throw new Error(`unexpected command ${command}`);
      },
    );
    const router = await renderMonitor("/monitor/settings");
    expect(router.state.location.pathname).toBe("/monitor/settings");
    const enabledAgents = await screen.findByTestId("monitor-enabled-agents");
    const hooksManagement = screen.getByTestId("monitor-hooks-management");
    const codexTab = within(hooksManagement).getByRole("tab", { name: "Codex" });
    expect(codexTab).toHaveAttribute("aria-selected", "true");
    expect(
      within(hooksManagement).getByRole("textbox", { name: "Config directory" }),
    ).toHaveValue("/tmp/codex");
    expect(
      within(hooksManagement).getByRole("textbox", { name: "Configuration file" }),
    ).toHaveValue("/tmp/codex/hooks.json");
    expect(
      within(hooksManagement).getByRole("textbox", { name: "Config directory" }),
    ).toHaveAttribute("readonly");
    expect(
      within(hooksManagement).getByRole("textbox", { name: "Configuration file" }),
    ).toHaveAttribute("readonly");
    await userEvent.click(
      within(hooksManagement).getByRole("button", { name: "Write Hooks" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("write_monitor_hook_config", { tool: "codex" });
    expect(await within(hooksManagement).findByText(/Wrote hooks.json/)).toBeVisible();
    await userEvent.click(
      within(enabledAgents).getByRole("checkbox", { name: "Grok Build" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("save_monitor_enabled_tools", {
      tools: expect.arrayContaining(["codex", "grok"]),
    });
    const grokTab = await within(hooksManagement).findByRole("tab", { name: "Grok Build" });
    await userEvent.click(grokTab);
    expect(grokTab).toHaveAttribute("aria-selected", "true");
    expect(codexTab).toHaveAttribute("aria-selected", "false");
    expect(within(hooksManagement).queryByText(/Wrote hooks.json/)).not.toBeInTheDocument();
    await userEvent.click(codexTab);
    expect(codexTab).toHaveAttribute("aria-selected", "true");
    expect(await within(hooksManagement).findByText(/Wrote hooks.json/)).toBeVisible();
    expect(screen.queryByText("app-settings")).not.toBeInTheDocument();
  });
});
