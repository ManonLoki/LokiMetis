import { Text } from '@mantine/core';

import { CHART_WIDTH, axisTickValues } from './chart-geometry';

/** 分布图中的一条可访问横向条。 */
export interface DistributionRow {
  /** 后端稳定分组 ID。 */
  id: string;
  /** 已本地化且完成消歧的标签。 */
  label: string;
  /** 当前选择指标的值；null 表示未提供。 */
  value: number | null;
  /** 后端按总 Token 计算的份额文案。 */
  shareLabel: string;
  /** 是否为 Top 10 之外的合并项。 */
  remainder: boolean;
}

/** 定义固定横向分布柱状图的对账数据与格式化输入。 */
interface DistributionChartProps {
  /** 图表可访问名称。 */
  ariaLabel: string;
  /** 横轴刻度值格式化。 */
  formatAxisValue: (value: number) => string;
  /** 当前指标的值格式化。 */
  formatValue: (value: number | null) => string;
  /** 空分布说明。 */
  emptyLabel: string;
  /** Top 10 与可选其余项。 */
  rows: DistributionRow[];
}

const WIDTH = CHART_WIDTH;
const PLOT_LEFT = 220;
const PLOT_RIGHT = 150;
const PLOT_TOP = 20;
const PLOT_BOTTOM = 44;
const ROW_HEIGHT = 52;
const BAR_HEIGHT = 22;
const X_STEPS = 4;

/** 对 SVG 分类轴做稳定截断，完整标签仍保留在 title 与可访问描述中。 */
function compactLabel(label: string): string {
  const characters = Array.from(label);
  return characters.length <= 24 ? label : `${characters.slice(0, 23).join('')}…`;
}

/** 按确定性后端顺序绘制带横轴、网格和精确值的 SVG 条形图。 */
export function DistributionChart({
  ariaLabel,
  formatAxisValue,
  formatValue,
  emptyLabel,
  rows,
}: DistributionChartProps) {
  const maximum = Math.max(1, ...rows.map((row) => row.value ?? 0));
  if (rows.length === 0) {
    return (
      <Text c="dimmed" p="xl" ta="center">
        {emptyLabel}
      </Text>
    );
  }

  const height = PLOT_TOP + PLOT_BOTTOM + rows.length * ROW_HEIGHT;
  const plotWidth = WIDTH - PLOT_LEFT - PLOT_RIGHT;
  const plotBottom = height - PLOT_BOTTOM;

  return (
    <div className="distribution-chart-frame">
      <svg
        aria-label={ariaLabel}
        className="distribution-chart"
        preserveAspectRatio="xMidYMid meet"
        role="img"
        viewBox={`0 0 ${WIDTH} ${height}`}
      >
        <title>{ariaLabel}</title>
        <desc>
          {rows
            .map((row) => `${row.label}: ${formatValue(row.value)}; ${row.shareLabel}`)
            .join(' · ')}
        </desc>
        {axisTickValues(maximum, X_STEPS).map((value, index) => {
          const ratio = index / X_STEPS;
          const x = PLOT_LEFT + ratio * plotWidth;
          return (
            <g aria-hidden="true" key={index}>
              <line
                className="distribution-grid-line"
                data-distribution-grid-line=""
                x1={x}
                x2={x}
                y1={PLOT_TOP}
                y2={plotBottom}
              />
              <text
                className="distribution-axis-label"
                textAnchor={index === 0 ? 'start' : index === X_STEPS ? 'end' : 'middle'}
                x={x}
                y={height - 15}
              >
                {formatAxisValue(Math.round(value))}
              </text>
            </g>
          );
        })}
        {rows.map((row, index) => {
          const rowTop = PLOT_TOP + index * ROW_HEIGHT;
          const valueLabel = formatValue(row.value);
          const barWidth =
            row.value === null ? null : (Math.max(0, row.value) / maximum) * plotWidth;
          return (
            <g
              aria-label={`${row.label}: ${valueLabel}; ${row.shareLabel}`}
              className="distribution-chart-row"
              key={row.id}
              tabIndex={0}
            >
              <title>{`${row.label} · ${valueLabel} · ${row.shareLabel}`}</title>
              <text
                className={
                  row.remainder
                    ? 'distribution-category-label remainder'
                    : 'distribution-category-label'
                }
                textAnchor="end"
                x={PLOT_LEFT - 12}
                y={rowTop + 20}
              >
                {compactLabel(row.label)}
              </text>
              <text
                className="distribution-share-label"
                textAnchor="end"
                x={PLOT_LEFT - 12}
                y={rowTop + 38}
              >
                {row.shareLabel}
              </text>
              <rect
                aria-hidden="true"
                className="distribution-bar-track"
                height={BAR_HEIGHT}
                rx={BAR_HEIGHT / 2}
                width={plotWidth}
                x={PLOT_LEFT}
                y={rowTop + 8}
              />
              {barWidth === null ? null : (
                <rect
                  aria-hidden="true"
                  className={row.remainder ? 'distribution-bar remainder' : 'distribution-bar'}
                  data-distribution-bar={row.id}
                  height={BAR_HEIGHT}
                  rx={BAR_HEIGHT / 2}
                  width={barWidth}
                  x={PLOT_LEFT}
                  y={rowTop + 8}
                />
              )}
              <text
                className="distribution-value-label"
                textAnchor="end"
                x={WIDTH - 16}
                y={rowTop + 23}
              >
                {valueLabel}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}
