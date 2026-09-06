// 延续 usage-types.ts 的 DTO 镜像职责，只是拆到第二个文件里——
// 这里主要放“稳定代码”类枚举（UiMessageCode/DisplayLabelCode/
// SourceDiscoveryCode 等）：后端只返回语言无关的代码字符串，具体展示
// 文案由前端 i18n 资源按当前语言查表得到（参见 i18n/backend-labels.ts），
// 这样切换语言不需要重新请求后端数据。
import type {
  AgentClientKind,
  AvailableAiTypeDto,
  Confidence,
  LocalIndexState,
  LocalUsageAggregateDto,
  MetricFactDto,
  TokenUsageDto,
  UsageWindow,
} from "./usage-types";

/** 看板页头时间标准模式。 */
export type TimeStandardMode = "local" | "custom";

/** 划分本机查看今日、昨日、本周、上周、本月和上月所用的时间标准。 */
export interface TimeStandard {
  /** 当地时间（设备时区）或 UTC 时间。 */
  mode: TimeStandardMode;
  /** UTC 模式固定为 `UTC`；当地时间模式为空。 */
  customTimeZone: string | null;
}

/** 缺省当地时间标准。 */
export const LOCAL_TIME_STANDARD: TimeStandard = { mode: "local", customTimeZone: null };

/** UTC 时间标准；页头「UTC时间」只写入该值。 */
export const UTC_TIME_STANDARD: TimeStandard = { mode: "custom", customTimeZone: "UTC" };

/** 用量读取身份：当地时间与 UTC 不得复用同一份已渲染快照。 */
export function timeStandardQueryKey(
  standard: TimeStandard | undefined,
): [TimeStandardMode, string | null] {
  const saved = standard ?? LOCAL_TIME_STANDARD;
  return saved.mode === "custom" ? ["custom", "UTC"] : ["local", null];
}

/** 后端固定可见消息的稳定代码。 */
export type UiMessageCode =
  | "overviewLocalIndexUnavailable"
  | "overviewWorkbuddyUnavailable"
  | "scanIdle"
  | "scanRunning"
  | "scanCancelling"
  | "scanCancelled"
  | "scanCompleted"
  | "scanFailed"
  | "sourceAddCancelled"
  | "sourceRegistered"
  | "sourceAlreadyRegistered"
  | "sourceManualDeepSearchStarted"
  | "sourceManualDeepSearchEmpty"
  | "sourceEnabled"
  | "sourceDisabled"
  | "sourceRenamed"
  | "sourceRemoved"
  | "primaryChanged"
  | "primaryAlreadySelected"
  | "primaryCleared"
  | "primaryNotSet"
  | "indexCleared";

/** 固定占位、推理强度与匿名短标签的稳定展示语义。 */
export type DisplayLabelCode =
  | "literal"
  | "unknownModel"
  | "unknownReasoningEffort"
  | "reasoningNone"
  | "reasoningMinimal"
  | "reasoningLow"
  | "reasoningMedium"
  | "reasoningHigh"
  | "reasoningXHigh"
  | "uncategorizedProject"
  | "project"
  | "unknownThread"
  | "thread"
  | "unnamedRoot"
  | "remainder";

/** 数据根发现方式的稳定代码。 */
export type SourceDiscoveryCode =
  | "defaultRoot"
  | "codexEnvironment"
  | "claudeEnvironment"
  | "userRegistered"
  | "fullDevice"
  | "metadataDiscovery";

/** 扫描阶段的稳定代码。 */
export type ScanScopeCode =
  | "registeredRoots"
  | "localFixedVolumes"
  | "discoveringVolumes"
  | "discoveryFinished"
  | "indexingRoots";

/** 描述调用页可跨页面保存的固定安全筛选项。 */
export interface UsageCallFiltersDto {
  /** 模型技术标签；空值表示全部。 */
  model: string | null;
  /** 推理强度技术标签；空值表示全部。 */
  reasoningEffort: string | null;
  /** 匿名项目 ID；不得是路径。 */
  project: string | null;
  /** 内容无关的线程 ID。 */
  thread: string | null;
  /** 内容无关的数据根 ID。 */
  root: string | null;
}

/** 后端唯一允许的九种调用排序字段。 */
export type UsageCallSortField =
  | "occurredAt"
  | "model"
  | "reasoningEffort"
  | "inputTokens"
  | "cachedInputTokens"
  | "uncachedInputTokens"
  | "outputTokens"
  | "reasoningOutputTokens"
  | "totalTokens";

/** 后端唯一允许的调用排序方向。 */
export type UsageCallSortDirection = "asc" | "desc";

/** 描述一个完整类型化调用查询；页大小由 backend 固定。 */
export interface UsageCallsQueryDto {
  /** 五类固定等值筛选。 */
  filters: UsageCallFiltersDto;
  /** 固定可见列排序字段。 */
  sortField: UsageCallSortField;
  /** 升序或降序。 */
  sortDirection: UsageCallSortDirection;
  /** 上一页返回的不透明游标；首屏为空。 */
  cursor: string | null;
}

/** 描述单条本机调用，不包含正文或绝对路径。 */
export interface UsageCallItemDto {
  /** 内容无关的稳定行 ID。 */
  id: string;
  /** 产生当前调用的物理 Agent；全部视图用它区分跨库结果。 */
  client: AgentClientKind;
  /** 调用发生时间。 */
  occurredAtEpochMs: number;
  /** 受控项目末段或回退短标签。 */
  projectLabel: string;
  /** 项目标签展示语义。 */
  projectLabelCode?: DisplayLabelCode;
  /** 线程短标题或回退短标签。 */
  threadLabel: string;
  /** 线程标签展示语义。 */
  threadLabelCode?: DisplayLabelCode;
  /** 模型显示名。 */
  modelLabel: string;
  /** 模型标签展示语义。 */
  modelLabelCode?: DisplayLabelCode;
  /** 推理强度显示名。 */
  reasoningEffortLabel: string;
  /** 推理强度展示语义。 */
  reasoningEffortLabelCode?: DisplayLabelCode;
  /** 与 backend 排序口径一致的未缓存输入。 */
  uncachedInputTokens: number | null;
  /** 本机调用的可追溯 Token 事实。 */
  fact: MetricFactDto<TokenUsageDto>;
}

/** 描述一个不透明筛选 ID 与安全显示标签。 */
export interface UsageFilterOptionDto {
  /** backend 生成或验证的等值筛选 ID。 */
  id: string;
  /** 不含绝对路径的显示标签。 */
  label: string;
  /** 筛选标签展示语义。 */
  labelCode?: DisplayLabelCode;
  /** 同名标签的稳定序号；由界面按当前语言添加标点。 */
  disambiguationIndex?: number;
}

/** 描述调用页从完整 canonical 集合生成的可选筛选值。 */
export interface UsageAvailableFiltersDto {
  /** 当前结果中可用的模型名。 */
  models: UsageFilterOptionDto[];
  /** 当前结果中可用的推理强度。 */
  reasoningEfforts: UsageFilterOptionDto[];
  /** 当前结果中可用的项目别名。 */
  projects: UsageFilterOptionDto[];
  /** 当前结果中可用的线程短标签。 */
  threads: UsageFilterOptionDto[];
  /** 当前结果中可用的数据根别名。 */
  roots: UsageFilterOptionDto[];
}

/** 描述稳定调用查询的一页与完整筛选总数。 */
export interface UsageCallsPageDto {
  /** 与本页调用来自同一快照的索引四态。 */
  indexState: LocalIndexState;
  /** 本页事实的统一观测时刻。 */
  observedAtEpochMs: number;
  /** 后端固定页大小内的调用。 */
  items: UsageCallItemDto[];
  /** 从完整启用集合生成的安全筛选值。 */
  availableFilters: UsageAvailableFiltersDto;
  /** 应用当前筛选后的完整结果数。 */
  totalCount: number;
  /** 仍有下一页时返回的不透明游标。 */
  nextCursor: string | null;
}

/** 标识后端唯一允许的本机统计分组维度。 */
export type UsageDimension = "model" | "reasoningEffort" | "project" | "thread" | "root";

/** 描述可在互斥日期桶或分组之间逐字段安全相加的统计量。 */
export interface UsageMeasureDto {
  /** 全部 Token 字段的安全聚合。 */
  tokens: TokenUsageDto;
  /** canonical 逻辑调用数量。 */
  callCount: number;
  /** 发生缓存读取的调用数量。 */
  cachedReadCallCount: number | null;
  /** 所含调用被合并的物理来源观察数量。 */
  duplicateSourceCount: number;
  /** 缓存读取占比基点数，零输入时不适用。 */
  cacheReadBasisPoints: number | null;
  /** 所含事实中的最低置信度。 */
  confidence: Confidence;
}

/** 描述一个不会因零值而省略的本地自然日桶。 */
export interface UsageDailyBucketDto {
  /** 系统本地时区下的 YYYY-MM-DD 日期。 */
  localDate: string;
  /** 当前日期仍在进行中。 */
  inProgress: boolean;
  /** 该日期的可加总统计量。 */
  measure: UsageMeasureDto;
}

/** 描述一个 Top-N 分组或合并后的其余项。 */
export interface UsageGroupDto {
  /** 固定维度与内部键生成的稳定、不透明行 ID。 */
  id: string;
  /** 不含绝对路径或完整内部标识的展示标签。 */
  label: string;
  /** 分组标签展示语义。 */
  labelCode?: DisplayLabelCode;
  /** 同名标签的稳定序号；由界面按当前语言添加标点。 */
  disambiguationIndex?: number;
  /** 该组的可加总统计量。 */
  measure: UsageMeasureDto;
  /** 该组总 Token 占窗口总量的基点数。 */
  totalTokenShareBasisPoints: number | null;
  /** 标识本行是超过上限后合并的其余项。 */
  remainder: boolean;
}

/** 描述固定窗口、固定维度且可对账的本机统计响应。 */
export interface UsageStatisticsDto {
  /** 今日、昨日、本周、上周、本月或上月。 */
  window: UsageWindow;
  /** 当前固定分组维度。 */
  dimension: UsageDimension;
  /** 与统计调用和根记录来自同一 SQLite 快照的索引状态。 */
  indexState: LocalIndexState;
  /** 窗口闭区间下界。 */
  lowerBoundEpochMs: number;
  /** 单一快照的观测时间与上界。 */
  observedAtEpochMs: number;
  /** 窗口总计和覆盖、scope、质量元数据。 */
  fact: MetricFactDto<LocalUsageAggregateDto>;
  /** 从首日起完整排列的本地自然日桶。 */
  dailyBuckets: UsageDailyBucketDto[];
  /** 确定性排序后的前十个分组。 */
  groups: UsageGroupDto[];
  /** 超过前十个分组时合并的其余项。 */
  remainder: UsageGroupDto | null;
}

/** 描述一次发现或扫描的覆盖结论。 */
export type CoverageState = "complete" | "partial" | "cancelled" | "failed";

/** 描述数据根发现与读取的覆盖报告。 */
export interface CoverageReportDto {
  /** 当前覆盖结论。 */
  state: CoverageState;
  /** 已检查的数据根数。 */
  rootsScanned: number;
  /** 已发现的数据根数。 */
  rootsDiscovered: number;
  /** 权限拒绝数量。 */
  permissionDeniedCount: number;
  /** 网络卷、链接、损坏或策略跳过数量。 */
  skippedCount: number;
  /** 格式或解析警告数量。 */
  warningCount: number;
}

/** 描述一个授权数据根的安全可见状态。 */
export interface SourceRootDto {
  /** 后端生成的稳定根 ID。 */
  id: string;
  /** 用户别名或安全路径末段。 */
  alias: string;
  /** 数据根是否参与扫描。 */
  enabled: boolean;
  /** 首次确认、索引中、就绪或验证失败。 */
  activationState: RootActivationState;
  /** Codex 根是否为当前唯一主数据目录。 */
  isPrimary: boolean;
  /** 快速发现、自定义或全设备发现来源。 */
  discoveryLabel: string;
  /** 数据根来源的稳定代码。 */
  discoveryCode?: SourceDiscoveryCode;
  /** 当前根中符合所选客户端签名的会话文件数。 */
  fileCount: number;
  /** 当前根跳过的文件或目录数。 */
  skippedCount: number;
  /** 当前根扫描错误数。 */
  errorCount: number;
  /** 当前根被去重的来源数量。 */
  duplicateCount: number;
  /** 最近成功扫描时间。 */
  lastScanAtEpochMs: number | null;
}

/** 数据根首次索引激活状态。 */
export type RootActivationState =
  "confirmedUnindexed" | "indexing" | "ready" | "validationFailed";

/** 数据源发现任务生命周期。 */
export type RootDiscoveryState =
  "idle" | "running" | "complete" | "partial" | "cancelled" | "failed";

/** 平台元数据策略。 */
export type RootDiscoveryStrategy =
  "windowsSearch" | "macOsSpotlight" | "metadataTraversal";

/** 当前运行平台；界面只展示对应平台的数据源发现说明。 */
export type RootDiscoveryPlatform = "windows" | "macOs" | "other";

/** 用户选择的数据源发现范围。 */
export type RootDiscoveryScope = "userPriority" | "fullLocalVolumes" | "manualSubtree";

/** 手动添加命令的稳定结果类别。 */
export type ManualAddOutcome =
  "cancelled" | "registered" | "alreadyRegistered" | "deepSearchStarted";

/** 手动添加结果；路径永不进入响应。 */
export interface ManualAddSourceRootDto {
  outcome: ManualAddOutcome;
  changed: boolean;
  messageCode: UiMessageCode;
  discovery: RootDiscoveryStatusDto | null;
}

/** 无百分比的数据源发现状态。 */
export interface RootDiscoveryStatusDto {
  state: RootDiscoveryState;
  strategy: RootDiscoveryStrategy;
  platform: RootDiscoveryPlatform;
  scope: RootDiscoveryScope;
  systemIndexAvailable: boolean;
  fallbackPerformed: boolean;
  volumesCompleted: number;
  volumesTotal: number;
  directoriesChecked: number;
  fileNamesChecked: number;
  candidatesFound: number;
  permissionDenied: number;
  ioErrors: number;
  skipped: number;
  errorCode: string | null;
}

/** 当前进程中的待确认候选。 */
export interface RootCandidateDto {
  id: string;
  client: AgentClientKind;
  absolutePath: string;
  strategy: RootDiscoveryStrategy;
  evidence: "codexRollout" | "claudeTranscript" | "claudeSubagent" | "grokSessionUpdates";
}

/** 单个候选添加到对应数据源后的结果。 */
export interface AddRootCandidateDto {
  client: AgentClientKind;
  rootId: string;
  added: boolean;
  backgroundState: RootActivationState;
}

/** 描述全局首次初始化门禁的持久状态。 */
export type LanguagePreference = "system" | "zh-CN" | "en-US";

/** 描述全局首次初始化门禁的持久状态。 */
export interface InitializationStatusDto {
  /** 完成后才允许挂载业务路由与后台扫描观察器。 */
  initializationCompleted: boolean;
  /** 当前界面语言偏好；system 由前端按当前系统语言解析。 */
  languagePreference: LanguagePreference;
}

/** 描述数据根 registry 变更的脱敏结果。 */
export interface SourceRootMutationDto {
  /** 标识本产品索引中的 registry 确已变更。 */
  changed: boolean;
  /** 明确客户端原始文件未被修改的中文说明。 */
  message: string;
  /** 操作结果的稳定本地化代码。 */
  messageCode?: UiMessageCode;
}

/** 标识扫描范围和是否会主动遍历设备。 */
export type ScanKind = "quick" | "fullDevice";

/** 标识扫描的稳定生命周期。 */
export type ScanState = "idle" | "running" | "completed" | "cancelled" | "failed";

/** 标识一次近 30 日索引是由哪类已批准用户流程触发。 */
export type LocalIndexRefreshTrigger = "initialization" | "discoveryBatch" | "directManual";

/** 扫描阶段的结构化进度参数。 */
export interface ScanScopeProgressDto {
  /** 当前正在索引的数据根稳定 ID；发现阶段为空。 */
  currentRootId: string | null;
  directoriesScanned: number;
  rootsDiscovered: number;
  rootsCompleted: number;
  rootsTotal: number;
}

/** 描述可轮询、可取消的扫描进度。 */
export interface ScanStatusDto {
  /** 当前扫描 ID；空闲时为空。 */
  scanId: string | null;
  /** 当前扫描范围。 */
  kind: ScanKind;
  /** 当前扫描生命周期。 */
  state: ScanState;
  /** 进度基点数，10000 表示 100%。 */
  progressBasisPoints: number;
  /** 不含绝对路径的当前扫描范围标签。 */
  currentScopeLabel: string;
  /** 当前扫描阶段的稳定代码。 */
  currentScopeCode?: ScanScopeCode;
  /** 当前阶段的结构化进度参数。 */
  scopeProgress?: ScanScopeProgressDto;
  /** 当前状态是否允许取消。 */
  canCancel: boolean;
  /** 已访问的候选文件数量。 */
  filesVisited: number;
  /** 已写入索引的 canonical 调用数量。 */
  callsIndexed: number;
  /** 扫描开始时间。 */
  startedAtEpochMs: number | null;
  /** 扫描结束时间。 */
  finishedAtEpochMs: number | null;
  /** 脱敏的中文状态说明。 */
  message: string;
  /** 当前扫描状态的稳定本地化代码。 */
  messageCode?: UiMessageCode;
}

/** 描述数据源页的本机数据根与扫描状态。 */
export interface SourcesDto {
  /** 当前授权的数据根。 */
  roots: SourceRootDto[];
  /** 最近一次扫描覆盖。 */
  coverage: CoverageReportDto;
  /** 当前扫描状态。 */
  scan: ScanStatusDto;
}

/** 描述隐私页需要的最小本地设置。 */
export interface PrivacySettingsDto {
  /** 当前持久语言偏好。 */
  languagePreference: LanguagePreference;
  /** 兼容字段：只保存仅本机偏好，不改变当前只读本机记录的行为。 */
  localOnly: boolean;
  /** 首次从系统会话候选初始化且可由用户修改或清除的标签；不是唯一身份。 */
  deviceUsername: string | null;
  /** 只读 OS 主机名快照；不可由用户修改。 */
  deviceName: string | null;
  /** 首次启动生成并持久化的只读设备唯一 ID；不可由用户修改。 */
  deviceUniqueId: string | null;
  /** 本机周期扫描与保留远端偏好共用的扫描间隔分钟数。 */
  scanIntervalMinutes: number;
  /** 派生用量自动清理天数；缺省 90。 */
  retentionDays: number;
  /** 当前设备 IANA 时区，供 Collect 信封使用，不作为页头选择。 */
  deviceTimeZone: string;
  /** 本产品索引的安全位置说明。 */
  indexLocationLabel: string;
  /** 当前客户端索引位置的稳定代码。 */
  indexLocationCode?: "codex" | "claudeCode" | "grokBuildCli";
  /** 当前索引体积，后端未知时为空。 */
  indexSizeBytes: number | null;
  /** 最近一次清空本产品索引的时间。 */
  lastClearedAtEpochMs: number | null;
  /** 统一 AI 目录中当前能映射到看板的选项；无法映射的类型不进入此数组。 */
  availableAiTypes: AvailableAiTypeDto[];
  /** 用户显式开放监控和上报的本机 Agent；缺省为空。 */
  enabledAgents: AgentClientKind[];
  /** 用户是否已显式开放读取 WorkBuddy 本地用量统计；缺省关闭。 */
  workbuddyStatsEnabled: boolean;
}

/** 单日 WorkBuddy 请求、会话与精确 Token 分布。 */
export interface WorkbuddyDailyBucketDto {
  /** 查看时间标准下的民用日期，格式 `YYYY-MM-DD`。 */
  date: string;
  /** 当日产生过请求的去重会话数量。 */
  sessionCount: number;
  /** 当日上游请求数。 */
  requestCount: number;
  /** 当日顶层会话请求数。 */
  topLevelRequestCount: number;
  /** 当日 subagent 请求数。 */
  subagentRequestCount: number;
  /** 全部输入 Token，包含缓存输入。 */
  inputTokens: number;
  /** 输入 Token 中命中缓存的子集。 */
  cachedInputTokens: number;
  /** 输入 Token 中未命中缓存的部分。 */
  uncachedInputTokens: number;
  /** 输出 Token。 */
  outputTokens: number;
  /** 输入与输出 Token 合计；缓存输入不重复计入。 */
  tokens: number;
  /** 发生缓存读取的 usage 事件所声明请求数。 */
  cachedReadRequestCount: number;
  /** 当日逐请求积分；任一记录缺失时保持未提供。 */
  credits: number | null;
  /** 该日是否仍是当前查看标准下的进行中当天。 */
  inProgress: boolean;
}

/** 单日窗口内一个民用小时的请求、会话与用量。 */
export interface WorkbuddyHourlyBucketDto {
  /** 稳定时间键，格式 `YYYY-MM-DDTHH`。 */
  key: string;
  /** 横轴短标签，格式 `HH:00`。 */
  label: string;
  /** 该小时产生过请求的去重会话数量。 */
  sessionCount: number;
  /** 该小时上游请求数。 */
  requestCount: number;
  /** 该小时顶层会话请求数。 */
  topLevelRequestCount: number;
  /** 该小时 subagent 请求数。 */
  subagentRequestCount: number;
  /** 该小时输入与输出 Token 合计。 */
  tokens: number;
  /** 该小时逐请求积分；任一记录缺失时保持未提供。 */
  credits: number | null;
  /** 该小时是否仍是观测时刻所在的进行中小时。 */
  inProgress: boolean;
}

/** 今日或昨日单日窗口的完整 24 小时趋势。 */
export interface WorkbuddyHourlyTrendDto {
  /** 只会是今日或昨日。 */
  window: UsageWindow;
  /** 该窗口对应的民用日期，格式 `YYYY-MM-DD`。 */
  date: string;
  /** 从 00:00 到 23:00 顺序排列的 24 个小时桶。 */
  buckets: WorkbuddyHourlyBucketDto[];
}

/** 单个日历窗口的 WorkBuddy 请求、会话、Token、积分与 trace 诊断。 */
export interface WorkbuddyWindowDto {
  /** 六个固定日历窗口之一。 */
  window: UsageWindow;
  /** 窗口内产生过请求的去重会话数量。 */
  sessionCount: number;
  /** 窗口内上游请求数。 */
  requestCount: number;
  /** 窗口内顶层会话请求数。 */
  topLevelRequestCount: number;
  /** 窗口内 subagent 请求数。 */
  subagentRequestCount: number;
  /** 全部输入 Token，包含缓存输入。 */
  inputTokens: number;
  /** 输入 Token 中命中缓存的子集。 */
  cachedInputTokens: number;
  /** 输入 Token 中未命中缓存的部分。 */
  uncachedInputTokens: number;
  /** 输出 Token。 */
  outputTokens: number;
  /** 输入与输出 Token 合计；缓存输入不重复计入。 */
  tokens: number;
  /** 发生缓存读取的 usage 事件所声明请求数。 */
  cachedReadRequestCount: number;
  /** 窗口内涉及的去重 transcript 来源数。 */
  sourceCount: number;
  /** 窗口内逐请求积分；任一记录缺失时保持未提供。 */
  credits: number | null;
  /** 窗口内有用量的民用日。 */
  dates: string[];
  /** 窗口内活跃会话按首末请求计算的平均时长，单位秒。 */
  averageSessionDurationSeconds: number;
  /** 窗口内开始的 trace 数量。 */
  traceTotalCount: number;
  /** 窗口内状态为错误的 trace 数量。 */
  traceErrorCount: number;
  /** 窗口内状态为已取消的 trace 数量。 */
  traceCancelledCount: number;
  /** 窗口内 trace 平均耗时，单位毫秒。 */
  traceAverageDurationMs: number;
}

/** 单个 WorkBuddy 实际执行模型的 project JSONL 请求汇总行。 */
export interface WorkbuddyModelUsageGroupDto {
  /** 安全的实际执行模型；无法归属时为 `null`。 */
  model: string | null;
  /** 汇入本行的上游请求数。 */
  callCount: number;
  /** 输入与输出 Token 合计；缓存输入不重复计入。 */
  totalTokens: number;
  /** 全部输入 Token，包含缓存输入。 */
  inputTokens: number;
  /** 输入 Token 中命中缓存的子集。 */
  cachedInputTokens: number;
  /** 输入 Token 中未命中缓存的子集。 */
  uncachedInputTokens: number;
  /** 输出 Token。 */
  outputTokens: number;
  /** 逐请求积分；任一记录缺失时保持未提供。 */
  credits: number | null;
  /** 顶层会话请求数。 */
  topLevelCallCount: number;
  /** subagent 请求数。 */
  subagentCallCount: number;
}

/** 同一日历窗口内，按 project JSONL 事件实际模型聚合的精确统计。 */
export interface WorkbuddyModelUsageWindowDto {
  /** 六个固定日历窗口之一。 */
  window: UsageWindow;
  /** 逐安全模型聚合的请求明细。 */
  groups: WorkbuddyModelUsageGroupDto[];
}

/** WorkBuddy 用量页一次 IPC 返回的请求统计与同窗口模型明细。 */
export interface WorkbuddyUsageDetailsDto {
  /** 由同一批 project JSONL 事件构造的通用统计页。 */
  statistics: UsageStatisticsDto;
  /** 与统计页使用同一观测时刻和日历窗口的实际模型明细。 */
  modelUsage: WorkbuddyModelUsageWindowDto;
}

/** WorkBuddy 本地用量统计快照，供设置页开关开启后展示。 */
export interface WorkbuddyStatisticsDto {
  /** 全历史产生过用量的去重会话总数。 */
  totalSessions: number;
  /** 全历史上游请求总数。 */
  totalRequests: number;
  /** 全历史顶层会话请求总数。 */
  topLevelRequests: number;
  /** 全历史 subagent 请求总数。 */
  subagentRequests: number;
  /** 全部输入 Token，包含缓存输入。 */
  totalInputTokens: number;
  /** 输入 Token 中命中缓存的子集。 */
  totalCachedInputTokens: number;
  /** 输入 Token 中未命中缓存的部分。 */
  totalUncachedInputTokens: number;
  /** 全部输出 Token。 */
  totalOutputTokens: number;
  /** 输入与输出 Token 合计；缓存输入不重复计入。 */
  totalTokens: number;
  /** 全部逐请求积分；任一记录缺失时保持未提供。 */
  totalCredits: number | null;
  /** 去重会话按首末请求计算的平均时长，单位秒。 */
  averageSessionDurationSeconds: number;
  /** 按查看时间标准民用日升序排列的每日分布。 */
  dailyBuckets: WorkbuddyDailyBucketDto[];
  /** 六个日历窗口的请求、会话、Token 与积分合计。 */
  windows: WorkbuddyWindowDto[];
  /** 今日与昨日两个单日窗口的固定 24 小时趋势。 */
  hourlyTrends: WorkbuddyHourlyTrendDto[];
  /** trace 记录总数。 */
  traceTotalCount: number;
  /** 状态为错误的 trace 数量。 */
  traceErrorCount: number;
  /** 状态为已取消的 trace 数量。 */
  traceCancelledCount: number;
  /** trace 平均耗时，单位毫秒。 */
  traceAverageDurationMs: number;
  /** 本次 project JSONL 读取与校验的覆盖结论。 */
  coverage: CoverageReportDto;
  /** 本次计算发生时刻的 Unix 毫秒时间戳。 */
  generatedAtEpochMs: number;
}

/** 数据源页展示的 WorkBuddy 只读发现状态；不携带绝对路径。 */
export interface WorkbuddySourceStatusDto {
  /** 用户是否已在设置中显式开放读取。 */
  enabled: boolean;
  /** 本机是否发现 `~/.workbuddy` 普通目录；Ready 状态另要求 projects 可读。 */
  installed: boolean;
  /** 发现命中时的安全展示别名，未发现为 `null`。 */
  alias: string | null;
  /** 与物理 Agent 数据源表同形的已发现默认根；未发现为空。 */
  roots: SourceRootDto[];
}

/** 描述单个独立数据上报 Provider 的稳定身份、目标、周期与三态结果。 */
export interface CollectProviderConfigDto {
  /** 本地稳定 UUID；不进入远端 payload。 */
  id: string;
  /** 规范化且在集合中唯一的 BaseURL。 */
  baseUrl: string;
  /** 该 Provider 的周期发送间隔分钟数。 */
  intervalMinutes: number;
  /** 可选上报用户别名；存在时替代设备用户名进入该 Provider 的上报载荷，未设置为 `null`。 */
  userAlias: string | null;
  /** 当前进程内最近一次 Health 探测的三态结果；上传成败不改写。 */
  connectionStatus: "untested" | "reachable" | "unreachable";
}

/** 描述一条不含身份和完整 payload 的本地尝试审计。 */
export interface CollectAttemptDto {
  /** 仅用于本地审计列表排序，不进入 REST 上报。 */
  attemptId: number;
  trigger: "startup" | "interval" | "config_changed" | "manual";
  state: "collecting" | "collection_failed" | "sending" | "sent" | "send_failed";
  destinationBaseUrl: string;
  startedAtEpochMs: number;
  finishedAtEpochMs: number | null;
  dayCount: number | null;
  channelCount: number | null;
  unavailableChannelCount: number | null;
  totalTokens: number | null;
  httpStatus: number | null;
  errorKind:
    | "configuration"
    | "identity_unavailable"
    | "no_available_channels"
    | "invalid_report"
    | "transport"
    | "timeout"
    | "http_status"
    | "service_unhealthy"
    | "invalid_response"
    | "interrupted"
    | null;
}

/** 描述全部 Provider 配置和最近二十条全局安全审计。 */
export interface CollectProviderStatusDto {
  providers: CollectProviderConfigDto[];
  lastSuccessAtEpochMs: number | null;
  recentAttempts: CollectAttemptDto[];
}

/** 描述用户显式执行的一次服务健康检测。 */
export interface CollectServiceHealthDto {
  checkedAtEpochMs: number;
  healthy: boolean;
  httpStatus: number | null;
  errorKind:
    | "transport"
    | "timeout"
    | "http_status"
    | "service_unhealthy"
    | "invalid_response"
    | null;
}

/** 描述清空本产品索引后的可见结果。 */
export interface ClearIndexResultDto {
  /** 是否成功清空本产品索引。 */
  cleared: boolean;
  /** 明确不会删除当前客户端原始记录的中文说明。 */
  message: string;
  /** 清空结果的稳定本地化代码。 */
  messageCode?: UiMessageCode;
}
