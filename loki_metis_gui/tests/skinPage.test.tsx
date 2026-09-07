import { render, screen } from "@testing-library/react";
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

  /** 统一 Agent 选择同时启用两个换皮宿主时，页面动态呈现两个隔离选项卡。 */
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
        return Promise.resolve({ state: "runningWithoutCdp" });
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
      screen.queryByText(
        "Manage the local skin library and safely apply a theme to an explicitly selected instance in the active host tab.",
      ),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Debug connection required")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create theme" })).toBeEnabled();

    await userEvent.click(screen.getByRole("tab", { name: "WorkBuddy" }));
    expect(mocks.invoke).toHaveBeenCalledWith("skin_host_runtime_status", {
      host: "workBuddy",
    });
    expect(mocks.invoke).toHaveBeenCalledWith("list_skin_host_instances", {
      host: "workBuddy",
    });
  });
});
