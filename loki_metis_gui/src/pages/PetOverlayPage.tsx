import { useQuery } from "@tanstack/react-query";
import { type MouseEvent, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  closePetOverlay,
  getMonitorImageBytes,
  getPetOverlayView,
  startPetOverlayDrag,
  type PetOverlaySlot,
} from "../api/monitor";
import { monitorImageBytesToDataUrl } from "../pet-overlay-image";
import "../pet-overlay.css";

/** 把 IPC 字节变成 CSP 已允许的 data URL。 */
function useSlotImageUrl(imageId: string | null): string | undefined {
  const query = useQuery({
    enabled: imageId !== null,
    queryFn: () => getMonitorImageBytes(imageId ?? ""),
    queryKey: ["monitor-image-bytes", imageId],
  });
  if (!query.data) {
    return undefined;
  }
  return monitorImageBytesToDataUrl(query.data);
}

/** 桌宠宫格中的单个槽位。 */
function PetOverlayTile({ slot, index }: { slot: PetOverlaySlot; index: number }): ReactElement {
  const { t } = useTranslation();
  const imageUrl = useSlotImageUrl(slot.imageId);
  return (
    <section className={`pet-tile${slot.occupied ? " occupied" : " empty"}`} data-tool={slot.tool}>
      {imageUrl ? (
        <img alt={slot.name} draggable={false} src={imageUrl} />
      ) : (
        <div className="pet-empty">
          <span>{String(index + 1).padStart(2, "0")}</span>
          <small>{t("monitor.pet.waiting")}</small>
        </div>
      )}
      <div className="pet-labels">
        <strong>{slot.name}</strong>
      </div>
    </section>
  );
}

/** 桌宠悬浮窗根：四槽宫格，不挂主壳。 */
export function PetOverlayPage(): ReactElement {
  const { t } = useTranslation();
  const view = useQuery({ queryFn: getPetOverlayView, queryKey: ["pet-overlay-view"] });
  const slots = view.data?.slots ?? [];

  /** 左键拖动无边框窗口；关闭按钮不触发拖动。 */
  const onMouseDown = (event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement).closest("[data-pet-control]")) return;
    void startPetOverlayDrag();
  };

  return (
    <main
      aria-label={t("monitor.pet.shellAria")}
      className="pet-shell"
      data-testid="pet-overlay-page"
      onMouseDown={onMouseDown}
    >
      <button
        aria-label={t("monitor.pet.closeAria")}
        className="pet-close"
        data-pet-control=""
        onClick={() => {
          void closePetOverlay();
        }}
        type="button"
      >
        {t("monitor.pet.close")}
      </button>
      <div className="pet-grid">
        {slots.map((slot, index) => (
          <PetOverlayTile index={index} key={slot.tool} slot={slot} />
        ))}
      </div>
    </main>
  );
}
