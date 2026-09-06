/**
 * 为显式本机发布验收采集无隐私浏览器性能指标。
 *
 * 正常启动只查询 Rust 的默认关闭状态；未启用时不会安装 PerformanceObserver、事件监听
 * 或帧回调。启用后的载荷仍由 Rust 重新校验并只写入系统临时目录。
 */
import { invoke } from "@tauri-apps/api/core";

import { isPetWindowPath } from "../default-landing";

const MAX_MONOTONIC_TIME_MS = 604_800_000;
const MAX_WALL_TIME_MS = 10_000_000_000_000;
const MAX_DURATION_MS = 60_000;
const MIN_LONG_TASK_DURATION_MS = 50;
const GENERIC_TARGET = "generic";
const ALLOWED_TARGETS = new Set([
  "image-picker-trigger",
  "image-picker-upload",
  "navigation-icon-dashboard",
  "navigation-icon-monitor",
  "navigation-icon-settings",
  "navigation-label-dashboard",
  "navigation-label-monitor",
  "navigation-label-settings",
  "settings-capability-autostart",
  "settings-capability-system_notification",
]);

/** 描述浏览器长任务能力的固定证据载荷。 */
interface RendererCapabilitiesMetric {
  kind: "renderer-capabilities";
  sequence: number;
  longTaskSupported: boolean;
}

/** 描述主壳可见并经过双帧稳定后的就绪时间。 */
interface MainWindowReadyMetric {
  kind: "main-window-ready";
  sequence: number;
  wallTimeMs: number;
  monotonicTimeMs: number;
}

/** 描述真实点击到双帧完成的渲染耗时，不携带页面文案或业务值。 */
interface InteractionMetric {
  kind: "interaction";
  sequence: number;
  target: string;
  durationMs: number;
}

/** 描述浏览器报告的单个长任务。 */
interface LongTaskMetric {
  kind: "long-task";
  sequence: number;
  startTimeMs: number;
  durationMs: number;
}

/** 前端唯一允许提交给本机证据命令的四类载荷。 */
type PerformanceEvidenceMetric =
  RendererCapabilitiesMetric | MainWindowReadyMetric | InteractionMetric | LongTaskMetric;

/** 写入前使用的无序号载荷；序号只能由本模块生成。 */
type PendingPerformanceEvidenceMetric =
  | Omit<RendererCapabilitiesMetric, "sequence">
  | Omit<MainWindowReadyMetric, "sequence">
  | Omit<InteractionMetric, "sequence">
  | Omit<LongTaskMetric, "sequence">;

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

/** 只返回固定白名单 data-testid；其余元素统一匿名为 generic。 */
export function resolvePerformanceInteractionTarget(target: EventTarget | null): string {
  if (!(target instanceof Element)) return GENERIC_TARGET;
  const taggedElement = target.closest<HTMLElement>("[data-testid]");
  const testId = taggedElement?.getAttribute("data-testid");
  return testId !== null && testId !== undefined && ALLOWED_TARGETS.has(testId)
    ? testId
    : GENERIC_TARGET;
}

/** 只允许系统交付的主键点击进入真实交互样本，拒绝脚本合成事件和辅助按键。 */
export function isTrustedPrimaryPerformanceClick(
  event: Pick<MouseEvent, "button" | "isTrusted">,
): boolean {
  return event.isTrusted && event.button === 0;
}

/** 确认主壳当前参与布局且没有被 CSS 或 hidden 属性隐藏。 */
function isMainShellVisible(): boolean {
  const shell = document.querySelector<HTMLElement>('[data-testid="app-shell"]');
  if (shell === null || shell.hidden || shell.getClientRects().length === 0) return false;
  const style = window.getComputedStyle(shell);
  return style.display !== "none" && style.visibility !== "hidden";
}

/** 把事件时间戳到当前高精度时间的差值限制为可接受的点击耗时。 */
function interactionDuration(eventTimestamp: number): number | null {
  const startTime = normalizeEventTimestamp(eventTimestamp);
  const duration = performance.now() - startTime;
  return isFiniteInRange(duration, 0, MAX_DURATION_MS) ? duration : null;
}

let activeCleanup: (() => void) | null = null;
let activationGeneration = 0;

/** 停止当前页面拥有的 Observer、点击监听与未完成帧回调。 */
export function stopMainPerformanceEvidence(): void {
  activationGeneration += 1;
  activeCleanup?.();
  activeCleanup = null;
}

/**
 * 仅为主视图启动观测会话；Rust 未显式启用时在状态查询后立即返回。
 * 会话在 pagehide 时自行回收，所有写入按 Promise 链保持严格序号顺序。
 */
export async function startMainPerformanceEvidence(): Promise<void> {
  stopMainPerformanceEvidence();
  if (isPetWindowPath(window.location.pathname)) return;
  const requestedGeneration = activationGeneration;

  let enabled = false;
  try {
    enabled = (await invoke<unknown>("get_performance_evidence_status")) === true;
  } catch {
    return;
  }
  if (!enabled || requestedGeneration !== activationGeneration) return;

  let stopped = false;
  let sequence = 0;
  let writeQueue = Promise.resolve();
  const frameIds = new Set<number>();

  /** 给固定载荷分配递增序号，并串行交给 Rust 强类型边界。 */
  const submit = (pending: PendingPerformanceEvidenceMetric): void => {
    if (stopped) return;
    sequence += 1;
    const payload = { ...pending, sequence } as PerformanceEvidenceMetric;
    writeQueue = writeQueue
      .then(async () => {
        await invoke("record_performance_evidence", { payload });
      })
      .catch(() => undefined);
  };

  /** 注册可被会话关闭统一取消的单帧回调。 */
  const requestOwnedFrame = (callback: FrameRequestCallback): void => {
    const id = window.requestAnimationFrame((time) => {
      frameIds.delete(id);
      if (!stopped) callback(time);
    });
    frameIds.add(id);
  };

  let longTaskObserver: PerformanceObserver | null = null;
  const declaresLongTaskSupport =
    typeof PerformanceObserver !== "undefined" &&
    PerformanceObserver.supportedEntryTypes?.includes("longtask") === true;
  if (declaresLongTaskSupport) {
    try {
      longTaskObserver = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (
            !isFiniteInRange(entry.startTime, 0, MAX_MONOTONIC_TIME_MS) ||
            !isFiniteInRange(entry.duration, MIN_LONG_TASK_DURATION_MS, MAX_DURATION_MS)
          ) {
            continue;
          }
          submit({
            durationMs: entry.duration,
            kind: "long-task",
            startTimeMs: entry.startTime,
          });
        }
      });
      longTaskObserver.observe({ buffered: true, type: "longtask" });
    } catch {
      longTaskObserver?.disconnect();
      longTaskObserver = null;
    }
  }
  submit({
    kind: "renderer-capabilities",
    longTaskSupported: longTaskObserver !== null,
  });

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

  /** 真实 click 发生后等待双帧，再记录从事件时间戳起算的完整交互耗时。 */
  const handleClick = (event: MouseEvent): void => {
    if (!isTrustedPrimaryPerformanceClick(event)) return;
    const eventTimestamp = event.timeStamp;
    const target = resolvePerformanceInteractionTarget(event.target);
    requestOwnedFrame(() => {
      requestOwnedFrame(() => {
        const durationMs = interactionDuration(eventTimestamp);
        if (durationMs !== null) {
          submit({ durationMs, kind: "interaction", target });
        }
      });
    });
  };
  document.addEventListener("click", handleClick, true);

  /** 回收当前页面持有的全部浏览器观测资源。 */
  const cleanup = (): void => {
    if (stopped) return;
    stopped = true;
    document.removeEventListener("click", handleClick, true);
    longTaskObserver?.disconnect();
    for (const frameId of frameIds) window.cancelAnimationFrame(frameId);
    frameIds.clear();
    window.removeEventListener("pagehide", cleanup);
  };
  activeCleanup = cleanup;
  window.addEventListener("pagehide", cleanup, { once: true });
}
