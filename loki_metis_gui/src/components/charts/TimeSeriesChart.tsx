import { useState, type MouseEvent } from 'react';

import { Group, Text } from '@mantine/core';

import { CHART_WIDTH, axisTickValues } from './chart-geometry';

/** 一条共享纵轴的时间序列。 */
export interface TimeSeries {
  /** 稳定系列键。 */
  id: string;
  /** 当前语言下的图例名称。 */
  label: string;
  /** SVG 线条与图例颜色。 */
  color: string;
  /** 与横轴桶一一对应；null 表示上游未提供。 */
  values: Array<number | null>;
}

/** 定义概览时间序列图的桶、单位和可访问标签。 */
interface TimeSeriesChartProps {
  /** 图表可访问名称。 */
  ariaLabel: string;
  /** 横轴短标签。 */
  labels: string[];
  /** 共享单位的一个或多个系列。 */
  series: TimeSeries[];
  /** 纵轴刻度格式化。 */
  formatValue: (value: number) => string;
  /** 悬停浮层使用的精确值格式化。 */
  formatTooltipValue: (value: number) => string;
}

const WIDTH = CHART_WIDTH;
const HEIGHT = 300;
const PLOT_LEFT = 72;
const PLOT_RIGHT = 22;
const PLOT_TOP = 18;
const PLOT_BOTTOM = 48;
const Y_STEPS = 4;
const TOOLTIP_WIDTH = 240;
const TOOLTIP_ROW_HEIGHT = 20;

/** 将桶索引映射到 SVG 绘图区的横坐标。 */
function xPosition(index: number, count: number): number {
  const width = WIDTH - PLOT_LEFT - PLOT_RIGHT;
  return count <= 1 ? PLOT_LEFT + width / 2 : PLOT_LEFT + (index / (count - 1)) * width;
}

/** 将度量值映射到 SVG 绘图区的纵坐标。 */
function yPosition(value: number, maximum: number): number {
  const height = HEIGHT - PLOT_TOP - PLOT_BOTTOM;
  return PLOT_TOP + height - (value / maximum) * height;
}

/** 为连续可用值生成 SVG 折线路径，并在缺失值处断开。 */
function linePath(values: Array<number | null>, maximum: number): string {
  let path = '';
  let continuing = false;
  values.forEach((value, index) => {
    if (value === null) {
      continuing = false;
      return;
    }
    path += `${continuing ? ' L' : ' M'} ${xPosition(index, values.length)} ${yPosition(value, maximum)}`;
    continuing = true;
  });
  return path;
}

/** 为不同桶数量选择有限且稳定的横轴刻度索引。 */
function tickIndices(count: number): number[] {
  if (count <= 1) return [0];
  const tickCount = Math.min(count, 7);
  return Array.from(
    new Set(
      Array.from({ length: tickCount }, (_, index) =>
        Math.round((index * (count - 1)) / (tickCount - 1)),
      ),
    ),
  );
}

/** 把鼠标横坐标映射到完整 SVG 绘图区内最近的时间桶。 */
function nearestBucketIndex(clientX: number, bounds: DOMRect, count: number): number | null {
  if (count === 0 || bounds.width <= 0) return null;
  if (count === 1) return 0;
  const scale = bounds.width / WIDTH;
  const plotLeft = bounds.left + PLOT_LEFT * scale;
  const plotWidth = (WIDTH - PLOT_LEFT - PLOT_RIGHT) * scale;
  const ratio = Math.max(0, Math.min(1, (clientX - plotLeft) / plotWidth));
  return Math.round(ratio * (count - 1));
}

/** 优先把浮层放在指示线右侧，空间不足时完整翻转到左侧。 */
function tooltipXPosition(index: number, count: number): number {
  const anchor = xPosition(index, count);
  const right = anchor + 12;
  return right + TOOLTIP_WIDTH <= WIDTH - PLOT_RIGHT
    ? right
    : Math.max(PLOT_LEFT, anchor - TOOLTIP_WIDTH - 12);
}

/** 无第三方绘图库的响应式 SVG 折线图；缺失分项会断线而不是伪装成零。 */
export function TimeSeriesChart({
  ariaLabel,
  labels,
  series,
  formatValue,
  formatTooltipValue,
}: TimeSeriesChartProps) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const maximum = Math.max(
    1,
    ...series.flatMap((item) => item.values).filter((value): value is number => value !== null),
  );
  const ticks = tickIndices(labels.length);
  const activeIndex = hoveredIndex !== null && hoveredIndex < labels.length ? hoveredIndex : null;
  const tooltipHeight = 32 + series.length * TOOLTIP_ROW_HEIGHT;

  /** 在整个绘图区内按当前位置选中最近时间桶，不要求命中折线点。 */
  const updateHoveredBucket = (event: MouseEvent<SVGRectElement>) => {
    const svg = event.currentTarget.ownerSVGElement;
    if (!svg) return;
    setHoveredIndex(nearestBucketIndex(event.clientX, svg.getBoundingClientRect(), labels.length));
  };

  return (
    <div className="chart-figure">
      <svg
        aria-label={ariaLabel}
        className="time-series-chart"
        preserveAspectRatio="xMidYMid meet"
        role="img"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      >
        <title>{ariaLabel}</title>
        {axisTickValues(maximum, Y_STEPS)
          .slice()
          .reverse()
          .map((value, index) => {
            const y = PLOT_TOP + (index / Y_STEPS) * (HEIGHT - PLOT_TOP - PLOT_BOTTOM);
            return (
              <g key={index}>
                <line
                  className="chart-grid-line"
                  x1={PLOT_LEFT}
                  x2={WIDTH - PLOT_RIGHT}
                  y1={y}
                  y2={y}
                />
                <text className="chart-axis-label" textAnchor="end" x={PLOT_LEFT - 10} y={y + 4}>
                  {formatValue(Math.round(value))}
                </text>
              </g>
            );
          })}
        {ticks.map((index) => (
          <text
            className="chart-axis-label"
            key={index}
            textAnchor={index === 0 ? 'start' : index === labels.length - 1 ? 'end' : 'middle'}
            x={xPosition(index, labels.length)}
            y={HEIGHT - 18}
          >
            {labels[index]}
          </text>
        ))}
        {series.map((item) => (
          <g data-chart-series={item.id} key={item.id}>
            <path
              className="chart-series-line"
              d={linePath(item.values, maximum)}
              fill="none"
              stroke={item.color}
            />
            {item.values.map((value, index) =>
              value === null ? null : (
                <circle
                  cx={xPosition(index, item.values.length)}
                  cy={yPosition(value, maximum)}
                  data-chart-point={item.id}
                  fill={item.color}
                  key={`${item.id}-${index}`}
                  r={3.2}
                >
                  <title>{`${item.label} · ${labels[index]} · ${formatTooltipValue(value)}`}</title>
                </circle>
              ),
            )}
          </g>
        ))}
        {activeIndex === null ? null : (
          <g className="chart-hover-overlay" data-chart-tooltip>
            <line
              className="chart-hover-line"
              x1={xPosition(activeIndex, labels.length)}
              x2={xPosition(activeIndex, labels.length)}
              y1={PLOT_TOP}
              y2={HEIGHT - PLOT_BOTTOM}
            />
            {series.map((item) => {
              const value = item.values[activeIndex];
              return value === null || value === undefined ? null : (
                <circle
                  className="chart-hover-point"
                  cx={xPosition(activeIndex, labels.length)}
                  cy={yPosition(value, maximum)}
                  fill={item.color}
                  key={item.id}
                  r={5}
                />
              );
            })}
            <g
              transform={`translate(${tooltipXPosition(activeIndex, labels.length)} ${PLOT_TOP + 8})`}
            >
              <rect
                className="chart-tooltip-panel"
                height={tooltipHeight}
                rx={8}
                width={TOOLTIP_WIDTH}
              />
              <text className="chart-tooltip-heading" x={12} y={21}>
                {labels[activeIndex]}
              </text>
              {series.map((item, index) => {
                const value = item.values[activeIndex];
                const rowY = 40 + index * TOOLTIP_ROW_HEIGHT;
                return (
                  <g key={item.id}>
                    <circle cx={15} cy={rowY - 4} fill={item.color} r={3.5} />
                    <text className="chart-tooltip-label" x={25} y={rowY}>
                      {item.label}
                    </text>
                    <text
                      className="chart-tooltip-value"
                      textAnchor="end"
                      x={TOOLTIP_WIDTH - 12}
                      y={rowY}
                    >
                      {value === null || value === undefined ? '—' : formatTooltipValue(value)}
                    </text>
                  </g>
                );
              })}
            </g>
          </g>
        )}
        <rect
          className="chart-hover-target"
          data-chart-hover-target
          fill="transparent"
          height={HEIGHT - PLOT_TOP - PLOT_BOTTOM}
          onMouseLeave={() => setHoveredIndex(null)}
          onMouseMove={updateHoveredBucket}
          width={WIDTH - PLOT_LEFT - PLOT_RIGHT}
          x={PLOT_LEFT}
          y={PLOT_TOP}
        />
      </svg>
      <Group aria-label={ariaLabel} className="chart-legend" gap="md" role="list">
        {series.map((item) => (
          <Group gap={6} key={item.id} role="listitem" wrap="nowrap">
            <span
              aria-hidden="true"
              className="chart-legend-swatch"
              style={{ color: item.color }}
            />
            <Text size="xs">{item.label}</Text>
          </Group>
        ))}
      </Group>
    </div>
  );
}
