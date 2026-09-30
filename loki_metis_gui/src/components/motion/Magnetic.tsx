import { motion, useMotionValue, useReducedMotion, useSpring } from "motion/react";
import type { PointerEvent, ReactElement, ReactNode } from "react";

/** 磁吸位移的最大像素与感应强度，保持克制以免影响点击精度。 */
const MAGNETIC_PULL = 0.18;
const MAGNETIC_LIMIT_PX = 6;

/** 让子控件在指针靠近时轻微向指针偏移；只写 motion value，不触发 React 重渲染。 */
export function Magnetic({ children }: { children: ReactNode }): ReactElement {
  const reduceMotion = useReducedMotion();
  const x = useMotionValue(0);
  const y = useMotionValue(0);
  const springX = useSpring(x, { damping: 20, stiffness: 150 });
  const springY = useSpring(y, { damping: 20, stiffness: 150 });

  if (reduceMotion) return <>{children}</>;

  const clamp = (value: number) =>
    Math.max(-MAGNETIC_LIMIT_PX, Math.min(MAGNETIC_LIMIT_PX, value));
  const handleMove = (event: PointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    x.set(clamp((event.clientX - rect.left - rect.width / 2) * MAGNETIC_PULL));
    y.set(clamp((event.clientY - rect.top - rect.height / 2) * MAGNETIC_PULL));
  };
  const handleLeave = () => {
    x.set(0);
    y.set(0);
  };

  return (
    <motion.div
      onPointerLeave={handleLeave}
      onPointerMove={handleMove}
      style={{ display: "inline-block", x: springX, y: springY }}
    >
      {children}
    </motion.div>
  );
}
