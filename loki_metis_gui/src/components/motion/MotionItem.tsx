import { motion, useReducedMotion } from "motion/react";
import type { ReactElement, ReactNode } from "react";

/** 列表项依序上浮入场，并在排序或筛选变化时用 layout 平滑换位。 */
export function MotionItem({
  children,
  index,
}: {
  children: ReactNode;
  index: number;
}): ReactElement {
  const reduceMotion = useReducedMotion();
  return (
    <motion.div
      animate={{ opacity: 1, y: 0 }}
      initial={reduceMotion || index > 11 ? false : { opacity: 0.4, y: 12 }}
      transition={{
        damping: 20,
        delay: Math.min(index, 8) * 0.04,
        stiffness: 100,
        type: "spring",
      }}
    >
      {children}
    </motion.div>
  );
}
