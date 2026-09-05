/** 图表 SVG 视口固定宽度；折线图与分布图共享同一响应式缩放基准。 */
export const CHART_WIDTH = 920;

/** 生成从 0 到 `maximum` 均匀分布的 `steps + 1` 个数值轴刻度值。 */
export function axisTickValues(maximum: number, steps: number): number[] {
  return Array.from({ length: steps + 1 }, (_, index) => (maximum * index) / steps);
}
