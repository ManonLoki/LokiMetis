import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { MonitorManagementPage } from "../src/pages/MonitorManagementPage";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 四项 Agent 的空白草稿。 */
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

/** 本机图库快照，含一张 PNG。 */
function galleryWithPng() {
  return {
    images: [
      {
        id: "img-1",
        filename: "idle.png",
        format: "png",
        image: "data:image/png;base64,iVBORw0KGgo=",
      },
    ],
    counts: { jpeg: 0, png: 1, gif: 0 },
  };
}

/** 监控静态能力。 */
function capabilities() {
  return {
    aiTools: [
      { tool: "codex", name: "Codex" },
      { tool: "claudeCode", name: "Claude Code" },
      { tool: "grok", name: "Grok Build" },
      { tool: "workBuddy", name: "WorkBuddy" },
    ],
    hookBehaviors: ["idle", "running", "asking", "error"],
    profileSlot: { default: 1, min: 1, max: 6 },
    imageUploadAccept: {
      mimeTypes: ["image/jpeg", "image/png", "image/gif"],
      extensions: [".jpg", ".jpeg", ".png", ".gif"],
    },
  };
}

describe("monitor management page", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  /** 未启用 Agent 时给出明确空态，且无设备门禁文案。 */
  test("shows_empty_state_when_no_agent_is_enabled", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_monitor_capabilities") return capabilities();
      if (command === "get_monitor_settings") {
        return {
          enabledAiTools: [],
          hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
        };
      }
      if (command === "list_monitor_profile_drafts") return emptyDrafts();
      if (command === "list_monitor_images_cmd") {
        return { images: [], counts: { jpeg: 0, png: 0, gif: 0 } };
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <MonitorManagementPage />
      </TestProviders>,
    );
    expect(await screen.findByText(/No agent is enabled/)).toBeVisible();
    expect(screen.queryByText(/device/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/LAN/i)).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("list_monitor_images_cmd");
    expect(invokeMock).not.toHaveBeenCalledWith("list_remote_images");
  });

  /** 已启用 Agent 时出现 Tab、行为卡片、图片选择与保存草稿。 */
  test("renders_agent_tabs_behavior_cards_and_saves_local_draft", async () => {
    invokeMock.mockImplementation(async (command: string, payload?: { profile?: { tool: string } }) => {
      if (command === "get_monitor_capabilities") return capabilities();
      if (command === "get_monitor_settings") {
        return {
          enabledAiTools: ["codex", "grok"],
          hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
        };
      }
      if (command === "list_monitor_profile_drafts") return emptyDrafts();
      if (command === "list_monitor_images_cmd") return galleryWithPng();
      if (command === "save_monitor_profile_draft") {
        return payload?.profile;
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <MonitorManagementPage />
      </TestProviders>,
    );
    expect(await screen.findByRole("tab", { name: "Codex" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Grok Build" })).toBeVisible();
    expect(screen.queryByRole("tab", { name: "Claude Code" })).not.toBeInTheDocument();
    expect(screen.getByText("Idle")).toBeVisible();
    expect(screen.getByText("Running")).toBeVisible();
    expect(screen.getByText("Asking")).toBeVisible();
    expect(screen.getByText("Error")).toBeVisible();
    expect(screen.getByText("Display position")).toBeVisible();
    expect(screen.getByRole("button", { name: "Position 1, row 1, column 1" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Position 6, row 1, column 6" })).toBeVisible();
    expect(screen.queryByRole("button", { name: /Position 7,/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /row 2,/ })).not.toBeInTheDocument();
    const pickers = screen.getAllByTestId("image-picker-trigger");
    expect(pickers.length).toBeGreaterThan(0);
    await userEvent.click(pickers[0]);
    expect(await screen.findByText("Choose display image")).toBeVisible();
    await userEvent.click(await screen.findByRole("option", { name: "Choose image idle.png" }));
    await userEvent.click(screen.getByRole("button", { name: "Save display configuration" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "save_monitor_profile_draft",
        expect.objectContaining({
          profile: expect.objectContaining({
            tool: "codex",
            hooks: expect.arrayContaining([
              expect.objectContaining({ behavior: "idle", image: "img-1" }),
            ]),
          }),
        }),
      );
    });
    expect(invokeMock).toHaveBeenCalledWith("list_monitor_images_cmd");
    expect(invokeMock).not.toHaveBeenCalledWith("list_remote_images");
    expect(screen.queryByText(/Write Hooks/)).not.toBeInTheDocument();
  });

  /** 设置未返回前不得把默认空列表渲染成「未启用 Agent」空态。 */
  test("does_not_show_empty_state_while_settings_are_pending", async () => {
    let releaseSettings: ((value: unknown) => void) | undefined;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_monitor_capabilities") return capabilities();
      if (command === "get_monitor_settings") {
        return await new Promise((resolve) => {
          releaseSettings = resolve;
        });
      }
      if (command === "list_monitor_profile_drafts") return emptyDrafts();
      if (command === "list_monitor_images_cmd") {
        return { images: [], counts: { jpeg: 0, png: 0, gif: 0 } };
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <MonitorManagementPage />
      </TestProviders>,
    );
    expect(await screen.findByTestId("monitor-management")).toBeVisible();
    expect(screen.queryByText(/No agent is enabled/)).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "Codex" })).not.toBeInTheDocument();
    releaseSettings?.({
      enabledAiTools: ["codex"],
      hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
    });
    expect(await screen.findByRole("tab", { name: "Codex" })).toBeVisible();
    expect(screen.queryByText(/No agent is enabled/)).not.toBeInTheDocument();
  });

  /** 同名直传绑定新写入的稳定 id，而不是图库里第一张同名旧图。 */
  test("direct_upload_binds_the_newly_saved_image_id_not_the_first_same_filename", async () => {
    invokeMock.mockImplementation(async (command: string, payload?: { profile?: { tool: string } }) => {
      if (command === "get_monitor_capabilities") return capabilities();
      if (command === "get_monitor_settings") {
        return {
          enabledAiTools: ["codex"],
          hookDirectories: { codex: "", claudeCode: "", grok: "", workBuddy: "" },
        };
      }
      if (command === "list_monitor_profile_drafts") return emptyDrafts();
      if (command === "list_monitor_images_cmd") {
        return {
          images: [
            {
              id: "img-old",
              filename: "idle.png",
              format: "png",
              image: "data:image/png;base64,old",
            },
          ],
          counts: { jpeg: 0, png: 1, gif: 0 },
        };
      }
      if (command === "save_monitor_image_cmd") {
        return {
          images: [
            {
              id: "img-old",
              filename: "idle.png",
              format: "png",
              image: "data:image/png;base64,old",
            },
            {
              id: "img-new",
              filename: "idle.png",
              format: "png",
              image: "data:image/png;base64,new",
            },
          ],
          counts: { jpeg: 0, png: 2, gif: 0 },
        };
      }
      if (command === "save_monitor_profile_draft") {
        return payload?.profile;
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <MonitorManagementPage />
      </TestProviders>,
    );
    const pickers = await screen.findAllByTestId("image-picker-trigger");
    await userEvent.click(pickers[0]);
    const fileInput = await screen.findByTestId("image-picker-upload");
    const file = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])], "idle.png", {
      type: "image/png",
    });
    await userEvent.upload(fileInput, file);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "save_monitor_image_cmd",
        expect.objectContaining({ filename: "idle.png" }),
      );
    });
    await userEvent.click(screen.getByRole("button", { name: "Save display configuration" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "save_monitor_profile_draft",
        expect.objectContaining({
          profile: expect.objectContaining({
            tool: "codex",
            hooks: expect.arrayContaining([
              expect.objectContaining({ behavior: "idle", image: "img-new" }),
            ]),
          }),
        }),
      );
    });
    expect(invokeMock).not.toHaveBeenCalledWith(
      "save_monitor_profile_draft",
      expect.objectContaining({
        profile: expect.objectContaining({
          hooks: expect.arrayContaining([
            expect.objectContaining({ behavior: "idle", image: "img-old" }),
          ]),
        }),
      }),
    );
  });
});
