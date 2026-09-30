import { animate, useReducedMotion } from "motion/react";
import { useEffect, useRef } from "react";

/** 数值变化时从旧值平滑滚动到新值；首次挂载直接显示终值，保证读屏器与测试读到确定文本。 */
export function CountUp({
  format,
  value,
}: {
  format: (value: number) => string;
  value: number;
}) {
  const reduceMotion = useReducedMotion();
  const ref = useRef<HTMLSpanElement>(null);
  const previous = useRef(value);

  useEffect(() => {
    const node = ref.current;
    const from = previous.current;
    previous.current = value;
    if (!node || reduceMotion || from === value) return;
    const controls = animate(from, value, {
      duration: 0.8,
      ease: [0.16, 1, 0.3, 1],
      onComplete: () => {
        node.textContent = format(value);
      },
      onUpdate: (latest) => {
        node.textContent = format(Math.round(latest));
      },
    });
    return () => controls.stop();
  }, [format, reduceMotion, value]);

  return <span ref={ref}>{format(value)}</span>;
}
