/**
 * 为显式本机发布验收采集无隐私浏览器性能指标。
 *
 * 正常启动只读取 Rust 在 WebView 初始化阶段注入的默认关闭标记；只有
 * 显式启用的测试进程才安装观测器、帧回调和结束握手控件。所有载荷仍由
 * Rust 重新校验并写入私有临时文件。
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { isPetWindowPath } from "../default-landing";

const MAX_MONOTONIC_TIME_MS = 604_800_000;
const MAX_WALL_TIME_MS = 10_000_000_000_000;
const MAX_DURATION_MS = 60_000;
const MIN_BLOCKING_INTERVAL_MS = 50;
const INTERACTION_WINDOW_TIMEOUT_MS = 5_000;
const FINALIZE_BUTTON_TEST_ID = "performance-evidence-finalize";
const PERFORMANCE_WINDOW_VISIBILITY_EVENT = "loki-metis-performance-window-visibility";

declare global {
  /** 声明 Rust 只在显式本机性能验收进程中注入的同步标记。 */
  interface Window {
    /** Rust 只在显式本机性能验收进程中注入的只读启用标记。 */
    readonly __LOKI_METIS_PERFORMANCE_EVIDENCE__?: true;
  }
}

const PERFORMANCE_TARGET_RESULTS: ReadonlyMap<string, string> = new Map([
  ["navigation-dashboard", "dashboard-page"],
  ["navigation-monitor", "monitor-page"],
  ["navigation-settings", "settings-page"],
] as const);

/** 声明当前性能测试实际采用的渲染阻塞计时来源。 */
export type RendererTimingSource = "animation-frame-gap" | "performance-observer";

/** 描述浏览器原生能力与本次实际使用的渲染阻塞观测源。 */
interface RendererCapabilitiesMetric {
  kind: "renderer-capabilities";
  longTaskSupported: boolean;
  timingSource: RendererTimingSource;
}

/** 描述主壳可见并经过双帧稳定后的就绪时间。 */
interface MainWindowReadyMetric {
  kind: "main-window-ready";
  wallTimeMs: number;
  monotonicTimeMs: number;
}

/** 描述真实导航、目标页面可见和双帧完成之间的渲染耗时。 */
interface InteractionMetric {
  kind: "interaction";
  target: string;
  result: string;
  durationMs: number;
}

/** 描述由声明观测源报告的主线程阻塞区间。 */
interface RendererBlockingIntervalMetric {
  kind: "renderer-blocking-interval";
  timingSource: RendererTimingSource;
  startTimeMs: number;
  durationMs: number;
}

/** 前端唯一允许提交给本机证据命令的四类载荷；序号只由 Rust 分配。 */
type PerformanceEvidenceMetric =
  | RendererCapabilitiesMetric
  | MainWindowReadyMetric
  | InteractionMetric
  | RendererBlockingIntervalMetric;

/** Rust 完成同步落盘后返回的固定摘要。 */
export interface PerformanceEvidenceFinalization {
  finalSequence: number;
  recordCount: number;
}

/** 保存当前页面唯一性能测试会话的结束与释放操作。 */
interface ActivePerformanceSession {
  finish: () => Promise<PerformanceEvidenceFinalization>;
  stop: () => void;
}

/** 判断数值有限且处于闭区间，避免无效浏览器数据触发 IPC。 */
function isFiniteInRange(value: number, minimum: number, maximum: number): boolean {
  return Number.isFinite(value) && value >= minimum && value <= maximum;
}

/** 把可能使用 Unix epoch 的事件时间戳规范到 performance 时间原点。 */
export function normalizeEventTimestamp(eventTimestamp: number): number {
  return eventTimestamp > MAX_MONOTONIC_TIME_MS
    ? eventTimestamp - performance.timeOrigin
    : eventTimestamp;
}

/** 只返回位于真实导航控件上的固定测试标识，其他点击完全忽略。 */
export function resolvePerformanceInteractionTarget(
  target: EventTarget | null,
): string | null {
  if (!(target instanceof Element)) return null;
  const taggedElement = target.closest<HTMLElement>("[data-testid]");
  const testId = taggedElement?.getAttribute("data-testid");
  return testId !== null && testId !== undefined && PERFORMANCE_TARGET_RESULTS.has(testId)
    ? testId
    : null;
}

/** 返回某个批准导航动作必须产生的固定页面结果。 */
export function expectedPerformanceInteractionResult(target: string): string | null {
  return PERFORMANCE_TARGET_RESULTS.get(target) ?? null;
}

/** 只允许系统交付的主键点击进入真实交互样本，拒绝脚本合成事件和辅助按键。 */
export function isTrustedPrimaryPerformanceClick(
  event: Pick<MouseEvent, "button" | "isTrusted">,
): boolean {
  return event.isTrusted && event.button === 0;
}

/** 确认指定测试标识对应的元素当前真实参与布局。 */
function isVisibleTestElement(testId: string): boolean {
  const element = document.querySelector<HTMLElement>(`[data-testid="${testId}"]`);
  if (element === null || element.hidden || element.getClientRects().length === 0)
    return false;
  const style = window.getComputedStyle(element);
  return style.display !== "none" && style.visibility !== "hidden";
}

/** 确认主壳当前参与布局且没有被 CSS 或 hidden 属性隐藏。 */
function isMainShellVisible(): boolean {
  return isVisibleTestElement("app-shell");
}

/** 把事件时间戳到当前高精度时间的差值限制为可接受的点击耗时。 */
function interactionDuration(eventTimestamp: number): number | null {
  const startTime = normalizeEventTimestamp(eventTimestamp);
  const duration = performance.now() - startTime;
  return isFiniteInRange(duration, 0, MAX_DURATION_MS) ? duration : null;
}

let activeSession: ActivePerformanceSession | null = null;
let activeStartPromise: Promise<void> | null = null;
let activationGeneration = 0;

/** 同步读取 Rust 的初始化标记，正常启动不发起任何 IPC。 */
export function isMainPerformanceEvidenceEnabled(): boolean {
  return window.__LOKI_METIS_PERFORMANCE_EVIDENCE__ === true;
}

/** 在完整路由加载前识别共用 index.html 的两个桌宠原生辅助窗。 */
export function isPetAuxiliaryPerformanceEntry(
  location: Pick<Location, "pathname" | "search">,
): boolean {
  if (isPetWindowPath(location.pathname)) return true;
  const view = new URLSearchParams(location.search).get("view");
  return view === "pet" || view === "pet-settings";
}

/** 停止当前页面拥有的 Observer、点击监听、测试控件与未完成帧回调。 */
export function stopMainPerformanceEvidence(): void {
  activationGeneration += 1;
  activeSession?.stop();
  activeSession = null;
}

/** 等待前端队列与 Rust 文件句柄完成最终同步；测试必须在退出前调用。 */
export async function finishMainPerformanceEvidence(): Promise<PerformanceEvidenceFinalization> {
  if (activeSession === null) throw new Error("performance-evidence-session-inactive");
  return activeSession.finish();
}

/**
 * 仅为主视图启动观测会话。Rust 未显式启用时同步返回；重复调用幂等。
 * 启用时由 Rust 统一分配序号，并提供显式结束握手避免退出丢失队尾 IPC。
 */
export function startMainPerformanceEvidence(): Promise<void> {
  if (
    !isMainPerformanceEvidenceEnabled() ||
    isPetAuxiliaryPerformanceEntry(window.location) ||
    activeSession !== null
  ) {
    return Promise.resolve();
  }
  if (activeStartPromise !== null) return activeStartPromise;

  const requestedGeneration = activationGeneration;
  const startPromise = (async (): Promise<void> => {
    if (requestedGeneration !== activationGeneration || activeSession !== null) return;

    let acceptingMetrics = true;
    let writeFailed = false;
    let writeQueue = Promise.resolve();
    let finishPromise: Promise<PerformanceEvidenceFinalization> | null = null;
    const frameIds = new Set<number>();

    /** 串行交给 Rust 强类型边界；Rust 负责唯一递增序号。 */
    const submit = (payload: PerformanceEvidenceMetric): void => {
      if (!acceptingMetrics) return;
      writeQueue = writeQueue
        .then(async () => {
          await invoke("record_performance_evidence", { payload });
        })
        .catch(() => {
          writeFailed = true;
        });
    };

    /** 注册可被会话关闭统一取消的单帧回调。 */
    const requestOwnedFrame = (callback: FrameRequestCallback): number | null => {
      if (!acceptingMetrics) return null;
      const id = window.requestAnimationFrame((time) => {
        frameIds.delete(id);
        if (acceptingMetrics) callback(time);
      });
      frameIds.add(id);
      return id;
    };

    /** 取消指定的会话帧，避免已结束交互留下下一帧唤醒。 */
    const cancelOwnedFrame = (id: number | null): void => {
      if (id === null) return;
      window.cancelAnimationFrame(id);
      frameIds.delete(id);
    };

    let longTaskObserver: PerformanceObserver | null = null;
    const declaresLongTaskSupport =
      typeof PerformanceObserver !== "undefined" &&
      PerformanceObserver.supportedEntryTypes?.includes("longtask") === true;

    /** 把浏览器原生 Long Task 条目转换为固定阻塞区间载荷。 */
    const submitNativeLongTasks = (entries: readonly PerformanceEntry[]): void => {
      for (const entry of entries) {
        if (
          !isFiniteInRange(entry.startTime, 0, MAX_MONOTONIC_TIME_MS) ||
          !isFiniteInRange(entry.duration, MIN_BLOCKING_INTERVAL_MS, MAX_DURATION_MS)
        ) {
          continue;
        }
        submit({
          durationMs: entry.duration,
          kind: "renderer-blocking-interval",
          startTimeMs: entry.startTime,
          timingSource: "performance-observer",
        });
      }
    };

    if (declaresLongTaskSupport) {
      try {
        longTaskObserver = new PerformanceObserver((list) => {
          submitNativeLongTasks(list.getEntries());
        });
        longTaskObserver.observe({ buffered: true, type: "longtask" });
      } catch {
        longTaskObserver?.disconnect();
        longTaskObserver = null;
      }
    }

    const timingSource: RendererTimingSource =
      longTaskObserver === null ? "animation-frame-gap" : "performance-observer";
    submit({
      kind: "renderer-capabilities",
      longTaskSupported: longTaskObserver !== null,
      timingSource,
    });

    /** WebKit 缺少 Long Task API 时，以单一低分配 rAF 循环覆盖完整可见会话。 */
    let nativeWindowVisible = true;
    let removeNativeVisibilityListener: (() => void) | null = null;
    let abortActiveInteraction: (() => void) | null = null;
    let frameGapFrameId: number | null = null;
    let previousVisibleFrameTime: number | null = null;
    const rendererIsVisible = (): boolean =>
      nativeWindowVisible && document.visibilityState === "visible";

    /** 提交一段已确认在前台连续发生的帧间隔。 */
    const submitFrameGap = (startTimeMs: number, endTimeMs: number): void => {
      const durationMs = endTimeMs - startTimeMs;
      if (isFiniteInRange(durationMs, MIN_BLOCKING_INTERVAL_MS, MAX_DURATION_MS)) {
        submit({
          durationMs,
          kind: "renderer-blocking-interval",
          startTimeMs,
          timingSource: "animation-frame-gap",
        });
      }
    };
    /** 隐藏时不保留 rAF；恢复后首帧只重建基线，不把隐藏时间算成阻塞。 */
    const stopFrameGapObservation = (): void => {
      if (frameGapFrameId !== null) window.cancelAnimationFrame(frameGapFrameId);
      frameGapFrameId = null;
      previousVisibleFrameTime = null;
    };
    const observeFrameGap = (frameTime: number): void => {
      frameGapFrameId = null;
      if (
        !acceptingMetrics ||
        timingSource !== "animation-frame-gap" ||
        !rendererIsVisible()
      ) {
        previousVisibleFrameTime = null;
        return;
      }
      if (previousVisibleFrameTime !== null)
        submitFrameGap(previousVisibleFrameTime, frameTime);
      previousVisibleFrameTime = frameTime;
      frameGapFrameId = window.requestAnimationFrame(observeFrameGap);
    };
    const startFrameGapObservation = (): void => {
      if (
        timingSource !== "animation-frame-gap" ||
        !acceptingMetrics ||
        !rendererIsVisible() ||
        frameGapFrameId !== null
      ) {
        return;
      }
      previousVisibleFrameTime = null;
      frameGapFrameId = window.requestAnimationFrame(observeFrameGap);
    };
    const handleRendererVisibilityChange = (): void => {
      if (rendererIsVisible()) {
        startFrameGapObservation();
        return;
      }
      stopFrameGapObservation();
      abortActiveInteraction?.();
    };
    const nativeVisibilityRegistration = getCurrentWindow()
      .listen<boolean>(PERFORMANCE_WINDOW_VISIBILITY_EVENT, ({ payload }) => {
        nativeWindowVisible = payload;
        handleRendererVisibilityChange();
      })
      .then((unlisten) => {
        if (acceptingMetrics) removeNativeVisibilityListener = unlisten;
        else unlisten();
      })
      .catch(() => {
        writeFailed = true;
      });
    document.addEventListener("visibilitychange", handleRendererVisibilityChange);
    startFrameGapObservation();

    /** 在主壳已经可见后再等完整双帧，随后报告同一时刻的墙钟和单调时间。 */
    const waitForReady = (): void => {
      if (!isMainShellVisible()) {
        requestOwnedFrame(waitForReady);
        return;
      }
      requestOwnedFrame(() => {
        requestOwnedFrame(() => {
          if (!isMainShellVisible()) {
            waitForReady();
            return;
          }
          const wallTimeMs = Date.now();
          const monotonicTimeMs = performance.now();
          if (
            isFiniteInRange(wallTimeMs, 0, MAX_WALL_TIME_MS) &&
            isFiniteInRange(monotonicTimeMs, 0, MAX_MONOTONIC_TIME_MS)
          ) {
            submit({ kind: "main-window-ready", monotonicTimeMs, wallTimeMs });
          }
        });
      });
    };
    waitForReady();

    /**
     * 只在可信导航点击后开启有界结果等待。目标结果连续两帧可见后立即释放；
     * 超时、隐藏或后续白名单点击也会终止，帧间隔由会话级回退循环独立采样。
     */
    const handleClick = (event: MouseEvent): void => {
      if (!isTrustedPrimaryPerformanceClick(event)) return;
      const target = resolvePerformanceInteractionTarget(event.target);
      if (target === null) return;
      const result = expectedPerformanceInteractionResult(target);
      if (result === null) return;
      abortActiveInteraction?.();
      if (isVisibleTestElement(result)) return;
      const pathBefore = window.location.pathname;
      const eventTimestamp = event.timeStamp;
      const normalizedStartTime = normalizeEventTimestamp(eventTimestamp);
      if (!isFiniteInRange(normalizedStartTime, 0, MAX_MONOTONIC_TIME_MS)) {
        writeFailed = true;
        return;
      }
      let stopped = false;
      let frameId: number | null = null;
      let resultWasVisible = false;
      let timeoutId: number | null = null;

      const stopInteraction = (aborted: boolean): void => {
        if (stopped) return;
        stopped = true;
        if (aborted) writeFailed = true;
        if (timeoutId !== null) window.clearTimeout(timeoutId);
        cancelOwnedFrame(frameId);
        frameId = null;
        if (abortActiveInteraction === abortInteraction) abortActiveInteraction = null;
      };
      const abortInteraction = (): void => stopInteraction(true);
      const observeInteractionFrame = (): void => {
        frameId = null;
        if (stopped || !rendererIsVisible()) {
          abortInteraction();
          return;
        }

        const resultIsVisible =
          window.location.pathname !== pathBefore && isVisibleTestElement(result);
        if (resultIsVisible && resultWasVisible) {
          const durationMs = interactionDuration(eventTimestamp);
          if (durationMs !== null) {
            submit({ durationMs, kind: "interaction", result, target });
            stopInteraction(false);
          } else {
            abortInteraction();
          }
          return;
        }
        resultWasVisible = resultIsVisible;
        frameId = requestOwnedFrame(observeInteractionFrame);
      };

      abortActiveInteraction = abortInteraction;
      timeoutId = window.setTimeout(abortInteraction, INTERACTION_WINDOW_TIMEOUT_MS);
      frameId = requestOwnedFrame(observeInteractionFrame);
    };
    document.addEventListener("click", handleClick, true);

    const finalizeButton = document.createElement("button");
    finalizeButton.type = "button";
    finalizeButton.dataset.testid = FINALIZE_BUTTON_TEST_ID;
    finalizeButton.textContent = "Finalize performance evidence";
    finalizeButton.style.cssText =
      "position:fixed;right:8px;bottom:8px;z-index:2147483647;padding:6px 10px";
    document.body.append(finalizeButton);

    /** 释放会话拥有的浏览器资源；是否移除结果控件由调用方决定。 */
    const releaseResources = (removeFinalizeButton: boolean): void => {
      document.removeEventListener("click", handleClick, true);
      document.removeEventListener("visibilitychange", handleRendererVisibilityChange);
      abortActiveInteraction?.();
      stopFrameGapObservation();
      removeNativeVisibilityListener?.();
      removeNativeVisibilityListener = null;
      longTaskObserver?.disconnect();
      for (const frameId of frameIds) window.cancelAnimationFrame(frameId);
      frameIds.clear();
      window.removeEventListener("pagehide", stopSession);
      if (removeFinalizeButton) finalizeButton.remove();
    };

    /** 异常卸载只负责同步释放；未出现 finalized 记录的证据必须失败关闭。 */
    const stopSession = (): void => {
      acceptingMetrics = false;
      releaseResources(true);
      if (activeSession?.stop === stopSession) activeSession = null;
    };

    /** 排空 Observer 与 IPC 队列，再让 Rust 同步文件并写入最终确认记录。 */
    const finishSession = (): Promise<PerformanceEvidenceFinalization> => {
      if (finishPromise !== null) return finishPromise;
      finishPromise = (async () => {
        if (longTaskObserver !== null)
          submitNativeLongTasks(longTaskObserver.takeRecords());
        acceptingMetrics = false;
        releaseResources(false);
        await nativeVisibilityRegistration;
        await writeQueue;
        if (writeFailed) throw new Error("performance-evidence-write-failed");
        const finalization = await invoke<PerformanceEvidenceFinalization>(
          "finish_performance_evidence",
        );
        finalizeButton.disabled = true;
        finalizeButton.dataset.state = "finalized";
        finalizeButton.textContent = "Performance evidence finalized";
        return finalization;
      })().catch((error: unknown) => {
        finalizeButton.dataset.state = "failed";
        finalizeButton.textContent = "Performance evidence failed";
        throw error;
      });
      return finishPromise;
    };

    finalizeButton.addEventListener("click", () => {
      void finishSession().catch(() => undefined);
    });
    activeSession = { finish: finishSession, stop: stopSession };
    window.addEventListener("pagehide", stopSession, { once: true });
    await nativeVisibilityRegistration;
  })();

  const trackedPromise = startPromise.finally(() => {
    if (activeStartPromise === trackedPromise) activeStartPromise = null;
  });
  activeStartPromise = trackedPromise;
  return trackedPromise;
}
