import { motion, useReducedMotion } from "motion/react";
import { memo } from "react";

/** 只用于真实的“进行中”状态：缓慢呼吸的指示点，独立成叶子组件避免带动父级重渲染。 */
export const BreathingDot = memo(function BreathingDot() {
  const reduceMotion = useReducedMotion();
  return (
    <motion.span
      animate={reduceMotion ? undefined : { opacity: [1, 0.35, 1], scale: [1, 0.82, 1] }}
      aria-hidden="true"
      className="inline-block size-2 flex-none rounded-full bg-(--app-accent)"
      transition={{ duration: 1.8, ease: "easeInOut", repeat: Infinity }}
    />
  );
});
