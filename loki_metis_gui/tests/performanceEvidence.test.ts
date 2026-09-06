/** 验证本机性能观测默认关闭、固定载荷、双帧计时及资源回收。 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
const windowListenMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ listen: windowListenMock }),
}));

import {
  expectedPerformanceInteractionResult,
  finishMainPerformanceEvidence,
  isPetAuxiliaryPerformanceEntry,
  isTrustedPrimaryPerformanceClick,
  normalizeEventTimestamp,
  resolvePerformanceInteractionTarget,
  startMainPerformanceEvidence,
  stopMainPerformanceEvidence,
} from "../src/lib/performanceEvidence";

/** 模拟浏览器帧队列，允许测试逐帧驱动双 rAF 协议。 */
class AnimationFrameHarness {
  private nextId = 1;
  private readonly callbacks = new Map<number, FrameRequestCallback>();

  /** 保存一条待执行帧回调并返回其可取消 ID。 */
  request = (callback: FrameRequestCallback): number => {
    const id = this.nextId;
    this.nextId += 1;
    this.callbacks.set(id, callback);
    return id;
  };

  /** 移除尚未执行的帧回调。 */
  cancel = (id: number): void => {
    this.callbacks.delete(id);
  };

  /** 执行当前批次的所有回调，新注册回调留到下一帧。 */
  flush(time: number): void {
    const callbacks = [...this.callbacks.values()];
    this.callbacks.clear();
    for (const callback of callbacks) callback(time);
  }

  /** 返回仍由会话持有的帧数量。 */
  size(): number {
    return this.callbacks.size;
  }
}

let observerCallback: PerformanceObserverCallback | null = null;
const observerDisconnect = vi.fn();
let nativeVisibilityHandler: ((event: { payload: boolean }) => void) | null = null;
const nativeVisibilityUnlisten = vi.fn();

/** 提供支持 longtask 的最小 PerformanceObserver 宿主。 */
class PerformanceObserverHarness implements PerformanceObserver {
  static readonly supportedEntryTypes = ["longtask"];

  /** 保存生产代码注册的 Observer 回调。 */
  constructor(callback: PerformanceObserverCallback) {
    observerCallback = callback;
  }

  /** 测试宿主不需要维护 observe 参数。 */
  observe(): void {}

  /** 记录会话是否正确回收 Observer。 */
  disconnect(): void {
    observerDisconnect();
  }

  /** 测试宿主没有额外缓存记录。 */
  takeRecords(): PerformanceEntryList {
    return [];
  }
}

/** 构造只返回指定条目的 PerformanceObserverEntryList。 */
function entryList(entries: PerformanceEntry[]): PerformanceObserverEntryList {
  return {
    getEntries: () => entries,
    getEntriesByName: (name: string) => entries.filter((entry) => entry.name === name),
    getEntriesByType: (type: string) => entries.filter((entry) => entry.entryType === type),
  };
}

/** 等待串行 IPC Promise 链完成当前微任务。 */
async function flushPromises(): Promise<void> {
  for (let step = 0; step < 12; step += 1) await Promise.resolve();
}

/** 创建参与布局的测试元素，避免 JSDOM 的空矩形被判为隐藏。 */
function appendVisibleTestElement(testId: string, parent: HTMLElement): HTMLElement {
  const element = document.createElement("div");
  element.dataset.testid = testId;
  Object.defineProperty(element, "getClientRects", {
    value: () => [{ height: 100, width: 100 }],
  });
  parent.append(element);
  return element;
}

/** 创建可见主壳并返回它，供启动就绪帧与导航控件共用。 */
function appendVisibleMainShell(): HTMLElement {
  return appendVisibleTestElement("app-shell", document.body);
}

/** 捕获性能模块注册的 click listener，允许以可信宿主事件载荷直接驱动。 */
function capturePerformanceClickHandler(): () => EventListener {
  let clickHandler: EventListener | null = null;
  const addEventListener = document.addEventListener.bind(document);
  vi.spyOn(document, "addEventListener").mockImplementation((type, listener, options) => {
    if (type === "click" && typeof listener === "function") clickHandler = listener;
    addEventListener(type, listener, options);
  });
  return () => {
    if (clickHandler === null)
      throw new Error("performance click handler was not registered");
    return clickHandler;
  };
}

/** 驱动主壳就绪所需三帧，并确认启动帧任务已经排空。 */
function flushMainReadyFrames(frames: AnimationFrameHarness): void {
  frames.flush(10);
  frames.flush(20);
  frames.flush(30);
  expect(frames.size()).toBe(0);
}

describe("local performance evidence", () => {
  let frames: AnimationFrameHarness;

  beforeEach(() => {
    frames = new AnimationFrameHarness();
    invokeMock.mockReset();
    windowListenMock.mockReset();
    nativeVisibilityHandler = null;
    nativeVisibilityUnlisten.mockReset();
    windowListenMock.mockImplementation(
      async (_event: string, handler: (event: { payload: boolean }) => void) => {
        nativeVisibilityHandler = handler;
        return nativeVisibilityUnlisten;
      },
    );
    observerCallback = null;
    observerDisconnect.mockReset();
    vi.stubGlobal("PerformanceObserver", PerformanceObserverHarness);
    vi.stubGlobal("requestAnimationFrame", frames.request);
    vi.stubGlobal("cancelAnimationFrame", frames.cancel);
    Object.defineProperty(window, "__LOKI_METIS_PERFORMANCE_EVIDENCE__", {
      configurable: true,
      value: true,
    });
    window.history.replaceState(null, "", "/dashboard");
  });

  afterEach(() => {
    stopMainPerformanceEvidence();
    document.body.replaceChildren();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  /** 未显式启用时同步返回，不安装 Observer、帧任务或发起 IPC。 */
  test("stays fully inactive when the Rust channel is disabled", async () => {
    Object.defineProperty(window, "__LOKI_METIS_PERFORMANCE_EVIDENCE__", {
      configurable: true,
      value: undefined,
    });

    await startMainPerformanceEvidence();

    expect(invokeMock).not.toHaveBeenCalled();
    expect(windowListenMock).not.toHaveBeenCalled();
    expect(observerCallback).toBeNull();
    expect(frames.size()).toBe(0);
  });

  /** 桌宠辅助视图在路由改写前后都不启动主窗口观测通道。 */
  test.each([
    "/pet",
    "/pet-settings",
    "/index.html?view=pet",
    "/index.html?view=pet-settings",
  ])("does not start from the pet auxiliary entry %s", async (entry) => {
    window.history.replaceState(null, "", entry);

    expect(isPetAuxiliaryPerformanceEntry(window.location)).toBe(true);
    await startMainPerformanceEvidence();

    expect(invokeMock).not.toHaveBeenCalled();
    expect(windowListenMock).not.toHaveBeenCalled();
    expect(frames.size()).toBe(0);
  });

  /** 启用后按序记录能力、可见主壳和浏览器长任务。 */
  test("records the fixed observer metrics with monotonically increasing sequences", async () => {
    invokeMock.mockResolvedValue(undefined);
    const shell = document.createElement("div");
    shell.dataset.testid = "app-shell";
    Object.defineProperty(shell, "getClientRects", {
      value: () => [{ height: 100, width: 100 }],
    });
    const button = document.createElement("button");
    button.dataset.testid = "navigation-dashboard";
    shell.append(button);
    document.body.append(shell);

    await startMainPerformanceEvidence();
    await flushPromises();
    frames.flush(16);
    frames.flush(32);
    frames.flush(48);
    await flushPromises();

    const longTask = {
      duration: 75,
      entryType: "longtask",
      name: "self",
      startTime: 20,
      toJSON: () => ({}),
    } satisfies PerformanceEntry;
    observerCallback?.(entryList([longTask]), {} as PerformanceObserver);
    await flushPromises();

    const payloads = invokeMock.mock.calls
      .filter(([command]) => command === "record_performance_evidence")
      .map(([, arguments_]) => arguments_.payload);
    expect(payloads.map((payload) => payload.kind)).toEqual([
      "renderer-capabilities",
      "main-window-ready",
      "renderer-blocking-interval",
    ]);
    expect(payloads.every((payload) => payload.sequence === undefined)).toBe(true);
    expect(payloads[0]).toMatchObject({
      longTaskSupported: true,
      timingSource: "performance-observer",
    });
  });

  /** 证据只接受可信主键点击，脚本合成或辅助按键都不能冒充真实用户交互。 */
  test("accepts only trusted primary clicks", () => {
    expect(isTrustedPrimaryPerformanceClick({ button: 0, isTrusted: true })).toBe(true);
    expect(isTrustedPrimaryPerformanceClick({ button: 0, isTrusted: false })).toBe(false);
    expect(isTrustedPrimaryPerformanceClick({ button: 2, isTrusted: true })).toBe(false);
  });

  /** 非白名单目标直接忽略，固定导航动作只能对应固定结果页。 */
  test("ignores targets outside the fixed navigation allowlist", () => {
    const privateElement = document.createElement("button");
    privateElement.dataset.testid = "customer-record-secret";
    const nested = document.createElement("span");
    privateElement.append(nested);
    expect(resolvePerformanceInteractionTarget(nested)).toBeNull();
    expect(resolvePerformanceInteractionTarget(null)).toBeNull();

    const navigation = document.createElement("button");
    navigation.dataset.testid = "navigation-monitor";
    expect(resolvePerformanceInteractionTarget(navigation)).toBe("navigation-monitor");
    expect(expectedPerformanceInteractionResult("navigation-monitor")).toBe("monitor-page");
    expect(expectedPerformanceInteractionResult("customer-record-secret")).toBeNull();
  });

  /** WebKit 回退只声明能力，主窗口空闲时不能留下常驻逐帧循环。 */
  test("keeps the animation-frame fallback idle outside navigation interactions", async () => {
    /** 模拟不声明 longtask entry 的 WebKit PerformanceObserver。 */
    class UnsupportedObserver extends PerformanceObserverHarness {
      static readonly supportedEntryTypes: string[] = [];
    }
    vi.stubGlobal("PerformanceObserver", UnsupportedObserver);
    invokeMock.mockResolvedValue(undefined);
    appendVisibleMainShell();

    await startMainPerformanceEvidence();
    await flushPromises();
    flushMainReadyFrames(frames);
    await flushPromises();

    const payloads = invokeMock.mock.calls
      .filter(([command]) => command === "record_performance_evidence")
      .map(([, arguments_]) => arguments_.payload);
    expect(payloads[0]).toMatchObject({
      kind: "renderer-capabilities",
      longTaskSupported: false,
      timingSource: "animation-frame-gap",
    });
    expect(
      payloads.filter((payload) => payload.kind === "renderer-blocking-interval"),
    ).toEqual([]);
    expect(frames.size()).toBe(0);
  });

  /** 可信导航点击开启回退帧窗口，并在结果连续两帧可见后记录阻塞且立即释放。 */
  test("samples frame gaps only during a trusted navigation interaction", async () => {
    /** 模拟不声明 longtask entry 的 WebKit PerformanceObserver。 */
    class UnsupportedObserver extends PerformanceObserverHarness {
      static readonly supportedEntryTypes: string[] = [];
    }
    vi.stubGlobal("PerformanceObserver", UnsupportedObserver);
    invokeMock.mockResolvedValue(undefined);
    const shell = appendVisibleMainShell();
    const navigation = document.createElement("button");
    navigation.dataset.testid = "navigation-monitor";
    shell.append(navigation);
    const getClickHandler = capturePerformanceClickHandler();

    await startMainPerformanceEvidence();
    await flushPromises();
    flushMainReadyFrames(frames);

    getClickHandler()({
      button: 0,
      isTrusted: true,
      target: navigation,
      timeStamp: 40,
    } as unknown as Event);
    expect(frames.size()).toBe(1);
    window.history.replaceState(null, "", "/monitor");
    appendVisibleTestElement("monitor-page", document.body);
    frames.flush(100);
    expect(frames.size()).toBe(1);
    frames.flush(120);
    await flushPromises();

    const payloads = invokeMock.mock.calls
      .filter(([command]) => command === "record_performance_evidence")
      .map(([, arguments_]) => arguments_.payload);
    expect(payloads).toContainEqual({
      durationMs: 60,
      kind: "renderer-blocking-interval",
      startTimeMs: 40,
      timingSource: "animation-frame-gap",
    });
    expect(payloads).toContainEqual(
      expect.objectContaining({
        kind: "interaction",
        result: "monitor-page",
        target: "navigation-monitor",
      }),
    );
    expect(frames.size()).toBe(0);
  });

  /** 本机窗口隐藏会终止交互帧窗口，恢复后不会自行重启常驻采样。 */
  test("stops frame-gap sampling when the native window is hidden", async () => {
    /** 模拟不声明 longtask entry 的 WebKit PerformanceObserver。 */
    class UnsupportedObserver extends PerformanceObserverHarness {
      static readonly supportedEntryTypes: string[] = [];
    }
    vi.stubGlobal("PerformanceObserver", UnsupportedObserver);
    invokeMock.mockResolvedValue(undefined);
    const shell = appendVisibleMainShell();
    const navigation = document.createElement("button");
    navigation.dataset.testid = "navigation-settings";
    shell.append(navigation);
    const getClickHandler = capturePerformanceClickHandler();

    await startMainPerformanceEvidence();
    flushMainReadyFrames(frames);
    getClickHandler()({
      button: 0,
      isTrusted: true,
      target: navigation,
      timeStamp: 40,
    } as unknown as Event);
    expect(frames.size()).toBe(1);
    nativeVisibilityHandler?.({ payload: false });
    expect(frames.size()).toBe(0);
    frames.flush(1_000);
    nativeVisibilityHandler?.({ payload: true });
    frames.flush(1_100);
    frames.flush(1_120);
    await flushPromises();

    const blockingPayloads = invokeMock.mock.calls
      .filter(
        ([command, arguments_]) =>
          command === "record_performance_evidence" &&
          arguments_.payload.kind === "renderer-blocking-interval",
      )
      .map(([, arguments_]) => arguments_.payload);
    expect(blockingPayloads).toEqual([]);
    expect(frames.size()).toBe(0);
  });

  /** 目标页始终未出现时，有界超时也必须取消下一帧并停止采样。 */
  test("stops frame-gap sampling when the navigation window times out", async () => {
    /** 模拟不声明 longtask entry 的 WebKit PerformanceObserver。 */
    class UnsupportedObserver extends PerformanceObserverHarness {
      static readonly supportedEntryTypes: string[] = [];
    }
    vi.stubGlobal("PerformanceObserver", UnsupportedObserver);
    invokeMock.mockResolvedValue(undefined);
    const shell = appendVisibleMainShell();
    const navigation = document.createElement("button");
    navigation.dataset.testid = "navigation-monitor";
    shell.append(navigation);
    const getClickHandler = capturePerformanceClickHandler();

    await startMainPerformanceEvidence();
    flushMainReadyFrames(frames);
    const timeoutCallbacks: Array<() => void> = [];
    vi.spyOn(window, "setTimeout").mockImplementation((handler) => {
      if (typeof handler !== "function") throw new Error("unexpected string timer");
      timeoutCallbacks.push(handler as () => void);
      return 42;
    });
    vi.spyOn(window, "clearTimeout").mockImplementation(() => undefined);

    getClickHandler()({
      button: 0,
      isTrusted: true,
      target: navigation,
      timeStamp: 40,
    } as unknown as Event);
    expect(frames.size()).toBe(1);
    const timeoutCallback = timeoutCallbacks[0];
    if (timeoutCallback === undefined)
      throw new Error("interaction timeout was not registered");
    timeoutCallback();

    expect(frames.size()).toBe(0);
  });

  /** finalize 会直接释放尚未完成的有界交互，不把退出等待时间冒充阻塞。 */
  test("releases an active frame-gap window before finalization", async () => {
    /** 模拟不声明 longtask entry 的 WebKit PerformanceObserver。 */
    class UnsupportedObserver extends PerformanceObserverHarness {
      static readonly supportedEntryTypes: string[] = [];
    }
    vi.stubGlobal("PerformanceObserver", UnsupportedObserver);
    invokeMock.mockImplementation(async (command: string) =>
      command === "finish_performance_evidence"
        ? { finalSequence: 3, recordCount: 2 }
        : undefined,
    );
    const shell = appendVisibleMainShell();
    const navigation = document.createElement("button");
    navigation.dataset.testid = "navigation-settings";
    shell.append(navigation);
    const getClickHandler = capturePerformanceClickHandler();

    await startMainPerformanceEvidence();
    flushMainReadyFrames(frames);
    getClickHandler()({
      button: 0,
      isTrusted: true,
      target: navigation,
      timeStamp: 40,
    } as unknown as Event);
    expect(frames.size()).toBe(1);
    await finishMainPerformanceEvidence();

    const payloads = invokeMock.mock.calls
      .filter(([command]) => command === "record_performance_evidence")
      .map(([, arguments_]) => arguments_.payload);
    expect(
      payloads.filter((payload) => payload.kind === "renderer-blocking-interval"),
    ).toEqual([]);
    expect(frames.size()).toBe(0);
  });

  /** 结束握手先排空指标队列，再调用 Rust finalize，且重复调用不会重复结束。 */
  test("flushes the queue before an idempotent Rust finalization", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "finish_performance_evidence") {
        return { finalSequence: 2, recordCount: 1 };
      }
      return undefined;
    });

    await startMainPerformanceEvidence();
    const first = finishMainPerformanceEvidence();
    const second = finishMainPerformanceEvidence();
    await expect(first).resolves.toEqual({ finalSequence: 2, recordCount: 1 });
    await expect(second).resolves.toEqual({ finalSequence: 2, recordCount: 1 });

    const commands = invokeMock.mock.calls.map(([command]) => command);
    expect(commands).toEqual([
      "record_performance_evidence",
      "finish_performance_evidence",
    ]);
    expect(
      document.querySelector(`[data-testid="performance-evidence-finalize"]`),
    ).toHaveTextContent("Performance evidence finalized");
  });

  /** 活跃会话重复启动保持幂等，不重建前端序号或第二个 Observer。 */
  test("keeps repeated starts idempotent", async () => {
    invokeMock.mockResolvedValue(undefined);
    await startMainPerformanceEvidence();
    await startMainPerformanceEvidence();

    expect(windowListenMock).toHaveBeenCalledTimes(1);
  });

  /** epoch 与相对时间戳都被映射到同一个高精度时间基准。 */
  test("normalizes epoch based event timestamps", () => {
    expect(normalizeEventTimestamp(42)).toBe(42);
    expect(normalizeEventTimestamp(performance.timeOrigin + 42)).toBeCloseTo(42);
  });

  /** 主动关闭会取消未完成帧并断开 Observer。 */
  test("releases all owned browser resources", async () => {
    invokeMock.mockResolvedValue(undefined);
    await startMainPerformanceEvidence();
    expect(frames.size()).toBeGreaterThan(0);

    stopMainPerformanceEvidence();

    expect(frames.size()).toBe(0);
    expect(observerDisconnect).toHaveBeenCalledTimes(1);
    expect(nativeVisibilityUnlisten).toHaveBeenCalledTimes(1);
  });
});
