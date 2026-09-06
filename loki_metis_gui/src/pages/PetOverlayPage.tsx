import { useQuery } from "@tanstack/react-query";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MouseEvent,
  type ReactElement,
  type WheelEvent,
} from "react";
import { useTranslation } from "react-i18next";

import {
  focusFirstPopulatedPetPage,
  getMonitorImageBytes,
  getPetWindowState,
  resizePetStep,
  showMainWindow,
  showPetSettings,
  startPetOverlayDrag,
  turnPetPage,
  type PetOverlaySlot,
  type PetPageDirection,
  type PetWindowState,
} from "../api/monitor";
import { monitorImageBytesToDataUrl } from "../pet-overlay-image";
import "../pet-overlay.css";
import {
  usePetAuxiliaryWindowLanguage,
  usePetWindowStateEvents,
} from "./usePetAuxiliaryWindowSync";

const WHEEL_THROTTLE_MS = 220;

/** IPC 返回前使用的最小桌宠窗口快照。 */
const EMPTY_PET_STATE: PetWindowState = {
  layout: "grid",
  locked: false,
  pageIndex: 0,
  pageCount: 3,
  pageHasImage: false,
  hasAnyImage: false,
  slots: [0, 1, 2, 3].map((slotIndex) => ({ slotIndex, tile: null })),
  petSize: 64,
  sizeMin: 32,
  sizeMax: 256,
  alwaysOnTop: true,
};

/** 把 IPC 字节变成 CSP 已允许的 data URL。 */
function useSlotImageUrl(imageId: string | null): {
  imageUnavailable: boolean;
  imageUrl: string | undefined;
} {
  const query = useQuery({
    enabled: imageId !== null,
    queryFn: () => getMonitorImageBytes(imageId ?? ""),
    queryKey: ["monitor-image-bytes", imageId],
    select: monitorImageBytesToDataUrl,
  });
  return {
    imageUnavailable: imageId !== null && query.data === undefined,
    imageUrl: query.data,
  };
}

/** 格式化全局位置编号，不将位置预绑定到任何 Agent。 */
function positionLabel(slotIndex: number): string {
  return String(slotIndex + 1).padStart(2, "0");
}

/** 桌宠当前页中的一个纯位置格。 */
function PetOverlayTile({ slot }: { slot: PetOverlaySlot }): ReactElement {
  const tile = slot.tile;
  const { imageUnavailable, imageUrl } = useSlotImageUrl(tile?.imageId ?? null);
  const [imageDecodeFailed, setImageDecodeFailed] = useState(false);
  const renderableImageUrl = imageDecodeFailed ? undefined : imageUrl;
  const label = positionLabel(slot.slotIndex);
  const accessibleLabel = tile
    ? [tile.name, tile.content].filter((value) => value.trim().length > 0).join("，")
    : undefined;

  return (
    <section
      className={`pet-tile${renderableImageUrl ? " occupied" : " empty"}${
        imageUnavailable || imageDecodeFailed ? " image-unavailable" : ""
      }`}
      data-tool={tile?.tool}
    >
      {renderableImageUrl && tile ? (
        <img
          alt={`${label}-${tile.name}`}
          draggable={false}
          onError={() => setImageDecodeFailed(true)}
          src={renderableImageUrl}
        />
      ) : (
        <div className="pet-empty">
          <span>{label}</span>
        </div>
      )}
      {tile ? (
        <div aria-label={accessibleLabel} className="pet-labels">
          <strong>{tile.name}</strong>
          {tile.content.trim().length > 0 ? <span>{tile.content}</span> : null}
        </div>
      ) : null}
    </section>
  );
}

/** 桌宠悬浮窗根：位置驱动的透明画布，不挂主壳。 */
export function PetOverlayPage(): ReactElement {
  const { i18n, t } = useTranslation();
  const view = useQuery({
    queryFn: getPetWindowState,
    queryKey: ["pet-window-state"],
  });
  const state = view.data ?? EMPTY_PET_STATE;
  const [isHovered, setIsHovered] = useState(false);
  const [isKeyboardFocused, setIsKeyboardFocused] = useState(false);
  const lastWheelAt = useRef(0);
  const focusedPopulatedPage = useRef(false);
  const refetch = view.refetch;
  const controlsVisible = isHovered || isKeyboardFocused;

  usePetWindowStateEvents(refetch);
  usePetAuxiliaryWindowLanguage(i18n);

  /** 宿主动作成功后立即重读快照；失败交给下一次宿主事件恢复。 */
  const runStateAction = useCallback(
    async (action: () => Promise<void>): Promise<void> => {
      await action();
      await refetch();
    },
    [refetch],
  );

  /** 请求切换桌宠页，分页边界由 Rust 宿主统一处理。 */
  const requestPageTurn = useCallback(
    (direction: PetPageDirection): void => {
      void runStateAction(() => turnPetPage(direction)).catch(() => undefined);
    },
    [runStateAction],
  );

  /** 左键单击非控件区域时将拖动交给原生窗口。 */
  const onMouseDown = (event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0 || event.detail > 1 || state.locked) return;
    if ((event.target as HTMLElement).closest("[data-pet-control]")) return;
    void startPetOverlayDrag().catch(() => undefined);
  };

  /** 双击非控件区域时显示并聚焦主界面。 */
  const onDoubleClick = (event: MouseEvent<HTMLElement>) => {
    if ((event.target as HTMLElement).closest("[data-pet-control]")) return;
    void showMainWindow().catch(() => undefined);
  };

  /** 右键改为打开桌宠独立设置窗，不显示浏览器菜单。 */
  const onContextMenu = (event: MouseEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    void showPetSettings().catch(() => undefined);
  };

  /** 普通滚轮翻页，Ctrl/Cmd 滚轮只上报缩放意图。 */
  const onWheel = (event: WheelEvent<HTMLElement>) => {
    const now = Date.now();
    if (now - lastWheelAt.current < WHEEL_THROTTLE_MS) return;
    lastWheelAt.current = now;
    if (event.ctrlKey || event.metaKey) {
      void runStateAction(() => resizePetStep(event.deltaY < 0 ? "grow" : "shrink")).catch(
        () => undefined,
      );
      return;
    }
    if (state.pageCount > 1) {
      requestPageTurn(event.deltaY < 0 ? "previous" : "next");
    }
  };

  useEffect(() => {
    /** 绑定桌宠分页键盘路径与键盘右键菜单等价入口。 */
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "ArrowLeft") requestPageTurn("previous");
      if (event.key === "ArrowRight") requestPageTurn("next");
      if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
        event.preventDefault();
        void showPetSettings().catch(() => undefined);
      }
    };
    /** 窗口失焦或指针离开文档时隐藏悬浮控件。 */
    const hideControls = () => {
      setIsHovered(false);
      setIsKeyboardFocused(false);
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("blur", hideControls);
    document.addEventListener("mouseleave", hideControls);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("blur", hideControls);
      document.removeEventListener("mouseleave", hideControls);
    };
  }, [requestPageTurn]);

  useEffect(() => {
    if (focusedPopulatedPage.current || !state.hasAnyImage) return;
    focusedPopulatedPage.current = true;
    void runStateAction(focusFirstPopulatedPetPage).catch(() => {
      focusedPopulatedPage.current = false;
    });
  }, [runStateAction, state.hasAnyImage]);

  return (
    <main
      aria-label={t("monitor.pet.shellAria", {
        page: state.pageIndex + 1,
        pages: state.pageCount,
      })}
      className={`pet-shell ${state.layout}${state.locked ? " locked" : ""}${
        isHovered ? " hovered" : ""
      }${isKeyboardFocused ? " keyboard-focused" : ""}${
        state.pageHasImage ? "" : " empty-page"
      }`}
      data-testid="pet-overlay-page"
      onBlur={(event) => {
        if (
          !(event.relatedTarget instanceof Node) ||
          !event.currentTarget.contains(event.relatedTarget)
        ) {
          setIsKeyboardFocused(false);
        }
      }}
      onContextMenu={onContextMenu}
      onDoubleClick={onDoubleClick}
      onFocus={() => setIsKeyboardFocused(true)}
      onMouseDown={onMouseDown}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      onWheel={onWheel}
      tabIndex={0}
    >
      <div className="pet-grid">
        {state.slots.map((slot) => (
          <PetOverlayTile
            key={`${slot.slotIndex}:${slot.tile?.imageId ?? "empty"}`}
            slot={slot}
          />
        ))}
      </div>

      <div aria-hidden={!controlsVisible} className="pet-pager" data-pet-control="">
        <button
          aria-label={t("monitor.pet.previousPage")}
          disabled={state.pageCount <= 1}
          onClick={() => requestPageTurn("previous")}
          tabIndex={controlsVisible ? 0 : -1}
          title={t("monitor.pet.previousPage")}
          type="button"
        >
          ‹
        </button>
        <span>
          {state.pageIndex + 1}/{state.pageCount}
        </span>
        <button
          aria-label={t("monitor.pet.nextPage")}
          disabled={state.pageCount <= 1}
          onClick={() => requestPageTurn("next")}
          tabIndex={controlsVisible ? 0 : -1}
          title={t("monitor.pet.nextPage")}
          type="button"
        >
          ›
        </button>
      </div>
    </main>
  );
}
