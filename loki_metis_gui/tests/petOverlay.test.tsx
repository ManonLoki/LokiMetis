import { createEvent, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { PetOverlayPage } from "../src/pages/PetOverlayPage";
import { monitorImageBytesToDataUrl } from "../src/pet-overlay-image";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const pngMagic = [137, 80, 78, 71];
describe("desktop pet overlay page", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_pet_overlay_view") {
        return {
          slots: [
            { tool: "codex", name: "Codex", occupied: true, imageId: "img-1" },
            { tool: "claudeCode", name: "Claude Code", occupied: false, imageId: null },
            { tool: "grok", name: "Grok Build", occupied: false, imageId: null },
            { tool: "workBuddy", name: "WorkBuddy", occupied: false, imageId: null },
          ],
        };
      }
      if (command === "get_monitor_image_bytes") {
        return pngMagic;
      }
      if (command === "close_pet_overlay") {
        return { label: "pet", hideMainWindow: false, stopHookListener: false };
      }
      if (command === "start_pet_overlay_drag") {
        return null;
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 悬浮窗根渲染四项 Agent 槽位，不出现未批准工具。 */
  test("pet_overlay_page_renders_four_approved_slots", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    expect(await screen.findByTestId("pet-overlay-page")).toBeVisible();
    expect(await screen.findByText("Codex")).toBeVisible();
    expect(screen.getByText("Claude Code")).toBeVisible();
    expect(screen.getByText("Grok Build")).toBeVisible();
    expect(screen.getByText("WorkBuddy")).toBeVisible();
    expect(screen.queryByText(/Cursor/i)).not.toBeInTheDocument();
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_pet_overlay_view");
    });
  });

  /** 占用槽位必须用 data: URL，才能通过现有 img-src CSP。 */
  test("occupied_slot_uses_csp_allowed_data_url_not_blob", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const image = await screen.findByRole("img", { name: "Codex" });
    const src = image.getAttribute("src") ?? "";
    expect(src).toBe(monitorImageBytesToDataUrl(pngMagic));
    expect(src.startsWith("data:image/png;base64,")).toBe(true);
    expect(src.startsWith("blob:")).toBe(false);
    expect(invokeMock).toHaveBeenCalledWith("get_monitor_image_bytes", { id: "img-1" });
  });

  /** 没有任何当前 Hook 行为时四槽保持空白，且不会请求初始图片字节。 */
  test("no_current_behavior_keeps_all_slots_without_initial_images", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_pet_overlay_view") {
        return {
          slots: [
            { tool: "codex", name: "Codex", occupied: false, imageId: null },
            { tool: "claudeCode", name: "Claude Code", occupied: false, imageId: null },
            { tool: "grok", name: "Grok Build", occupied: false, imageId: null },
            { tool: "workBuddy", name: "WorkBuddy", occupied: false, imageId: null },
          ],
        };
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    expect(await screen.findByText("Codex")).toBeVisible();
    expect(screen.queryAllByRole("img")).toHaveLength(0);
    expect(invokeMock).not.toHaveBeenCalledWith(
      "get_monitor_image_bytes",
      expect.anything(),
    );
  });

  /** 圆形关闭控件始终可见可点，且不读取可见性设置。 */
  test("permanent_close_control_closes_overlay_without_reading_visibility_setting", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const close = await screen.findByRole("button", {
      name: "Close the desktop pet overlay",
    });
    expect(close).toHaveClass("pet-close");
    expect(
      invokeMock.mock.calls.some(([command]) => command === "get_monitor_settings"),
    ).toBe(false);
    await userEvent.click(close);
    expect(invokeMock).toHaveBeenCalledWith("close_pet_overlay");
  });

  /** 浮窗主体左键开始原生拖动；常驻关闭控件上的左键不拖动。 */
  test("left_button_on_shell_starts_drag_but_close_control_does_not", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    fireEvent.mouseDown(shell, { button: 0 });
    expect(invokeMock).toHaveBeenCalledWith("start_pet_overlay_drag");
    invokeMock.mockClear();
    const close = screen.getByRole("button", { name: "Close the desktop pet overlay" });
    fireEvent.mouseDown(close, { button: 0 });
    expect(invokeMock).not.toHaveBeenCalledWith("start_pet_overlay_drag");
  });

  /** 原生 start_dragging 会吃掉 mouseup，位置保存不走 webview 读回/写入命令。 */
  test("webview_mouseup_does_not_save_overlay_position", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    fireEvent.mouseDown(shell, { button: 0 });
    fireEvent.mouseUp(shell, { button: 0 });
    expect(invokeMock).toHaveBeenCalledWith("start_pet_overlay_drag");
    expect(invokeMock).not.toHaveBeenCalledWith("get_pet_overlay_position");
    expect(invokeMock).not.toHaveBeenCalledWith("save_pet_overlay_position");
  });

  /** 右键弹出设置窗口，拦住默认菜单，且不是应用 Settings 页。 */
  test("right_click_opens_overlay_settings_window_not_app_settings", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    const event = createEvent.contextMenu(shell);
    fireEvent(shell, event);
    expect(event.defaultPrevented).toBe(true);
    const dialog = await screen.findByRole("dialog", { name: "Pet overlay settings" });
    expect(dialog).toBeVisible();
    expect(dialog).toHaveAttribute("data-testid", "pet-overlay-settings");
    expect(screen.queryByText("Application information")).not.toBeInTheDocument();
    expect(screen.queryByText("Interface language")).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: /^Settings:/ })).not.toBeInTheDocument();
  });
});
