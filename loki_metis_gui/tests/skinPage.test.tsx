import { render, screen } from "@testing-library/react";
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

  /** 真实宿主契约返回目录后，页面必须完整呈现七套迁移的内置资源和唯一实例。 */
  test("renders the migrated built-in catalog in a Tauri host", async () => {
    mocks.hostAvailable = true;
    const names = [
      "Aurora Theme",
      "Minecraft",
      "Misty Meadow Dawn",
      "Pastoral Landscape",
      "Pikachu",
      "Silver Core Voyage",
      "Woodland Dawn",
    ];
    mocks.invoke.mockImplementation((command: string) => {
      if (command === "list_skins") {
        return Promise.resolve(
          names.map((name, index) => ({
            author: "LokiMetis",
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
      if (command === "codex_runtime_status") return Promise.resolve({ state: "ready" });
      if (command === "list_codex_instances") {
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

    expect(await screen.findByText("Showing 7 skins")).toBeVisible();
    expect(screen.getAllByTestId(/^skin-card-/)).toHaveLength(7);
    expect(screen.getByText("Codex is connectable")).toBeVisible();
    expect(screen.getByRole("button", { name: "Create theme" })).toBeEnabled();
  });
});
