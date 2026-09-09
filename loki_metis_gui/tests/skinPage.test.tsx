import { render, screen, waitFor } from "@testing-library/react";
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
});
