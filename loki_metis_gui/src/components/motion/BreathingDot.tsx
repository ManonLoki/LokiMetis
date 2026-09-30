/** 只用于真实的“进行中”状态：缓慢呼吸的指示点，动画由合成器上的 CSS 关键帧驱动。 */
export function BreathingDot() {
  return (
    <span
      aria-hidden="true"
      className="breathing-dot inline-block size-2 flex-none rounded-full bg-(--app-accent)"
    />
  );
}
