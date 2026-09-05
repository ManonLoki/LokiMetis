import { useQuery } from "@tanstack/react-query";
import { useState, type MouseEvent, type ReactElement } from "react";
import { useTranslation } from "react-i18next";

import {
  closePetOverlay,
  getMonitorImageBytes,
  getMonitorSettings,
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
  const view = useQuery({
    queryFn: getPetOverlayView,
    queryKey: ["pet-overlay-view"],
    refetchInterval: 1000,
  });
  const settings = useQuery({
    queryFn: getMonitorSettings,
    queryKey: ["monitor-settings"],
    refetchInterval: 1000,
  });
  const slots = view.data?.slots ?? [];
  const showCloseControl = settings.data?.petCloseControlVisible ?? true;
  const [settingsOpen, setSettingsOpen] = useState(false);

  /** 左键交给原生拖动；位置由宿主 WindowEvent::Moved 记录。关闭按钮与设置面板不拖动。 */
  const onMouseDown = (event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement).closest("[data-pet-control]")) return;
    void startPetOverlayDrag();
  };

  /** 右键弹出设置窗口并拦住浏览器默认菜单。 */
  const onContextMenu = (event: MouseEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    setSettingsOpen(true);
  };

  return (
    <main
      aria-label={t("monitor.pet.shellAria")}
      className="pet-shell"
      data-testid="pet-overlay-page"
      onContextMenu={onContextMenu}
      onMouseDown={onMouseDown}
    >
      {showCloseControl ? (
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
      ) : null}
      {settingsOpen ? (
        <section
          aria-label={t("monitor.pet.settingsTitle")}
          className="pet-settings"
          data-pet-control=""
          data-testid="pet-overlay-settings"
          role="dialog"
        >
          <strong>{t("monitor.pet.settingsTitle")}</strong>
          <p>{t("monitor.pet.settingsDescription")}</p>
          <button
            onClick={() => {
              setSettingsOpen(false);
            }}
            type="button"
          >
            {t("monitor.pet.settingsClose")}
          </button>
        </section>
      ) : null}
      <div className="pet-grid">
        {slots.map((slot, index) => (
          <PetOverlayTile index={index} key={slot.tool} slot={slot} />
        ))}
      </div>
    </main>
  );
}
