/** 验证本机性能观测默认关闭、固定载荷、双帧计时及资源回收。 */
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
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

describe("local performance evidence", () => {
  let frames: AnimationFrameHarness;

  beforeEach(() => {
    frames = new AnimationFrameHarness();
    invokeMock.mockReset();
    observerCallback = null;
    observerDisconnect.mockReset();
    vi.stubGlobal("PerformanceObserver", PerformanceObserverHarness);
    vi.stubGlobal("requestAnimationFrame", frames.request);
    vi.stubGlobal("cancelAnimationFrame", frames.cancel);
    window.history.replaceState(null, "", "/dashboard");
  });

  afterEach(() => {
    stopMainPerformanceEvidence();
    vi.unstubAllGlobals();
  });

  /** 未显式启用时只查询状态，不安装 Observer、帧任务或写入。 */
  test("stays fully inactive when the Rust channel is disabled", async () => {
    invokeMock.mockResolvedValue(false);

    await startMainPerformanceEvidence();

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("get_performance_evidence_status");
    expect(observerCallback).toBeNull();
    expect(frames.size()).toBe(0);
  });

  /** 桌宠辅助视图不查询也不启动主窗口观测通道。 */
  test("does not start from a pet auxiliary view", async () => {
    window.history.replaceState(null, "", "/pet");

    await startMainPerformanceEvidence();

    expect(invokeMock).not.toHaveBeenCalled();
    expect(frames.size()).toBe(0);
  });

  /** 启用后按序记录能力、可见主壳和浏览器长任务。 */
  test("records the fixed observer metrics with monotonically increasing sequences", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "get_performance_evidence_status" ? true : undefined,
    );
    const shell = document.createElement("div");
    shell.dataset.testid = "app-shell";
    Object.defineProperty(shell, "getClientRects", {
      value: () => [{ height: 100, width: 100 }],
    });
    const button = document.createElement("button");
    button.dataset.testid = "navigation-label-dashboard";
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
      "long-task",
    ]);
    expect(payloads.map((payload) => payload.sequence)).toEqual([1, 2, 3]);
    expect(payloads[0]).toMatchObject({ longTaskSupported: true });
  });

  /** 证据只接受可信主键点击，脚本合成或辅助按键都不能冒充真实用户交互。 */
  test("accepts only trusted primary clicks", () => {
    expect(isTrustedPrimaryPerformanceClick({ button: 0, isTrusted: true })).toBe(true);
    expect(isTrustedPrimaryPerformanceClick({ button: 0, isTrusted: false })).toBe(false);
    expect(isTrustedPrimaryPerformanceClick({ button: 2, isTrusted: true })).toBe(false);
  });

  /** 非白名单或包含业务语义的测试标识统一退化为 generic。 */
  test("anonymizes interaction targets outside the fixed allowlist", () => {
    const privateElement = document.createElement("button");
    privateElement.dataset.testid = "customer-record-secret";
    const nested = document.createElement("span");
    privateElement.append(nested);
    expect(resolvePerformanceInteractionTarget(nested)).toBe("generic");
    expect(resolvePerformanceInteractionTarget(null)).toBe("generic");
  });

  /** epoch 与相对时间戳都被映射到同一个高精度时间基准。 */
  test("normalizes epoch based event timestamps", () => {
    expect(normalizeEventTimestamp(42)).toBe(42);
    expect(normalizeEventTimestamp(performance.timeOrigin + 42)).toBeCloseTo(42);
  });

  /** 主动关闭会取消未完成帧并断开 Observer。 */
  test("releases all owned browser resources", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "get_performance_evidence_status" ? true : undefined,
    );
    await startMainPerformanceEvidence();
    expect(frames.size()).toBeGreaterThan(0);

    stopMainPerformanceEvidence();

    expect(frames.size()).toBe(0);
    expect(observerDisconnect).toHaveBeenCalledTimes(1);
  });
});
