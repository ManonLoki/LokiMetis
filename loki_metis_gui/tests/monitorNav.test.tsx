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
import {
  allMonitorAiToolsFixture,
  monitorCapabilitiesFixture,
  TestProviders,
} from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 统一公开目录五项展示草稿夹具。 */
function emptyDrafts() {
  const behaviors = ["idle", "running", "asking", "error"] as const;
  return {
    drafts: (["codex", "claudeCode", "cursor", "grok", "workBuddy"] as const).map(
      (tool) => ({
        tool,
        slot: 1,
        hooks: behaviors.map((behavior) => ({ behavior, content: "", image: "" })),
      }),
    ),
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
  const appSettingsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/settings",
    component: MonitorSettingsPage,
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: [initialPath] }),
    routeTree: rootRoute.addChildren([
      monitorRoute.addChildren([
        workbenchRoute,
        managementRoute,
        imagesRoute,
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
    if (initialPath.startsWith("/monitor")) {
      expect(screen.getByRole("navigation")).toBeVisible();
    } else {
      expect(screen.getByTestId("monitor-settings")).toBeVisible();
    }
  });
  return router;
}

describe("monitor header tabs", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_hook_relay_status") {
        return {
          listening: true,
          bindAddress: "127.0.0.1:23456",
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

  /** 二级选项卡只保留工作台、监控管理与图片管理，不再复制 Hooks 设置入口。 */
  test("monitor_tabs_follow_workbench_management_images_settings_order", async () => {
    await renderMonitor();
    const workbench = screen.getByRole("link", { name: /Workbench:/ });
    const management = screen.getByRole("link", { name: /Monitor management:/ });
    const images = screen.getByRole("link", { name: /Image management:/ });
    expect(workbench).toBeVisible();
    expect(management).toBeVisible();
    expect(images).toBeVisible();
    expect(
      within(screen.getByRole("navigation", { name: "Monitor pages" })).getAllByRole(
        "link",
      ),
    ).toHaveLength(3);
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
      screen.queryByRole("link", { name: /Hooks settings:/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /^Settings:/ })).not.toBeInTheDocument();
  });

  /** 工作台挂载真实中继查询；监控管理按已启用 Agent 分 Tab，不再以写入 Hooks 为主。 */
  test("workbench_and_management_use_enabled_agents_and_relay_query", async () => {
    const router = await renderMonitor();
    expect(await screen.findByText(/Listening on 127.0.0.1:23456/)).toBeVisible();
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

  /** 工作台最近事件无法在当前后端能力目录匹配时不得显示原始工具值。 */
  test("workbench_ignores_last_event_outside_the_available_catalog", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_monitor_capabilities") {
        return monitorCapabilitiesFixture({
          aiTools: [{ tool: "codex", name: "Codex" }],
        });
      }
      if (command === "get_hook_relay_status") {
        return {
          listening: true,
          bindAddress: "127.0.0.1:23456",
          receivedCount: 1,
          failedCount: 0,
          lastEvent: { tool: "openCode", hookType: "session.idle" },
          lastError: null,
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    await renderMonitor();

    expect(await screen.findByText("No Hook events yet")).toBeVisible();
    expect(screen.queryByText("openCode")).not.toBeInTheDocument();
  });

  /** Hooks 设置只使用后端公开目录，并在保存时丢弃目录外的历史启用项。 */
  test("hooks_settings_intersect_capabilities_and_historical_enabled_tools", async () => {
    invokeMock.mockImplementation(async (command, payload) => {
      const typedPayload = payload as { tools?: string[] } | undefined;
      if (command === "get_monitor_capabilities") {
        return monitorCapabilitiesFixture();
      }
      if (command === "get_monitor_settings") {
        return {
          enabledAiTools: allMonitorAiToolsFixture.map(({ tool }) => tool),
          hookDirectories: {},
        };
      }
      if (command === "list_monitor_hook_locations") {
        return allMonitorAiToolsFixture.map(({ tool }) => ({
          tool,
          directory: `/tmp/${tool}`,
          configPath: `/tmp/${tool}/hooks.json`,
          isCustom: true,
        }));
      }
      if (command === "get_hook_relay_status") {
        return {
          listening: true,
          bindAddress: "127.0.0.1:23456",
          receivedCount: 0,
          failedCount: 0,
          lastEvent: null,
          lastError: null,
        };
      }
      if (command === "save_monitor_enabled_tools") {
        return {
          enabledAiTools: typedPayload?.tools ?? [],
          hookDirectories: {},
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    await renderMonitor("/settings");
    const enabledAgents = await screen.findByTestId("monitor-enabled-agents");
    const hooksManagement = screen.getByTestId("monitor-hooks-management");
    expect(within(enabledAgents).getAllByRole("checkbox")).toHaveLength(5);
    expect(within(hooksManagement).getAllByRole("tab")).toHaveLength(5);
    expect(within(hooksManagement).getByRole("tab", { name: "Cursor" })).toBeVisible();
    expect(within(enabledAgents).queryByText("GitHub Copilot")).not.toBeInTheDocument();
    await userEvent.click(within(enabledAgents).getByRole("checkbox", { name: "Cursor" }));
    expect(invokeMock).toHaveBeenCalledWith("save_monitor_enabled_tools", {
      tools: ["codex", "claudeCode", "grok", "workBuddy"],
    });
  });

  /** Hermes 与 OpenClaw 写入后展示可复制的真实激活命令，且 OpenClaw 保持执行顺序。 */
  test("hooks_settings_show_copyable_plugin_activation_commands", async () => {
    invokeMock.mockImplementation(async (command, payload) => {
      const tool = (payload as { tool?: "hermes" | "openClaw" } | undefined)?.tool;
      if (command === "get_monitor_capabilities") {
        return monitorCapabilitiesFixture({
          aiTools: [
            { tool: "hermes", name: "Hermes" },
            { tool: "openClaw", name: "OpenClaw" },
          ],
        });
      }
      if (command === "get_monitor_settings") {
        return { enabledAiTools: ["hermes", "openClaw"], hookDirectories: {} };
      }
      if (command === "list_monitor_hook_locations") {
        return ["hermes", "openClaw"].map((item) => ({
          tool: item,
          directory: `/tmp/${item}`,
          configPath: `/tmp/${item}/hooks.json`,
          isCustom: true,
        }));
      }
      if (command === "write_monitor_hook_config") {
        return {
          tool,
          filename: "hooks.json",
          outcome: tool === "hermes" ? "hermesEnableRequired" : "openClawEnableRequired",
          configChanged: true,
          requiresReview: true,
          restartRequired: true,
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    await renderMonitor("/settings");
    const hooksManagement = await screen.findByTestId("monitor-hooks-management");
    await userEvent.click(
      within(hooksManagement).getByRole("button", { name: "Write Hooks" }),
    );

    let guidance = await within(hooksManagement).findByTestId("hook-activation-guidance");
    expect(
      within(guidance).getByText(
        "Run the command below, then restart Hermes or start a new session.",
      ),
    ).toBeVisible();
    expect(within(guidance).getByText("hermes plugins enable lokimetis")).toBeVisible();
    expect(
      within(guidance).getByRole("button", {
        name: "Copy command: hermes plugins enable lokimetis",
      }),
    ).toBeVisible();

    await userEvent.click(within(hooksManagement).getByRole("tab", { name: "OpenClaw" }));
    await userEvent.click(
      within(hooksManagement).getByRole("button", { name: "Write Hooks" }),
    );
    guidance = await within(hooksManagement).findByTestId("hook-activation-guidance");

    expect(
      within(guidance).getByText("Run the following commands in order."),
    ).toBeVisible();
    const enable = within(guidance).getByText("openclaw plugins enable lokimetis");
    const allowAccess = within(guidance).getByText(
      "openclaw config set plugins.entries.lokimetis.hooks.allowConversationAccess true",
    );
    const restart = within(guidance).getByText("openclaw gateway restart");
    expect(
      enable.compareDocumentPosition(allowAccess) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      allowAccess.compareDocumentPosition(restart) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);
    expect(
      within(guidance).getAllByRole("button", { name: /^Copy command:/ }),
    ).toHaveLength(3);
  });

  /** Hooks 选项卡可启用 Agent 并写入/定位 Hook 配置。 */
  test("hooks_settings_tab_enables_agents_and_writes_local_hook_config", async () => {
    invokeMock.mockImplementation(async (command, payload) => {
      const typedPayload = payload as
        { tools?: string[]; tool?: string; directory?: string } | undefined;
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
          enabledAiTools: ["codex", "openCode"],
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
          enabledAiTools: typedPayload?.tools ?? ["codex", "grok"],
          hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
        };
      }
      if (command === "save_hook_config_directory") {
        const directory = typedPayload?.directory || "/Users/test/.codex";
        return {
          tool: typedPayload?.tool ?? "codex",
          directory,
          configPath: `${directory}/hooks.json`,
          isCustom: Boolean(typedPayload?.directory),
        };
      }
      if (command === "write_monitor_hook_config") {
        return {
          tool: typedPayload?.tool ?? "codex",
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
          bindAddress: "127.0.0.1:23456",
          receivedCount: 0,
          failedCount: 0,
          lastEvent: null,
          lastError: null,
        };
      }
      throw new Error(`unexpected command ${command}`);
    });
    const router = await renderMonitor("/settings");
    expect(router.state.location.pathname).toBe("/settings");
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
    const directoryInput = within(hooksManagement).getByRole("textbox", {
      name: "Config directory",
    });
    expect(directoryInput).not.toHaveAttribute("readonly");
    expect(
      within(hooksManagement).getByRole("textbox", { name: "Configuration file" }),
    ).toHaveAttribute("readonly");
    await userEvent.clear(directoryInput);
    await userEvent.type(directoryInput, "/tmp/codex-next");
    expect(
      within(hooksManagement).getByRole("button", { name: "Write Hooks" }),
    ).toBeDisabled();
    await userEvent.click(
      within(hooksManagement).getByRole("button", { name: "Save path" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("save_hook_config_directory", {
      tool: "codex",
      directory: "/tmp/codex-next",
    });
    await waitFor(() => {
      expect(
        within(hooksManagement).getByRole("textbox", { name: "Configuration file" }),
      ).toHaveValue("/tmp/codex-next/hooks.json");
      expect(
        within(hooksManagement).getByRole("button", { name: "Write Hooks" }),
      ).toBeEnabled();
    });
    await userEvent.click(
      within(hooksManagement).getByRole("button", { name: "Write Hooks" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("write_monitor_hook_config", { tool: "codex" });
    expect(await within(hooksManagement).findByText(/Wrote hooks.json/)).toBeVisible();
    await userEvent.click(
      within(enabledAgents).getByRole("checkbox", { name: "Grok Build" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("save_monitor_enabled_tools", {
      tools: ["codex", "grok"],
    });
    const grokTab = await within(hooksManagement).findByRole("tab", { name: "Grok Build" });
    await userEvent.click(grokTab);
    expect(grokTab).toHaveAttribute("aria-selected", "true");
    expect(codexTab).toHaveAttribute("aria-selected", "false");
    expect(within(hooksManagement).queryByText(/Wrote hooks.json/)).not.toBeInTheDocument();
    await userEvent.click(codexTab);
    expect(codexTab).toHaveAttribute("aria-selected", "true");
    expect(within(hooksManagement).queryByText(/Wrote hooks.json/)).not.toBeInTheDocument();
  });

  /** 结构化目录错误显示本地化原因，不能退化成 `[object Object]`。 */
  test("hooks_settings_localize_structured_directory_errors", async () => {
    invokeMock.mockImplementation(async (command) => {
      if (command === "get_monitor_capabilities") {
        return monitorCapabilitiesFixture({
          aiTools: [{ tool: "codex", name: "Codex" }],
        });
      }
      if (command === "get_monitor_settings") {
        return { enabledAiTools: ["codex"], hookDirectories: { codex: "" } };
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
      if (command === "save_hook_config_directory") {
        throw {
          code: "error.hooks.directoryNotAbsolute",
          params: { path: "relative/hooks" },
        };
      }
      throw new Error(`unexpected command ${command}`);
    });

    await renderMonitor("/settings");
    const hooksManagement = await screen.findByTestId("monitor-hooks-management");
    const directoryInput = within(hooksManagement).getByRole("textbox", {
      name: "Config directory",
    });
    await userEvent.clear(directoryInput);
    await userEvent.type(directoryInput, "relative/hooks");
    await userEvent.click(
      within(hooksManagement).getByRole("button", { name: "Save path" }),
    );

    expect(
      await within(hooksManagement).findByText(
        "The Hooks configuration directory must be an absolute path.",
      ),
    ).toBeVisible();
    expect(within(hooksManagement).queryByText("[object Object]")).not.toBeInTheDocument();
  });
});
