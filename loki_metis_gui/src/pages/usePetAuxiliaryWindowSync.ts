import { listen } from "@tauri-apps/api/event";
import type { i18n as I18nInstance } from "i18next";
import { useEffect } from "react";

import { INTERFACE_LANGUAGE_STORAGE_KEY } from "../lib/language";

/** 宿主在桌宠内容或窗口偏好变化后广播的唯一事件名。 */
export const PET_WINDOW_STATE_CHANGED_EVENT = "pet-window-state-changed";

/** 原生语言命令成功后向所有 WebView 广播的事件名。 */
export const INTERFACE_LANGUAGE_CHANGED_EVENT = "interface-language-changed";

/** 订阅宿主桌宠状态变化，并按事件立即重读唯一快照。 */
export function usePetWindowStateEvents(refetch: () => Promise<unknown>): void {
  useEffect(() => {
    let disposed = false;
    let stopListening: (() => void) | undefined;
    let reconcileRunning = false;
    let reconcileDirty = false;

    /** 串行合并突发事件；订阅首次就绪时强制在当前请求结束后再读一次。 */
    const requestReconcile = (forceSecondPass = false): void => {
      if (disposed) return;
      reconcileDirty = true;
      if (reconcileRunning) return;
      reconcileRunning = true;
      void (async () => {
        let forceNext = forceSecondPass;
        try {
          while (!disposed && reconcileDirty) {
            reconcileDirty = false;
            try {
              await refetch();
            } catch {
              // 查询失败由下一个宿主事件重新触发，不向窗口抛出未处理异常。
            }
            if (forceNext && !disposed) {
              forceNext = false;
              reconcileDirty = true;
            }
          }
        } finally {
          reconcileRunning = false;
          if (!disposed && reconcileDirty) requestReconcile();
        }
      })();
    };

    void listen<void>(PET_WINDOW_STATE_CHANGED_EVENT, () => {
      // 事件可能撞上组件动作发起的外部 refetch；强制在该请求 settle 后再读一次。
      requestReconcile(true);
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else {
          stopListening = unlisten;
          // React Query 可能复用尚未完成的首次请求，因此订阅就绪后保证再执行一轮。
          requestReconcile(true);
        }
      })
      .catch(() => {
        // 普通浏览器预览没有 Tauri 事件通道，仍保留首次静态快照。
      });
    return () => {
      disposed = true;
      stopListening?.();
    };
  }, [refetch]);
}

/** 跨 WebView 同步主窗口保存的界面语言。 */
export function usePetAuxiliaryWindowLanguage(instance: I18nInstance): void {
  useEffect(() => {
    let disposed = false;
    let stopListening: (() => void) | undefined;
    const changeLanguage = (value: string | null) => {
      if (value !== "zh-CN" && value !== "en-US") return;
      if (value === instance.resolvedLanguage) return;
      void instance.changeLanguage(value).catch(() => undefined);
    };
    const onStorage = (event: StorageEvent) => {
      if (event.key !== INTERFACE_LANGUAGE_STORAGE_KEY) return;
      changeLanguage(event.newValue);
    };
    window.addEventListener("storage", onStorage);
    void listen<string>(INTERFACE_LANGUAGE_CHANGED_EVENT, ({ payload }) => {
      if (!disposed) changeLanguage(payload);
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else {
          stopListening = unlisten;
          // 订阅后重读已保存语言，封闭原生与 storage 监听建立前的竞态。
          try {
            changeLanguage(window.localStorage.getItem(INTERFACE_LANGUAGE_STORAGE_KEY));
          } catch {
            // 存储不可用时仍可依赖后续原生语言事件。
          }
        }
      })
      .catch(() => {
        // 普通浏览器预览通过 storage 事件同步语言即可。
      });
    return () => {
      disposed = true;
      stopListening?.();
      window.removeEventListener("storage", onStorage);
    };
  }, [instance]);
}
