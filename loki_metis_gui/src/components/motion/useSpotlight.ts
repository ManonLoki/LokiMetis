import { useEffect } from "react";

/** 面板选择器：指针经过时为其写入光标坐标变量，供 CSS 绘制聚光高亮。 */
const SPOTLIGHT_SELECTOR = ".surface-card, .metric-card, .chart-panel, .breakdown-panel";

/** 在文档级委托 pointermove，只写 CSS 变量，不触发 React 重渲染。 */
export function useSpotlight(): void {
  useEffect(() => {
    let frame = 0;
    const handleMove = (event: PointerEvent) => {
      if (event.pointerType !== "mouse") return;
      const target = (event.target as Element | null)?.closest<HTMLElement>(
        SPOTLIGHT_SELECTOR,
      );
      if (!target) return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const rect = target.getBoundingClientRect();
        target.style.setProperty("--spot-x", `${event.clientX - rect.left}px`);
        target.style.setProperty("--spot-y", `${event.clientY - rect.top}px`);
      });
    };
    document.addEventListener("pointermove", handleMove, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointermove", handleMove);
    };
  }, []);
}
