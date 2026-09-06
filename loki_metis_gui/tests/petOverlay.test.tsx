import {
  act,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import type { PetWindowState } from "../src/api/monitor";
import { INTERFACE_LANGUAGE_STORAGE_KEY } from "../src/lib/language";
import { PetOverlayPage } from "../src/pages/PetOverlayPage";
import {
  INTERFACE_LANGUAGE_CHANGED_EVENT,
  PET_WINDOW_STATE_CHANGED_EVENT,
} from "../src/pages/usePetAuxiliaryWindowSync";
import { monitorImageBytesToDataUrl } from "../src/pet-overlay-image";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);
const pngMagic = [137, 80, 78, 71];
type TestEventHandler = (event: { event: string; id: number; payload: unknown }) => void;
const eventHandlers = new Map<string, TestEventHandler>();
const unlistenMock = vi.fn();

/** 构造四格当前页的位置驱动宿主快照。 */
function petState(overrides: Partial<PetWindowState> = {}): PetWindowState {
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
    ...overrides,
  };
}

/** 安装桌宠测试需要的通用 IPC 应答。 */
function mockPetCommands(
  state: PetWindowState | (() => PetWindowState),
  onCommand?: (command: string, args: unknown) => void,
): void {
  invokeMock.mockImplementation(async (command: string, args?: unknown) => {
    if (command === "get_pet_window_state") {
      return typeof state === "function" ? state() : state;
    }
    if (command === "get_monitor_image_bytes") return pngMagic;
    if (
      [
        "start_pet_overlay_drag",
        "show_pet_settings",
        "show_main_window",
        "turn_pet_page",
        "resize_pet_step",
        "focus_first_populated_pet_page",
      ].includes(command)
    ) {
      onCommand?.(command, args);
      return null;
    }
    throw new Error(`unexpected command ${command}`);
  });
}

describe("desktop pet overlay page", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    eventHandlers.clear();
    unlistenMock.mockReset();
    listenMock.mockImplementation(async (event, handler) => {
      eventHandlers.set(String(event), handler as TestEventHandler);
      return unlistenMock;
    });
    mockPetCommands(petState());
  });

  afterEach(() => {
    document.documentElement.classList.remove("pet-window");
  });

  /** 桌宠主题中间层必须显式铺满窗口，避免百分比高度链在真实 WebView 中断。 */
  test("pet_theme_surface_fills_the_native_window", async () => {
    document.documentElement.classList.add("pet-window");
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );

    expect(await screen.findByTestId("app-theme-surface")).toHaveStyle({
      background: "transparent",
      height: "100%",
    });
  });

  /** 空首页只显示纯位置编号，不带任何预绑定 Agent 名称。 */
  test("empty_first_page_shows_only_positions_01_through_04", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );

    expect(await screen.findByText("01")).toBeInTheDocument();
    expect(screen.getByText("02")).toBeInTheDocument();
    expect(screen.getByText("03")).toBeInTheDocument();
    expect(screen.getByText("04")).toBeInTheDocument();
    expect(screen.queryByText("Waiting for data")).not.toBeInTheDocument();
    expect(screen.queryByText("Codex")).not.toBeInTheDocument();
    expect(screen.queryByText("Claude Code")).not.toBeInTheDocument();
    expect(screen.queryByText("Grok Build")).not.toBeInTheDocument();
    expect(screen.queryByText("WorkBuddy")).not.toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  /** 第三页使用全局纯位置编号 09–12。 */
  test("later_page_keeps_absolute_position_numbers", async () => {
    mockPetCommands(
      petState({
        pageIndex: 2,
        slots: [8, 9, 10, 11].map((slotIndex) => ({ slotIndex, tile: null })),
      }),
    );
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );

    expect(await screen.findByText("09")).toBeInTheDocument();
    expect(screen.getByText("10")).toBeInTheDocument();
    expect(screen.getByText("11")).toBeInTheDocument();
    expect(screen.getByText("12")).toBeInTheDocument();
    expect(screen.getByText("3/3")).toBeInTheDocument();
  });

  /** 仅实际占用位置的 tile 才显示动态 Agent 标签与本机图片。 */
  test("occupied_position_renders_dynamic_tile_and_csp_safe_image", async () => {
    mockPetCommands(
      petState({
        hasAnyImage: true,
        pageHasImage: true,
        slots: [
          {
            slotIndex: 0,
            tile: {
              tool: "codex",
              name: "Codex",
              content: "Running",
              imageId: "img-1",
            },
          },
          ...[1, 2, 3].map((slotIndex) => ({ slotIndex, tile: null })),
        ],
      }),
    );
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );

    const image = await screen.findByRole("img", { name: "01-Codex" });
    expect(image).toHaveAttribute("src", monitorImageBytesToDataUrl(pngMagic));
    expect(screen.getByText("Codex")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(screen.queryByText("01")).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("get_monitor_image_bytes", { id: "img-1" });
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("focus_first_populated_pet_page");
    });
    expect(
      invokeMock.mock.calls.filter(
        ([command]) => command === "focus_first_populated_pet_page",
      ),
    ).toHaveLength(1);
  });

  /** 图片载入失败时仍保留该位置的淡轮廓，不让透明窗口完全消失。 */
  test("missing_image_keeps_a_recoverable_position_outline", async () => {
    const state = petState({
      hasAnyImage: true,
      pageHasImage: true,
      slots: [
        {
          slotIndex: 0,
          tile: {
            tool: "codex",
            name: "Codex",
            content: "Running",
            imageId: "missing-image",
          },
        },
        ...[1, 2, 3].map((slotIndex) => ({ slotIndex, tile: null })),
      ],
    });
    mockPetCommands(state);
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_pet_window_state") return state;
      if (command === "get_monitor_image_bytes") throw new Error("missing image");
      if (command === "focus_first_populated_pet_page") return null;
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );

    await screen.findByText("01");
    await waitFor(() => {
      expect(screen.getByText("01").closest(".pet-tile")).toHaveClass("image-unavailable");
    });
  });

  /** 字节存在但浏览器解码失败时回退编号；换图后由组件 key 清除失败状态。 */
  test("image_decode_failure_falls_back_to_the_position_outline", async () => {
    let state = petState({
      hasAnyImage: true,
      pageHasImage: true,
      slots: [
        {
          slotIndex: 0,
          tile: {
            tool: "codex",
            name: "Codex",
            content: "Running",
            imageId: "corrupted-image",
          },
        },
        ...[1, 2, 3].map((slotIndex) => ({ slotIndex, tile: null })),
      ],
    });
    mockPetCommands(() => state);
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );

    const image = await screen.findByRole("img", { name: "01-Codex" });
    fireEvent.error(image);

    const position = await screen.findByText("01");
    expect(screen.queryByRole("img", { name: "01-Codex" })).not.toBeInTheDocument();
    expect(position.closest(".pet-tile")).toHaveClass("image-unavailable");

    state = {
      ...state,
      slots: state.slots.map((slot) =>
        slot.slotIndex === 0 && slot.tile
          ? { ...slot, tile: { ...slot.tile, imageId: "replacement-image" } }
          : slot,
      ),
    };
    await waitFor(() =>
      expect(eventHandlers.has(PET_WINDOW_STATE_CHANGED_EVENT)).toBe(true),
    );
    await act(async () => {
      eventHandlers.get(PET_WINDOW_STATE_CHANGED_EVENT)?.({
        event: PET_WINDOW_STATE_CHANGED_EVENT,
        id: 4,
        payload: null,
      });
    });

    expect(await screen.findByRole("img", { name: "01-Codex" })).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("get_monitor_image_bytes", {
      id: "replacement-image",
    });
  });

  /** 左键单击可拖动，非左键、双击第二次与控件区域都不触发拖动。 */
  test("left_single_press_drags_except_excluded_inputs", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    await screen.findByText("01");

    fireEvent.mouseDown(shell, { button: 0, detail: 1 });
    expect(invokeMock).toHaveBeenCalledWith("start_pet_overlay_drag");
    invokeMock.mockClear();

    fireEvent.mouseDown(shell, { button: 2, detail: 1 });
    fireEvent.mouseDown(shell, { button: 0, detail: 2 });
    fireEvent.mouseDown(screen.getByLabelText("Next page"), {
      button: 0,
      detail: 1,
    });
    expect(invokeMock).not.toHaveBeenCalledWith("start_pet_overlay_drag");
  });

  /** 锁定窗口时左键不得启动拖动。 */
  test("locked_overlay_does_not_drag", async () => {
    mockPetCommands(petState({ locked: true }));
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    await waitFor(() => expect(shell).toHaveClass("locked"));
    fireEvent.mouseDown(shell, { button: 0, detail: 1 });
    expect(invokeMock).not.toHaveBeenCalledWith("start_pet_overlay_drag");
  });

  /** 右键与键盘菜单键都打开原生设置窗，DOM 中不再渲染内联对话框。 */
  test("context_actions_open_native_settings_without_inline_dialog", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    await screen.findByText("01");
    const contextEvent = createEvent.contextMenu(shell);
    fireEvent(shell, contextEvent);
    expect(contextEvent.defaultPrevented).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("show_pet_settings");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    invokeMock.mockClear();
    fireEvent.keyDown(window, { key: "F10", shiftKey: true });
    expect(invokeMock).toHaveBeenCalledWith("show_pet_settings");
  });

  /** 双击非控件区域返回主界面。 */
  test("double_click_returns_to_main_window", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    fireEvent.doubleClick(shell, { button: 0, detail: 2 });
    expect(invokeMock).toHaveBeenCalledWith("show_main_window");
  });

  /** 滚轮、键盘与悬浮 pager 在宿主动作成功后立即重读并显示新状态。 */
  test("wheel_keyboard_and_pager_refresh_navigation_and_resize_state", async () => {
    let state = petState();
    mockPetCommands(
      () => state,
      (command, args) => {
        if (command === "turn_pet_page") {
          const direction = (args as { direction: "previous" | "next" }).direction;
          const pageIndex = direction === "next" ? 1 : 0;
          state = {
            ...state,
            pageIndex,
            slots: [0, 1, 2, 3].map((offset) => ({
              slotIndex: pageIndex * 4 + offset,
              tile: null,
            })),
          };
        }
        if (command === "resize_pet_step") {
          state = { ...state, petSize: 88 };
        }
      },
    );
    const now = vi.spyOn(Date, "now").mockReturnValue(1000);
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    await screen.findByText("01");

    fireEvent.wheel(shell, { deltaY: 1 });
    expect(invokeMock).toHaveBeenCalledWith("turn_pet_page", { direction: "next" });
    expect(await screen.findByText("05")).toBeInTheDocument();
    expect(screen.getByText("2/3")).toBeInTheDocument();
    now.mockReturnValue(1300);
    fireEvent.wheel(shell, { ctrlKey: true, deltaY: -1 });
    expect(invokeMock).toHaveBeenCalledWith("resize_pet_step", { direction: "grow" });
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(invokeMock).toHaveBeenCalledWith("turn_pet_page", { direction: "previous" });
    expect(await screen.findByText("01")).toBeInTheDocument();

    fireEvent.mouseEnter(shell);
    expect(shell).toHaveClass("hovered");
    await userEvent.click(screen.getByLabelText("Next page"));
    expect(invokeMock).toHaveBeenCalledWith("turn_pet_page", { direction: "next" });
    now.mockRestore();
  });

  /** 原生状态事件无需等待轮询即可刷新纯位置页，并在卸载时解除两个订阅。 */
  test("native_state_event_refetches_immediately_and_unsubscribes", async () => {
    let state = petState();
    mockPetCommands(() => state);
    const mounted = render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    await screen.findByText("01");
    await waitFor(() => {
      expect(eventHandlers.has(PET_WINDOW_STATE_CHANGED_EVENT)).toBe(true);
      expect(eventHandlers.has(INTERFACE_LANGUAGE_CHANGED_EVENT)).toBe(true);
    });

    state = {
      ...state,
      pageIndex: 1,
      slots: [4, 5, 6, 7].map((slotIndex) => ({ slotIndex, tile: null })),
    };
    await act(async () => {
      eventHandlers.get(PET_WINDOW_STATE_CHANGED_EVENT)?.({
        event: PET_WINDOW_STATE_CHANGED_EVENT,
        id: 1,
        payload: null,
      });
    });
    expect(await screen.findByText("05")).toBeInTheDocument();

    mounted.unmount();
    expect(unlistenMock).toHaveBeenCalledTimes(2);
  });

  /** 订阅建立后主动重读，封闭首次查询与异步 listen 之间的丢事件窗口。 */
  test("state_subscription_reconciles_changes_that_happen_before_listen_resolves", async () => {
    let state = petState();
    let finishStateListener: (() => void) | undefined;
    mockPetCommands(() => state);
    listenMock.mockImplementation((event, handler) => {
      eventHandlers.set(String(event), handler as TestEventHandler);
      if (String(event) === PET_WINDOW_STATE_CHANGED_EVENT) {
        return new Promise((resolve) => {
          finishStateListener = () => resolve(unlistenMock);
        });
      }
      return Promise.resolve(unlistenMock);
    });
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    expect(await screen.findByText("01")).toBeInTheDocument();
    await waitFor(() => expect(finishStateListener).toBeTypeOf("function"));

    state = {
      ...state,
      pageIndex: 1,
      slots: [4, 5, 6, 7].map((slotIndex) => ({ slotIndex, tile: null })),
    };
    await act(async () => {
      finishStateListener?.();
    });

    expect(await screen.findByText("05")).toBeInTheDocument();
  });

  /** 首次查询未完成时 listen-ready/event 复用旧 Promise，settle 后仍必须真正再读。 */
  test("state_reconcile_repeats_after_an_in_flight_initial_fetch", async () => {
    const initialState = petState();
    let currentState = initialState;
    let getCount = 0;
    let finishInitialFetch: (() => void) | undefined;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_pet_window_state") {
        getCount += 1;
        if (getCount === 1) {
          return new Promise<PetWindowState>((resolve) => {
            finishInitialFetch = () => resolve(initialState);
          });
        }
        return currentState;
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    await waitFor(() => {
      expect(finishInitialFetch).toBeTypeOf("function");
      expect(eventHandlers.has(PET_WINDOW_STATE_CHANGED_EVENT)).toBe(true);
    });

    currentState = {
      ...initialState,
      pageIndex: 1,
      slots: [4, 5, 6, 7].map((slotIndex) => ({ slotIndex, tile: null })),
    };
    eventHandlers.get(PET_WINDOW_STATE_CHANGED_EVENT)?.({
      event: PET_WINDOW_STATE_CHANGED_EVENT,
      id: 5,
      payload: null,
    });
    await act(async () => {
      finishInitialFetch?.();
    });

    expect(await screen.findByText("05")).toBeInTheDocument();
    expect(getCount).toBeGreaterThanOrEqual(2);
  });

  /** 状态事件撞上组件动作发起的 refetch 时，也必须在旧请求后再取新快照。 */
  test("state_event_reconciles_after_an_in_flight_action_refetch", async () => {
    const oldState = petState();
    let currentState = oldState;
    let deferNextGet = false;
    let finishActionFetch: (() => void) | undefined;
    let getCount = 0;
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_pet_window_state") {
        getCount += 1;
        if (deferNextGet) {
          deferNextGet = false;
          return new Promise<PetWindowState>((resolve) => {
            finishActionFetch = () => resolve(oldState);
          });
        }
        return currentState;
      }
      if (command === "turn_pet_page") {
        deferNextGet = true;
        return null;
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    await screen.findByText("01");
    await waitFor(() =>
      expect(eventHandlers.has(PET_WINDOW_STATE_CHANGED_EVENT)).toBe(true),
    );
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    const readsBeforeAction = getCount;

    fireEvent.mouseEnter(shell);
    await userEvent.click(screen.getByLabelText("Next page"));
    await waitFor(() => expect(finishActionFetch).toBeTypeOf("function"));
    currentState = {
      ...oldState,
      pageIndex: 1,
      slots: [4, 5, 6, 7].map((slotIndex) => ({ slotIndex, tile: null })),
    };
    eventHandlers.get(PET_WINDOW_STATE_CHANGED_EVENT)?.({
      event: PET_WINDOW_STATE_CHANGED_EVENT,
      id: 6,
      payload: null,
    });
    await act(async () => {
      finishActionFetch?.();
    });

    expect(await screen.findByText("05")).toBeInTheDocument();
    expect(getCount).toBeGreaterThanOrEqual(readsBeforeAction + 2);
  });

  /** 原生语言事件让辅助 WebView 与主窗口即时同步。 */
  test("native_language_event_updates_auxiliary_window_copy", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    expect(shell).toHaveAccessibleName("Desktop pet overlay, page 1 of 3");
    await waitFor(() => {
      expect(eventHandlers.has(INTERFACE_LANGUAGE_CHANGED_EVENT)).toBe(true);
    });

    await act(async () => {
      eventHandlers.get(INTERFACE_LANGUAGE_CHANGED_EVENT)?.({
        event: INTERFACE_LANGUAGE_CHANGED_EVENT,
        id: 2,
        payload: "zh-CN",
      });
    });
    expect(shell).toHaveAccessibleName("桌宠悬浮窗，第 1 / 3 页");
  });

  /** 主 WebView 写入真实语言键后，跨窗 storage 事件同样同步文案。 */
  test("storage_language_event_uses_the_shared_interface_key", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    expect(shell).toHaveAccessibleName("Desktop pet overlay, page 1 of 3");

    fireEvent(
      window,
      new StorageEvent("storage", {
        key: INTERFACE_LANGUAGE_STORAGE_KEY,
        newValue: "zh-CN",
      }),
    );
    expect(shell).toHaveAccessibleName("桌宠悬浮窗，第 1 / 3 页");
  });

  /** 语言订阅建立后重读存储，封闭主窗切换发生在 listen 就绪前的竞态。 */
  test("language_subscription_reconciles_storage_after_listen_resolves", async () => {
    let finishLanguageListener: (() => void) | undefined;
    listenMock.mockImplementation((event, handler) => {
      eventHandlers.set(String(event), handler as TestEventHandler);
      if (String(event) === INTERFACE_LANGUAGE_CHANGED_EVENT) {
        return new Promise((resolve) => {
          finishLanguageListener = () => resolve(unlistenMock);
        });
      }
      return Promise.resolve(unlistenMock);
    });
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    expect(shell).toHaveAccessibleName("Desktop pet overlay, page 1 of 3");
    await waitFor(() => expect(finishLanguageListener).toBeTypeOf("function"));

    window.localStorage.setItem(INTERFACE_LANGUAGE_STORAGE_KEY, "zh-CN");
    await act(async () => {
      finishLanguageListener?.();
    });

    expect(shell).toHaveAccessibleName("桌宠悬浮窗，第 1 / 3 页");
  });

  /** 键盘聚焦浮窗时 pager 进入 Tab 路径，鼠标悬浮不是唯一入口。 */
  test("keyboard_focus_exposes_pager_controls", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const shell = await screen.findByTestId("pet-overlay-page");
    const next = screen.getByTitle("Next page");
    expect(next).toHaveAttribute("tabindex", "-1");

    fireEvent.focus(shell);
    expect(shell).toHaveClass("keyboard-focused");
    expect(next).toHaveAttribute("tabindex", "0");
    expect(next.parentElement).toHaveAttribute("aria-hidden", "false");

    fireEvent(window, new Event("blur"));
    expect(shell).not.toHaveClass("keyboard-focused");
    expect(next).toHaveAttribute("tabindex", "-1");
    expect(next.parentElement).toHaveAttribute("aria-hidden", "true");
  });
});
