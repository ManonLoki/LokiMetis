import type { UiMessageCode } from "./usage-types-detail";

// 本文件是 Rust 后端 `dto.rs`（以及 core 里若干类型）序列化出的 JSON
// 形状在 TypeScript 侧的镜像：Rust 字段用 `#[serde(rename_all =
// "camelCase")]` 转成驼峰命名后，这里的字段名和类型就应该逐一对应。
// 两侧独立维护是有意为之——前端不直接依赖 Rust 类型定义，只依赖“通过
// IPC 传过来的 JSON 长什么样”这个契约，改动任一侧都需要同步确认这份
// 契约仍然成立（这类改动通常配合测试断言序列化后的 JSON 形状）。

/** 标识当前查看和操作的 Agent 客户端。 */
export type AgentClientKind = "codex" | "claudeCode" | "grokBuildCli";

/** 统一 AI 目录中可映射到看板的稳定值；WorkBuddy 仍使用独立只读统计开关。 */
export type AvailableAiTypeValue = AgentClientKind | "workbuddy";

/** 后端统一 AI 目录投影到看板的可用选项。 */
export interface AvailableAiTypeDto {
  /** 不参与逻辑判断的产品展示名。 */
  name: string;
  /** 看板物理客户端或 WorkBuddy 独立视图的稳定映射值。 */
  value: AvailableAiTypeValue;
}

/** 标识可在用量界面选择的本机客户端。 */
export type UsageClientKind = AgentClientKind;

/** 标识概览与调用页可选择的只读视图；`all` 与 `workbuddy` 都不是物理扫描客户端。 */
export type UsageViewKind = "all" | AgentClientKind | "workbuddy";

/** 标识指标事实来自哪一类本机记录。 */
export type ProviderKind =
  | "rolloutJsonl"
  | "claudeTranscriptJsonl"
  | "grokSessionJsonl"
  | "combinedLocalAgents"
  | "workbuddyProjectJsonl";

/** 标识指标覆盖的业务范围，前端不得跨范围合并数值。 */
export type MetricScope = "deviceObserved" | "rootObserved" | "threadObserved";

/** 描述事实相对刷新策略的时效。 */
export type Freshness = "fresh" | "stale" | "expired" | "unknown";

/** 描述已知扫描或 provider 覆盖程度。 */
export type Completeness = "complete" | "partial" | "unknown";

/** 描述数据是直接事实、受控推算还是疑似冲突。 */
export type Confidence = "exact" | "derived" | "suspected";

/** 包装带来源、范围和质量元数据的可展示事实。 */
export interface MetricFactDto<T> {
  /** 经过后端规范化的业务值。 */
  value: T;
  /** 产生该值的 provider。 */
  provider: ProviderKind;
  /** 该值覆盖的独立业务范围。 */
  scope: MetricScope;
  /** 完成观测时的 Unix 毫秒时间戳。 */
  observedAtEpochMs: number;
  /** 当前事实的新鲜度。 */
  freshness: Freshness;
  /** 当前事实的覆盖完整度。 */
  completeness: Completeness;
  /** 当前事实的置信度。 */
  confidence: Confidence;
  /** 不含敏感信息的协议或解析器版本。 */
  sourceVersion: string | null;
}

/** 描述一次调用或本机聚合中的 Token 字段。 */
export interface TokenUsageDto {
  /** 全部输入 Token，包含缓存输入。 */
  inputTokens: number;
  /** 输入 Token 中由缓存读取提供的子集。 */
  cachedInputTokens: number | null;
  /** 上游提供时的缓存写入 Token。 */
  cacheWriteInputTokens: number | null;
  /** 全部输出 Token。 */
  outputTokens: number;
  /** 输出中的推理分析维度。 */
  reasoningOutputTokens: number | null;
  /** 上游单次总量或受控推算总量。 */
  totalTokens: number;
  /** 总量是否由输入加输出推算。 */
  totalIsDerived: boolean;
}

/** 描述已经去重的本机用量聚合。 */
export interface LocalUsageAggregateDto {
  /** 逐字段汇总的 Token。 */
  tokens: TokenUsageDto;
  /** canonical 调用数量。 */
  callCount: number;
  /** 发生缓存读取的调用数量。 */
  cachedReadCallCount: number | null;
  /** 去重后的线程数量。 */
  threadCount: number;
  /** 当前窗口 canonical provenance 中的数据根数量。 */
  rootCount: number;
  /** 去重后的来源文件数量。 */
  sourceCount: number;
  /** 活动、归档或复制来源的重复数量。 */
  duplicateSourceCount: number;
  /** 相对单根中最完整的来源观察基线、来自其他数据根的重复观察数量。 */
  crossRootDuplicateSourceCount: number;
  /** 缓存读取占比基点数，零输入时不适用。 */
  cacheReadBasisPoints: number | null;
  /** 聚合事实的最低置信度。 */
  confidence: Confidence;
}

/** 标识概览与用量中唯一允许的六个日历统计窗口，不提供全量累计口径。 */
export type UsageWindow =
  "today" | "yesterday" | "thisWeek" | "lastWeek" | "thisMonth" | "lastMonth";

/** 区分本机索引尚未建立、需要重扫、已扫描空结果与当前可用调用。 */
export type LocalIndexState = "notScanned" | "needsRescan" | "readyNoCalls" | "ready";

/** 描述一个本机时间窗口的可追溯事实。 */
export interface WindowUsageDto {
  /** 稳定时间窗口键。 */
  window: UsageWindow;
  /** 当前窗口中的本机可观测事实。 */
  fact: MetricFactDto<LocalUsageAggregateDto>;
}

/** 描述本机记录区块中的各时间窗口。 */
export interface LocalRecordsSectionDto {
  /** 后端从 SQLite 扫描与 parser 来源事实推导的稳定索引状态。 */
  indexState: LocalIndexState;
  /** 保持相互一致口径的本机时间窗口。 */
  windows: WindowUsageDto[];
}

/** 描述概览 command 的完整稳定响应。 */
export interface UsageOverviewDto {
  /** 真实业务 command 尚未装配时保持实施门禁。 */
  productDefinitionRequired: boolean;
  /** 实施门禁启用时展示的真实阶段说明。 */
  implementationMessage: string | null;
  /** 可由前端按当前语言渲染的稳定降级说明代码。 */
  implementationMessageCode?: UiMessageCode | null;
  /** 业务装配完成后提供本机记录区块。 */
  localRecords: LocalRecordsSectionDto | null;
}

export * from "./usage-types-detail";
export * from "./usage-types-chart";
