import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import type { PetWindowState } from "../src/api/monitor";
import { PetSettingsPage } from "../src/pages/PetSettingsPage";
import {
  INTERFACE_LANGUAGE_CHANGED_EVENT,
  PET_WINDOW_STATE_CHANGED_EVENT,
} from "../src/pages/usePetAuxiliaryWindowSync";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);
type TestEventHandler = (event: { event: string; id: number; payload: unknown }) => void;
const eventHandlers = new Map<string, TestEventHandler>();
const unlistenMock = vi.fn();

/** 构造桌宠设置窗使用的宿主快照。 */
function settingsState(): PetWindowState {
  return {
    layout: "grid",
    locked: false,
    pageIndex: 0,
    pageCount: 3,
    pageHasImage: false,
    hasAnyImage: false,
    slots: [0, 1, 2, 3].map((slotIndex) => ({ slotIndex, tile: null })),
    petSize: 64,
    sizeMin: 32,
    sizeMax: 180,
    alwaysOnTop: true,
  };
}

describe("desktop pet settings window", () => {
  let state: PetWindowState;

  beforeEach(() => {
    state = settingsState();
    invokeMock.mockReset();
    listenMock.mockReset();
    eventHandlers.clear();
    unlistenMock.mockReset();
    listenMock.mockImplementation(async (event, handler) => {
      eventHandlers.set(String(event), handler as TestEventHandler);
      return unlistenMock;
    });
    invokeMock.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "get_pet_window_state") return state;
      if (command === "set_pet_layout") {
        state = { ...state, layout: (args as { layout: PetWindowState["layout"] }).layout };
        return null;
      }
      if (command === "set_pet_size") {
        state = { ...state, petSize: (args as { size: number }).size };
        return null;
      }
      if (command === "set_pet_always_on_top") {
        state = { ...state, alwaysOnTop: (args as { enabled: boolean }).enabled };
        return null;
      }
      if (command === "set_pet_locked") {
        state = { ...state, locked: (args as { locked: boolean }).locked };
        return null;
      }
      if (
        ["hide_pet_settings", "show_main_window", "close_pet_overlay"].includes(command)
      ) {
        return null;
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  afterEach(() => {
    document.documentElement.classList.remove("pet-settings-window");
  });

  /** 设置辅助窗主题中间层铺满视口并固定暗色，不受主窗亮色主题影响。 */
  test("settings_theme_surface_fills_viewport_with_fixed_background", async () => {
    document.documentElement.classList.add("pet-settings-window");
    render(
      <TestProviders>
        <PetSettingsPage />
      </TestProviders>,
    );

    expect(await screen.findByTestId("app-theme-surface")).toHaveStyle({
      background: "#11151d",
      height: "100%",
    });
  });

  /** 独立设置窗只保留六布局、尺寸和窗口行为控件。 */
  test("renders_compact_benchmark_controls_without_inline_help", async () => {
    render(
      <TestProviders>
        <PetSettingsPage />
      </TestProviders>,
    );

    expect(await screen.findByTestId("pet-settings-page")).toBeVisible();
    expect(screen.getByText("Desktop pet settings")).toBeVisible();
    const layoutGroup = screen.getByRole("group", { name: "Desktop pet layout" });
    for (const label of ["1×1", "1×2", "2×1", "1×3", "3×1", "2×2"]) {
      expect(within(layoutGroup).getByRole("button", { name: label })).toBeVisible();
    }
    expect(within(layoutGroup).getByRole("button", { name: "2×2" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("slider", { name: "Desktop pet size" })).toHaveValue("64");
    expect(
      screen.getByText("Size is limited automatically for the current display"),
    ).toBeVisible();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/This is not the app Settings page/i),
    ).not.toBeInTheDocument();
  });

  /** 布局、尺寸、置顶与锁定动作成功后立即刷新真实宿主状态。 */
  test("preference_controls_refresh_the_confirmed_native_state", async () => {
    render(
      <TestProviders>
        <PetSettingsPage />
      </TestProviders>,
    );
    const layoutGroup = await screen.findByRole("group", { name: "Desktop pet layout" });

    await userEvent.click(within(layoutGroup).getByRole("button", { name: "1×2" }));
    expect(invokeMock).toHaveBeenCalledWith("set_pet_layout", { layout: "row" });
    await waitFor(() => {
      expect(within(layoutGroup).getByRole("button", { name: "1×2" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });

    fireEvent.change(screen.getByRole("slider", { name: "Desktop pet size" }), {
      target: { value: "80" },
    });
    expect(invokeMock).toHaveBeenCalledWith("set_pet_size", { size: 80 });
    await waitFor(() => {
      expect(screen.getByRole("slider", { name: "Desktop pet size" })).toHaveValue("80");
    });

    await userEvent.click(screen.getByRole("switch", { name: "Always on top" }));
    expect(invokeMock).toHaveBeenCalledWith("set_pet_always_on_top", { enabled: false });
    await waitFor(() => {
      expect(screen.getByRole("switch", { name: "Always on top" })).toHaveAttribute(
        "aria-checked",
        "false",
      );
    });

    await userEvent.click(screen.getByRole("switch", { name: "Lock position and size" }));
    expect(invokeMock).toHaveBeenCalledWith("set_pet_locked", { locked: true });
    await waitFor(() => {
      expect(
        screen.getByRole("switch", { name: "Lock position and size" }),
      ).toHaveAttribute("aria-checked", "true");
    });
  });

  /** 设置窗同样订阅状态事件，并在卸载时解除状态和语言订阅。 */
  test("settings_state_event_refetches_and_all_listeners_are_cleaned_up", async () => {
    const mounted = render(
      <TestProviders>
        <PetSettingsPage />
      </TestProviders>,
    );
    expect(await screen.findByRole("slider", { name: "Desktop pet size" })).toHaveValue(
      "64",
    );
    await waitFor(() => {
      expect(eventHandlers.has(PET_WINDOW_STATE_CHANGED_EVENT)).toBe(true);
      expect(eventHandlers.has(INTERFACE_LANGUAGE_CHANGED_EVENT)).toBe(true);
    });

    state = { ...state, petSize: 92 };
    eventHandlers.get(PET_WINDOW_STATE_CHANGED_EVENT)?.({
      event: PET_WINDOW_STATE_CHANGED_EVENT,
      id: 3,
      payload: null,
    });
    await waitFor(() => {
      expect(screen.getByRole("slider", { name: "Desktop pet size" })).toHaveValue("92");
    });

    mounted.unmount();
    expect(unlistenMock).toHaveBeenCalledTimes(2);
  });

  /** 关闭、返回主界面与隐藏到托盘均交给原生窗口命令。 */
  test("window_actions_delegate_to_native_host", async () => {
    render(
      <TestProviders>
        <PetSettingsPage />
      </TestProviders>,
    );
    await screen.findByTestId("pet-settings-page");

    await userEvent.click(
      screen.getByRole("button", { name: "Close desktop pet settings" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("hide_pet_settings");
    await userEvent.click(screen.getByRole("button", { name: "Return to main window" }));
    expect(invokeMock).toHaveBeenCalledWith("show_main_window");
    await userEvent.click(screen.getByRole("button", { name: "Hide to system tray" }));
    expect(invokeMock).toHaveBeenCalledWith("close_pet_overlay");

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_pet_window_state");
    });
  });
});
