import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  hostAvailable: false,
  invoke: vi.fn(),
  onDragDropEvent: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class MockChannel {},
  invoke: mocks.invoke,
  isTauri: () => mocks.hostAvailable,
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: mocks.onDragDropEvent }),
}));

import { SkinPage } from "../src/pages/SkinPage";
import { TestProviders } from "./testUtils";

/** 提供无皮肤但权威查询均成功的 Tauri 宿主基线。 */
function mockReadyEmptySkinHost(
  extra?: (command: string) => Promise<unknown> | undefined,
): void {
  mocks.hostAvailable = true;
  mocks.invoke.mockImplementation((command: string) => {
    const overridden = extra?.(command);
    if (overridden !== undefined) return overridden;
    if (command === "get_monitor_capabilities") {
      return Promise.resolve({
        aiTools: [{ tool: "codex", name: "Codex", skinHost: "codex" }],
      });
    }
    if (command === "get_monitor_settings") {
      return Promise.resolve({ enabledAiTools: ["codex"], hookDirectories: {} });
    }
    if (command === "list_skins") return Promise.resolve([]);
    if (command === "list_skin_host_instances") return Promise.resolve([]);
    if (command === "skin_status") {
      return Promise.resolve({
        affectedPages: 0,
        compatibility: null,
        installed: false,
        packageType: null,
        skinId: null,
        skinName: null,
        source: null,
        version: "1",
      });
    }
    return Promise.reject(new Error(`unexpected command: ${command}`));
  });
}

describe("skin page", () => {
  beforeEach(() => {
    mocks.hostAvailable = false;
    mocks.invoke.mockReset();
    mocks.onDragDropEvent.mockReset();
    mocks.onDragDropEvent.mockResolvedValue(() => undefined);
    window.localStorage.clear();
  });

  /** 浏览器预览必须明确禁用宿主能力，同时仍显示可理解的换皮页面。 */
  test("does not access local host outside Tauri", () => {
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "get_monitor_capabilities") return Promise.resolve({ aiTools: [] });
      if (command === "get_monitor_settings") {
        return Promise.resolve({ enabledAiTools: [], hookDirectories: {} });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });
    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );
    expect(screen.getByTestId("skin-page")).toBeVisible();
    expect(screen.getByText("Desktop host required")).toBeVisible();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create theme" })).toBeDisabled();
  });

  /** 两个换皮宿主共用精简工具栏，均不暴露显式启动或用户专属筛选。 */
  test("renders the migrated built-in catalog in a Tauri host", async () => {
    mocks.hostAvailable = true;
    const names = [
      "Minecraft",
      "Misty Meadow Dawn",
      "Pastoral Landscape",
      "Pikachu",
      "Silver Core Voyage",
      "Woodland Dawn",
    ];
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "get_monitor_capabilities") {
        return Promise.resolve({
          aiTools: [
            { tool: "codex", name: "Codex", skinHost: "codex" },
            { tool: "workBuddy", name: "WorkBuddy", skinHost: "workBuddy" },
            { tool: "cursor", name: "Cursor", skinHost: null },
          ],
        });
      }
      if (command === "get_monitor_settings") {
        return Promise.resolve({
          enabledAiTools: ["codex", "workBuddy", "cursor"],
          hookDirectories: {},
        });
      }
      if (command === "list_skins") {
        return Promise.resolve(
          names.map((name, index) => ({
            author: "ManonLoki",
            id: `builtin-${index + 1}`,
            name,
            packageType: index === 0 ? "theme" : "legacySkin",
            previewDataUrl: "",
            source: "builtin",
            supportedColorModes: ["light", "dark"],
            version: "1.0.0",
          })),
        );
      }
      if (command === "skin_host_runtime_status")
        return Promise.resolve({ state: "stopped" });
      if (command === "list_skin_host_instances") {
        return Promise.resolve([
          {
            accountLabel: null,
            activeSkin: null,
            activeSkinName: null,
            avatarDataUrl: null,
            debugPort: 9222,
            id: "codex-1",
            label: "Codex",
            pid: 42,
            profile: null,
            state: "ready",
          },
        ]);
      }
      if (command === "skin_status") {
        return Promise.resolve({
          affectedPages: 0,
          compatibility: null,
          installed: false,
          packageType: null,
          skinId: null,
          skinName: null,
          source: null,
          version: "1",
        });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    expect(await screen.findAllByTestId(/^skin-card-/)).toHaveLength(6);
    expect(screen.getByRole("tab", { name: "Codex" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "WorkBuddy" })).toBeVisible();
    expect(screen.queryByRole("tab", { name: "Cursor" })).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.queryByText("Showing 6 skins")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Skins" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("switch", { name: "User skins only" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Launch Codex" })).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "Manage the local skin library and safely apply a theme to an explicitly selected instance in the active host tab.",
      ),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Debug connection required")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create theme" })).toBeEnabled();

    await userEvent.click(screen.getByRole("tab", { name: "WorkBuddy" }));
    expect(
      screen.queryByRole("button", { name: "Launch WorkBuddy" }),
    ).not.toBeInTheDocument();
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "skin_host_runtime_status",
      expect.anything(),
    );
    expect(mocks.invoke).toHaveBeenCalledWith("list_skin_host_instances", {
      host: "workBuddy",
    });
  });

  /** WorkBuddy 没有唯一安全目标时，必须先确认，再由一次安装命令完成全量恢复。 */
  test("confirms atomic close-all recovery for unsafe workbuddy roots", async () => {
    mocks.hostAvailable = true;
    let recovered = false;
    mocks.invoke.mockImplementation(
      (
        command: string,
        args?: {
          allowAppearanceMismatch?: boolean;
          allowWorkBuddyRecovery?: boolean;
          host?: string;
        },
      ) => {
        if (command === "get_monitor_capabilities") {
          return Promise.resolve({
            aiTools: [
              { tool: "codex", name: "Codex", skinHost: "codex" },
              { tool: "workBuddy", name: "WorkBuddy", skinHost: "workBuddy" },
            ],
          });
        }
        if (command === "get_monitor_settings") {
          return Promise.resolve({
            enabledAiTools: ["codex", "workBuddy"],
            hookDirectories: {},
          });
        }
        if (command === "list_skins") {
          return Promise.resolve([
            {
              author: "ManonLoki",
              id: "minecraft",
              name: "Minecraft",
              packageType: "theme",
              previewDataUrl: "",
              source: "builtin",
              supportedColorModes: ["light", "dark"],
              version: "1.0.0",
            },
          ]);
        }
        if (command === "skin_status") {
          return Promise.resolve({
            affectedPages: 0,
            compatibility: null,
            installed: false,
            packageType: null,
            skinId: null,
            skinName: null,
            source: null,
            version: "1",
          });
        }
        if (command === "supports_windows_workbuddy_recovery") {
          return Promise.resolve(true);
        }
        if (command === "list_skin_host_instances") {
          if (args?.host === "codex") {
            return Promise.resolve([
              {
                accountLabel: null,
                activeSkin: null,
                activeSkinName: null,
                avatarDataUrl: null,
                debugPort: 9341,
                id: "codex-1",
                label: "Codex",
                pid: 10,
                profile: null,
                state: "ready",
              },
            ]);
          }
          const workBuddyInstance = (id: string, pid: number, ready = recovered) => ({
            accountLabel: null,
            activeSkin: null,
            activeSkinName: null,
            avatarDataUrl: null,
            debugPort: ready ? 9441 : null,
            id,
            label: "WorkBuddy",
            pid,
            profile: null,
            state: ready ? "ready" : "runningWithoutCdp",
          });
          return Promise.resolve(
            recovered
              ? [workBuddyInstance("workbuddy-new", 30)]
              : [
                  workBuddyInstance("workbuddy-old", 20, false),
                  workBuddyInstance("workbuddy-other", 21, true),
                ],
          );
        }
        if (command === "install_skin") {
          if (args?.allowWorkBuddyRecovery) recovered = true;
          if (!recovered) {
            return Promise.reject({
              code: "skin.workbuddy_recovery_required",
              details: [],
              message: "WorkBuddy debug connection is unavailable.",
            });
          }
          if (args?.allowAppearanceMismatch) {
            return Promise.resolve({ type: "installed", status: {} });
          }
          return Promise.resolve({
            type: "needsConfirmation",
            check: {
              differences: [
                {
                  currentValue: "dark",
                  expectedValue: "light",
                  field: "colorMode",
                  label: "Color mode",
                },
              ],
              effectiveMode: "dark",
              supportedColorModes: ["light"],
              unreadable: [],
            },
          });
        }
        return Promise.reject(new Error(`unexpected command: ${command}`));
      },
    );

    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    await screen.findByTestId("skin-card-minecraft");
    await userEvent.click(screen.getByRole("tab", { name: "WorkBuddy" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("list_skin_host_instances", {
        host: "workBuddy",
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(await screen.findByText("Restart WorkBuddy with a debug port?")).toBeVisible();
    expect(
      screen.getByText(
        /closes every running WorkBuddy process under a verified official install path/i,
      ),
    ).toBeVisible();
    expect(mocks.invoke).not.toHaveBeenCalledWith("force_launch_skin_host", {
      host: "workBuddy",
    });
    expect(mocks.invoke).toHaveBeenCalledWith(
      "supports_windows_workbuddy_recovery",
      undefined,
    );

    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("install_skin", {
        allowAppearanceMismatch: false,
        allowThirdPartyCode: false,
        allowWorkBuddyRecovery: true,
        host: "workBuddy",
        skin: { id: "minecraft", source: "builtin" },
      }),
    );
    expect(mocks.invoke).not.toHaveBeenCalledWith("force_launch_skin_host", {
      host: "workBuddy",
    });
    expect(await screen.findByText("Confirm appearance differences")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Apply anyway" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("install_skin", {
        allowAppearanceMismatch: true,
        allowThirdPartyCode: false,
        allowWorkBuddyRecovery: true,
        host: "workBuddy",
        skin: { id: "minecraft", source: "builtin" },
      }),
    );
  });

  /** 非 Windows 平台必须保留原来的所选实例重启，不能调用 close-all 恢复。 */
  test("keeps selected-instance workbuddy restart outside windows", async () => {
    mocks.hostAvailable = true;
    let restarted = false;
    const instance = (ready: boolean) => ({
      accountLabel: null,
      activeSkin: null,
      activeSkinName: null,
      avatarDataUrl: null,
      debugPort: ready ? 9441 : null,
      id: "workbuddy-1",
      label: "WorkBuddy process 20",
      pid: 20,
      profile: null,
      state: ready ? "ready" : "runningWithoutCdp",
    });
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "get_monitor_capabilities") {
        return Promise.resolve({
          aiTools: [{ tool: "workBuddy", name: "WorkBuddy", skinHost: "workBuddy" }],
        });
      }
      if (command === "get_monitor_settings") {
        return Promise.resolve({ enabledAiTools: ["workBuddy"], hookDirectories: {} });
      }
      if (command === "list_skins") {
        return Promise.resolve([
          {
            author: "ManonLoki",
            id: "minecraft",
            name: "Minecraft",
            packageType: "theme",
            previewDataUrl: "",
            source: "builtin",
            supportedColorModes: ["light", "dark"],
            version: "1.0.0",
          },
        ]);
      }
      if (command === "skin_status") {
        return Promise.resolve({
          affectedPages: 0,
          compatibility: null,
          installed: false,
          packageType: null,
          skinId: null,
          skinName: null,
          source: null,
          version: "1",
        });
      }
      if (command === "list_skin_host_instances") {
        return Promise.resolve([instance(restarted)]);
      }
      if (command === "supports_windows_workbuddy_recovery") return Promise.resolve(false);
      if (command === "restart_skin_host_instance") {
        restarted = true;
        return Promise.resolve(instance(true));
      }
      if (command === "install_skin") {
        return restarted
          ? Promise.resolve({ type: "installed", status: {} })
          : Promise.reject({
              code: "skin.workbuddy_recovery_required",
              details: [],
              message: "WorkBuddy debug connection is unavailable.",
            });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    await userEvent.click(await screen.findByRole("button", { name: "Apply" }));
    expect(await screen.findByText("Restart the selected host instance?")).toBeVisible();
    expect(
      screen.queryByText("Restart WorkBuddy with a debug port?"),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));

    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("restart_skin_host_instance", {
        host: "workBuddy",
        instanceId: "workbuddy-1",
      }),
    );
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "force_launch_skin_host",
      expect.anything(),
    );
  });

  /** WorkBuddy 恢复分支不能改变 Codex 原有的单实例重启确认。 */
  test("keeps selected-instance restart confirmation for codex", async () => {
    mocks.hostAvailable = true;
    let restarted = false;
    const instance = (ready: boolean) => ({
      accountLabel: null,
      activeSkin: null,
      activeSkinName: null,
      avatarDataUrl: null,
      debugPort: ready ? 9222 : null,
      id: "codex-1",
      label: "Codex process 10",
      pid: 10,
      profile: null,
      state: ready ? "ready" : "runningWithoutCdp",
    });
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "get_monitor_capabilities") {
        return Promise.resolve({
          aiTools: [{ tool: "codex", name: "Codex", skinHost: "codex" }],
        });
      }
      if (command === "get_monitor_settings") {
        return Promise.resolve({ enabledAiTools: ["codex"], hookDirectories: {} });
      }
      if (command === "list_skins") {
        return Promise.resolve([
          {
            author: "ManonLoki",
            id: "minecraft",
            name: "Minecraft",
            packageType: "theme",
            previewDataUrl: "",
            source: "builtin",
            supportedColorModes: ["light", "dark"],
            version: "1.0.0",
          },
        ]);
      }
      if (command === "skin_status") {
        return Promise.resolve({
          affectedPages: 0,
          compatibility: null,
          installed: false,
          packageType: null,
          skinId: null,
          skinName: null,
          source: null,
          version: "1",
        });
      }
      if (command === "list_skin_host_instances") {
        return Promise.resolve([instance(restarted)]);
      }
      if (command === "restart_skin_host_instance") {
        restarted = true;
        return Promise.resolve(instance(true));
      }
      if (command === "install_skin") {
        return restarted
          ? Promise.resolve({ type: "installed", status: {} })
          : Promise.reject(new Error("install must wait for restart confirmation"));
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    await userEvent.click(await screen.findByRole("button", { name: "Apply" }));
    expect(await screen.findByText("Restart the selected host instance?")).toBeVisible();
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "supports_windows_workbuddy_recovery",
      expect.anything(),
    );
    expect(mocks.invoke).not.toHaveBeenCalledWith("install_skin", expect.anything());

    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("restart_skin_host_instance", {
        host: "codex",
        instanceId: "codex-1",
      }),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("install_skin", expect.anything()),
    );
  });

  /** 重启确认和后续安装必须锁定发起时的宿主，不能因标签切换而错投。 */
  test("binds a restart confirmation to its originating host", async () => {
    mocks.hostAvailable = true;
    let restarted = false;
    const instance = (host: "codex" | "workBuddy", ready: boolean) => ({
      accountLabel: null,
      activeSkin: null,
      activeSkinName: null,
      avatarDataUrl: null,
      debugPort: ready ? (host === "codex" ? 9222 : 9441) : null,
      id: `${host}-1`,
      label: host === "codex" ? "Codex process 10" : "WorkBuddy process 20",
      pid: host === "codex" ? 10 : 20,
      profile: null,
      state: ready ? "ready" : "runningWithoutCdp",
    });
    mocks.invoke.mockImplementation(
      (command: string, args?: { host?: "codex" | "workBuddy" }) => {
        if (command === "get_monitor_capabilities") {
          return Promise.resolve({
            aiTools: [
              { tool: "codex", name: "Codex", skinHost: "codex" },
              { tool: "workBuddy", name: "WorkBuddy", skinHost: "workBuddy" },
            ],
          });
        }
        if (command === "get_monitor_settings") {
          return Promise.resolve({
            enabledAiTools: ["codex", "workBuddy"],
            hookDirectories: {},
          });
        }
        if (command === "list_skins") {
          return Promise.resolve([
            {
              author: "ManonLoki",
              id: "minecraft",
              name: "Minecraft",
              packageType: "theme",
              previewDataUrl: "",
              source: "builtin",
              supportedColorModes: ["light", "dark"],
              version: "1.0.0",
            },
          ]);
        }
        if (command === "list_skin_host_instances") {
          const targetHost = args?.host ?? "codex";
          return Promise.resolve([
            instance(targetHost, targetHost === "workBuddy" || restarted),
          ]);
        }
        if (command === "skin_status") {
          return Promise.resolve({
            affectedPages: 0,
            compatibility: null,
            installed: false,
            packageType: null,
            skinId: null,
            skinName: null,
            source: null,
            version: "1",
          });
        }
        if (command === "restart_skin_host_instance") {
          if (args?.host !== "codex") return Promise.reject(new Error("wrong host"));
          restarted = true;
          return Promise.resolve(instance("codex", true));
        }
        if (command === "install_skin") {
          return Promise.resolve({ type: "installed", status: {} });
        }
        return Promise.reject(new Error(`unexpected command: ${command}`));
      },
    );

    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    await userEvent.click(await screen.findByRole("button", { name: "Apply" }));
    expect(await screen.findByText("Restart the selected host instance?")).toBeVisible();
    expect(screen.getByRole("tab", { name: "Codex" })).toBeDisabled();
    expect(screen.getByRole("tab", { name: "WorkBuddy" })).toBeDisabled();

    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("restart_skin_host_instance", {
        host: "codex",
        instanceId: "codex-1",
      }),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("install_skin", {
        allowAppearanceMismatch: false,
        allowThirdPartyCode: false,
        allowWorkBuddyRecovery: false,
        host: "codex",
        instanceId: "codex-1",
        skin: { id: "minecraft", source: "builtin" },
      }),
    );
  });

  /** 兼容皮肤必须在任何启动、重启或安装调用前取得一次性显式信任，取消保持零副作用。 */
  test("gates compatible skin application before every host side effect", async () => {
    mocks.hostAvailable = true;
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "get_monitor_capabilities") {
        return Promise.resolve({
          aiTools: [{ tool: "codex", name: "Codex", skinHost: "codex" }],
        });
      }
      if (command === "get_monitor_settings") {
        return Promise.resolve({ enabledAiTools: ["codex"], hookDirectories: {} });
      }
      if (command === "list_skins") {
        return Promise.resolve([
          {
            author: "External author",
            id: "retro-script",
            name: "Retro Script",
            packageType: "legacySkin",
            previewDataUrl: "",
            source: "user",
            supportedColorModes: ["light"],
            version: "1.0.0",
          },
        ]);
      }
      if (command === "list_skin_host_instances") {
        return Promise.resolve([
          {
            accountLabel: null,
            activeSkin: null,
            activeSkinName: null,
            avatarDataUrl: null,
            debugPort: 9341,
            id: "codex-1",
            label: "Codex",
            pid: 42,
            profile: null,
            state: "ready",
          },
        ]);
      }
      if (command === "skin_status") {
        return Promise.resolve({
          affectedPages: 0,
          compatibility: null,
          installed: false,
          packageType: null,
          skinId: null,
          skinName: null,
          source: null,
          version: "1",
        });
      }
      if (command === "install_skin") {
        return Promise.resolve({ type: "installed", status: {} });
      }
      return Promise.reject(new Error(`unexpected command: ${command}`));
    });

    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    expect(await screen.findByText("Runs third-party code")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(await screen.findByText("Trust and execute third-party code?")).toBeVisible();
    expect(
      screen.getByText(/will execute this third-party code inside Codex/i),
    ).toBeVisible();
    expect(screen.getByText(/do not review or prove the script is safe/i)).toBeVisible();
    expect(mocks.invoke).not.toHaveBeenCalledWith("install_skin", expect.anything());
    expect(mocks.invoke).not.toHaveBeenCalledWith("launch_skin_host", expect.anything());
    expect(mocks.invoke).not.toHaveBeenCalledWith(
      "restart_skin_host_instance",
      expect.anything(),
    );

    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(
      screen.queryByText("Trust and execute third-party code?"),
    ).not.toBeInTheDocument();
    expect(mocks.invoke).not.toHaveBeenCalledWith("install_skin", expect.anything());

    await userEvent.click(screen.getByRole("button", { name: "Apply" }));
    const continueButton = await screen.findByRole("button", {
      name: "Trust and continue",
    });
    expect(continueButton).toBeDisabled();
    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I understand this will execute third-party code/i,
      }),
    );
    await userEvent.click(continueButton);
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("install_skin", {
        allowAppearanceMismatch: false,
        allowThirdPartyCode: true,
        allowWorkBuddyRecovery: false,
        host: "codex",
        instanceId: "codex-1",
        skin: { id: "retro-script", source: "user" },
      }),
    );
  });

  /** 页面先卸载、拖放订阅后完成时，晚到的 cleanup 仍必须立刻执行。 */
  test("cleans up a drag-drop listener that resolves after unmount", async () => {
    mockReadyEmptySkinHost();
    let resolveListener: ((cleanup: () => void) => void) | undefined;
    mocks.onDragDropEvent.mockImplementation(
      () =>
        new Promise<() => void>((resolve) => {
          resolveListener = resolve;
        }),
    );
    const cleanup = vi.fn();
    const view = render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    await waitFor(() => expect(mocks.onDragDropEvent).toHaveBeenCalledOnce());
    view.unmount();
    await act(async () => resolveListener?.(cleanup));

    await waitFor(() => expect(cleanup).toHaveBeenCalledOnce());
  });

  /** 原生预检失败后必须清除动画进度，并把失败保留在可见错误边界。 */
  test("clears import progress when native preparation fails", async () => {
    let rejectImport: ((cause: unknown) => void) | undefined;
    const preparation = new Promise<never>((_resolve, reject) => {
      rejectImport = reject;
    });
    mockReadyEmptySkinHost((command) =>
      command === "prepare_skin_import" ? preparation : undefined,
    );
    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    const importButton = await screen.findByRole("button", { name: "Import" });
    await waitFor(() => expect(importButton).toBeEnabled());
    await userEvent.click(importButton);
    expect(screen.getByRole("progressbar")).toBeVisible();

    await act(async () => rejectImport?.(new Error("native preparation failed")));

    await waitFor(() => expect(screen.queryByRole("progressbar")).not.toBeInTheDocument());
    expect(await screen.findByText("native preparation failed")).toBeVisible();
  });

  /** 宿主权威查询失败不得伪装成空列表，重试成功前也不得开放写操作。 */
  test("shows and recovers from an authoritative host query failure", async () => {
    let instanceReads = 0;
    mockReadyEmptySkinHost((command) => {
      if (command !== "list_skin_host_instances") return undefined;
      instanceReads += 1;
      return instanceReads === 1
        ? Promise.reject(new Error("instance query failed"))
        : Promise.resolve([]);
    });
    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    expect(await screen.findByText("instance query failed")).toBeVisible();
    expect(
      screen.queryByText("No skins match the current filters."),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();

    await userEvent.click(screen.getByRole("button", { name: "Retry" }));

    await waitFor(() =>
      expect(screen.queryByText("instance query failed")).not.toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Import" })).toBeEnabled(),
    );
  });

  /** 后台刷新失败时保留已读取目录，但冻结写操作直至权威查询恢复。 */
  test("keeps a cached catalog visible while a host refetch is failing", async () => {
    let catalogReads = 0;
    mockReadyEmptySkinHost((command) => {
      if (command !== "list_skins") return undefined;
      catalogReads += 1;
      if (catalogReads > 1) return Promise.reject(new Error("catalog refresh failed"));
      return Promise.resolve([
        {
          author: "ManonLoki",
          id: "cached-theme",
          name: "Cached theme",
          packageType: "theme",
          previewDataUrl: "",
          source: "user",
          supportedColorModes: ["light", "dark"],
          version: "1.0.0",
        },
      ]);
    });
    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    expect(await screen.findByText("Cached theme")).toBeVisible();
    await userEvent.click(screen.getByRole("checkbox", { name: "Select Cached theme" }));
    expect(screen.getByRole("button", { name: "Delete selected (1)" })).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(await screen.findByText("catalog refresh failed")).toBeVisible();
    expect(screen.getByText("Cached theme")).toBeVisible();
    expect(screen.getByRole("button", { name: "Import" })).toBeDisabled();
    const deleteSelected = screen.getByRole("button", { name: "Delete selected (1)" });
    expect(deleteSelected).toBeDisabled();
    await userEvent.click(deleteSelected);
    expect(mocks.invoke).not.toHaveBeenCalledWith("delete_skins", expect.anything());
  });

  /** 宿主能力尚未读取时目录保持加载态，不能把 disabled query 伪装成空目录。 */
  test("does not show an empty catalog while host selection is still loading", async () => {
    let resolveCapabilities:
      | ((value: {
          aiTools: Array<{ name: string; skinHost: "codex"; tool: string }>;
        }) => void)
      | undefined;
    const pendingCapabilities = new Promise<{
      aiTools: Array<{ name: string; skinHost: "codex"; tool: string }>;
    }>((resolve) => {
      resolveCapabilities = resolve;
    });
    mockReadyEmptySkinHost((command) =>
      command === "get_monitor_capabilities" ? pendingCapabilities : undefined,
    );
    render(
      <TestProviders>
        <SkinPage />
      </TestProviders>,
    );

    expect(
      screen.queryByText("No skins match the current filters."),
    ).not.toBeInTheDocument();
    await act(async () =>
      resolveCapabilities?.({
        aiTools: [{ name: "Codex", skinHost: "codex", tool: "codex" }],
      }),
    );

    expect(await screen.findByText("No skins match the current filters.")).toBeVisible();
  });
});
