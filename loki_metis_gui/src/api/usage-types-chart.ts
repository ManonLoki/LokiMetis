import type {
  LocalIndexState,
  LocalUsageAggregateDto,
  MetricFactDto,
  UsageViewKind,
  UsageWindow,
} from "./usage-types";
import type { UsageGroupDto, UsageMeasureDto } from "./usage-types-detail";

/** 图表分布允许选择的固定维度；Agent 只适用于全部视图。 */
export type UsageChartDimension =
  "agent" | "model" | "reasoningEffort" | "project" | "thread" | "root";

/** 后端根据窗口固定选择的横轴粒度。 */
export type UsageChartGranularity = "hour" | "day";

/** 图表横轴上的完整时间桶。 */
export interface UsageChartBucketDto {
  /** 小时或日期稳定键。 */
  key: string;
  /** 横轴短标签。 */
  label: string;
  /** 当前尚未结束的小时或日期。 */
  inProgress: boolean;
  /** 桶内可加总计量。 */
  measure: UsageMeasureDto;
}

/** 同一后端快照生成的趋势、摘要和维度分布。 */
export interface UsageChartDto {
  /** 当前固定自然日窗口。 */
  window: UsageWindow;
  /** 当前固定分布维度。 */
  dimension: UsageChartDimension;
  /** 单日为小时，多日为日。 */
  granularity: UsageChartGranularity;
  /** 与同一快照绑定的索引状态。 */
  indexState: LocalIndexState;
  /** 窗口下界 Unix 毫秒。 */
  lowerBoundEpochMs: number;
  /** 统一观测时刻 Unix 毫秒。 */
  observedAtEpochMs: number;
  /** 窗口总量与质量元数据。 */
  fact: MetricFactDto<LocalUsageAggregateDto>;
  /** 从最旧到最新且不省略零值的时间桶。 */
  buckets: UsageChartBucketDto[];
  /** 确定性 Top 10 分组。 */
  groups: UsageGroupDto[];
  /** Top 10 之外的可对账其余项。 */
  remainder: UsageGroupDto | null;
}

/** 趋势图中可同时展示的 Token 指标。 */
export type UsageChartTokenMetric =
  | "totalTokens"
  | "inputTokens"
  | "cachedInputTokens"
  | "cacheWriteInputTokens"
  | "outputTokens"
  | "reasoningOutputTokens";

/** 分布图可选择 Token 指标或调用数；调用数不与 Token 共用坐标轴。 */
export type UsageChartDistributionMetric = UsageChartTokenMetric | "callCount";

/** 一个只读视图需要恢复的完整图表展示偏好。 */
export interface ChartPreferencesDto {
  /** 概览趋势窗口。 */
  overviewWindow: UsageWindow;
  /** 用量分布窗口。 */
  usageWindow: UsageWindow;
  /** 非空且不重复的概览 Token 指标。 */
  tokenMetrics: UsageChartTokenMetric[];
  /** 当前用量分组维度。 */
  dimension: UsageChartDimension;
  /** 当前用量指标。 */
  distributionMetric: UsageChartDistributionMetric;
}

/** 「全部」与三个物理 Agent 互不覆盖的图表偏好集合。 */
export type ChartPreferencesByViewDto = Record<UsageViewKind, ChartPreferencesDto>;
