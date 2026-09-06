import { useQuery } from "@tanstack/react-query";
import { useCallback, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  closePetOverlay,
  getPetWindowState,
  hidePetSettings,
  setPetAlwaysOnTop,
  setPetLayout,
  setPetLocked,
  setPetSize,
  showMainWindow,
  type PetLayout,
  type PetWindowState,
} from "../api/monitor";
import "../pet-overlay.css";
import {
  usePetAuxiliaryWindowLanguage,
  usePetWindowStateEvents,
} from "./usePetAuxiliaryWindowSync";

/** 设置窗首次读取前的保守桌宠偏好。 */
const DEFAULT_WINDOW_STATE: PetWindowState = {
  layout: "grid",
  locked: false,
  pageIndex: 0,
  pageCount: 1,
  pageHasImage: false,
  hasAnyImage: false,
  slots: [],
  petSize: 64,
  sizeMin: 32,
  sizeMax: 256,
  alwaysOnTop: true,
};

/** 桌宠支持的六种布局及其直观尺寸标识。 */
const LAYOUT_OPTIONS: ReadonlyArray<{ layout: PetLayout; label: string }> = [
  { layout: "single", label: "1×1" },
  { layout: "row", label: "1×2" },
  { layout: "column", label: "2×1" },
  { layout: "row3", label: "1×3" },
  { layout: "column3", label: "3×1" },
  { layout: "grid", label: "2×2" },
];

/** 桌宠独立设置窗：只保留布局、尺寸与窗口行为。 */
export function PetSettingsPage(): ReactElement {
  const { i18n, t } = useTranslation();
  const view = useQuery({
    queryFn: getPetWindowState,
    queryKey: ["pet-window-state"],
  });
  const state = view.data ?? DEFAULT_WINDOW_STATE;
  const size = Math.min(state.sizeMax, Math.max(state.sizeMin, state.petSize));
  const refetch = view.refetch;

  usePetWindowStateEvents(refetch);
  usePetAuxiliaryWindowLanguage(i18n);

  /** 执行窗口偏好变更后重读宿主快照。 */
  const updatePreference = useCallback(
    async (action: () => Promise<void>): Promise<void> => {
      await action();
      await refetch();
    },
    [refetch],
  );

  /** 设置窗不展示临时错误，失败由下一次宿主事件恢复。 */
  const runPreferenceUpdate = (action: () => Promise<void>): void => {
    void updatePreference(action).catch(() => undefined);
  };

  return (
    <main className="pet-settings-shell" data-testid="pet-settings-page">
      <header>
        <div>
          <strong>{t("monitor.pet.settingsTitle")}</strong>
          <small>{t("monitor.pet.settingsHint")}</small>
        </div>
        <button
          aria-label={t("monitor.pet.settingsClose")}
          className="pet-settings-close"
          onClick={() => {
            void hidePetSettings().catch(() => undefined);
          }}
          title={t("monitor.pet.settingsClose")}
          type="button"
        >
          ×
        </button>
      </header>

      <section aria-label={t("monitor.pet.settingsTitle")} className="pet-settings-menu">
        <div className="pet-menu-label">{t("monitor.pet.displayCount")}</div>
        <div aria-label={t("monitor.pet.layout")} className="pet-menu-segment" role="group">
          {LAYOUT_OPTIONS.map((option) => (
            <button
              aria-pressed={state.layout === option.layout}
              className={state.layout === option.layout ? "selected" : ""}
              key={option.layout}
              onClick={() => {
                runPreferenceUpdate(() => setPetLayout(option.layout));
              }}
              type="button"
            >
              {option.label}
            </button>
          ))}
        </div>

        <div className="pet-menu-label">{t("monitor.pet.size")}</div>
        <div className="pet-size-control">
          <input
            aria-label={t("monitor.pet.size")}
            aria-valuetext={t("monitor.pet.pixels", { size })}
            max={state.sizeMax}
            min={state.sizeMin}
            onChange={(event) => {
              const nextSize = Number(event.currentTarget.value);
              runPreferenceUpdate(() => setPetSize(nextSize));
            }}
            step={1}
            type="range"
            value={size}
          />
          <output>{size}px</output>
        </div>

        <button
          aria-label={t("monitor.pet.alwaysOnTop")}
          aria-checked={state.alwaysOnTop}
          className="pet-menu-check"
          onClick={() => {
            runPreferenceUpdate(() => setPetAlwaysOnTop(!state.alwaysOnTop));
          }}
          role="switch"
          type="button"
        >
          <span aria-hidden="true">{state.alwaysOnTop ? "✓" : ""}</span>
          {t("monitor.pet.alwaysOnTop")}
        </button>
        <button
          aria-label={t("monitor.pet.lockPositionSize")}
          aria-checked={state.locked}
          className="pet-menu-check"
          onClick={() => {
            runPreferenceUpdate(() => setPetLocked(!state.locked));
          }}
          role="switch"
          type="button"
        >
          <span aria-hidden="true">{state.locked ? "✓" : ""}</span>
          {t("monitor.pet.lockPositionSize")}
        </button>

        <div className="pet-menu-divider" />
        <button
          onClick={() => {
            void showMainWindow().catch(() => undefined);
          }}
          type="button"
        >
          {t("monitor.pet.returnMain")}
        </button>
        <button
          onClick={() => {
            void closePetOverlay().catch(() => undefined);
          }}
          type="button"
        >
          {t("monitor.pet.hideToTray")}
        </button>
      </section>
    </main>
  );
}
