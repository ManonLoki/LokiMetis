import { useEffect } from "react";

/** 面板选择器：指针经过时为其写入光标坐标变量，供 CSS 绘制聚光高亮。 */
const SPOTLIGHT_SELECTOR = ".spot-panel";

/** 在文档级委托 pointermove，只写 CSS 变量，不触发 React 重渲染。 */
export function useSpotlight(): void {
  useEffect(() => {
    let frame = 0;
    let cachedTarget: HTMLElement | null = null;
    let cachedRect: DOMRect | null = null;
    const handleMove = (event: PointerEvent) => {
      if (event.pointerType !== "mouse") return;
      const target = (event.target as Element | null)?.closest<HTMLElement>(
        SPOTLIGHT_SELECTOR,
      );
      if (!target) return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        // 同一面板内连续移动只读一次布局矩形，切换面板时再刷新。
        if (cachedTarget !== target || cachedRect === null) {
          cachedTarget = target;
          cachedRect = target.getBoundingClientRect();
        }
        target.style.setProperty("--spot-x", `${event.clientX - cachedRect.left}px`);
        target.style.setProperty("--spot-y", `${event.clientY - cachedRect.top}px`);
      });
    };
    // 滚动或窗口变化后缓存矩形失效。
    const invalidate = () => {
      cachedTarget = null;
      cachedRect = null;
    };
    document.addEventListener("pointermove", handleMove, { passive: true });
    document.addEventListener("scroll", invalidate, { capture: true, passive: true });
    window.addEventListener("resize", invalidate);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointermove", handleMove);
      document.removeEventListener("scroll", invalidate, { capture: true });
      window.removeEventListener("resize", invalidate);
    };
  }, []);
}
