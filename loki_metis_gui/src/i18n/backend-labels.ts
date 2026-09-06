// 本文件是 usage-types-detail.ts 里提到的“稳定代码 -> 双语文案”查表层
// 的具体实现：后端只返回语言无关的枚举代码（UiMessageCode 等），
// 这里统一用 `t(\`backend.xxx.${code}\`)` 去 i18n 资源（zh-CN.ts/en-US.ts）
// 里查出当前语言对应的文案，页面组件不需要各自重复这套映射逻辑。
import type { TFunction } from "i18next";

import type {
  DisplayLabelCode,
  ScanStatusDto,
  SourceDiscoveryCode,
  UiMessageCode,
  UsageChartDistributionMetric,
} from "../api/usage";

/** 图表指标（趋势线/分布图）复用既有 `metric.*` 文案对应的 key。 */
const chartMetricKeys: Record<UsageChartDistributionMetric, string> = {
  cacheWriteInputTokens: "cacheWrite",
  cachedInputTokens: "cachedInput",
  callCount: "calls",
  inputTokens: "input",
  outputTokens: "output",
  reasoningOutputTokens: "reasoningOutput",
  totalTokens: "totalTokens",
};

/** 按图表指标渲染既有 `metric.*` 文案，避免维护第二套重复的双语标签。 */
export function chartMetricLabel(
  t: TFunction,
  metric: UsageChartDistributionMetric,
): string {
  return t(`metric.${chartMetricKeys[metric]}`);
}

/** 按稳定代码渲染 backend 固定消息；缺失代码时使用安全通用文案。 */
export function uiMessageLabel(t: TFunction, code?: UiMessageCode): string {
  return code ? t(`backend.message.${code}`) : t("common.unknownError");
}

/** 按稳定代码渲染占位或匿名短标签；字面技术值和用户标签保持原样。 */
// disambiguationIndex：当同一个安全短标签（比如两个数据根被脱敏后
// 显示成相同的匿名后缀）在列表里出现重复、单看文字无法区分时，
// 后端会附带一个消歧序号，这里再包一层"(2)"这样的后缀渲染出来，
// 让用户至少能区分"这是列表里第几个同名项"，而不需要暴露真实路径。
export function displayLabel(
  t: TFunction,
  label: string,
  code: DisplayLabelCode | undefined,
  disambiguationIndex?: number,
): string {
  const translated =
    !code || code === "literal"
      ? label
      : code === "project" || code === "thread"
        ? t(`backend.display.${code}`, { value: label })
        : t(`backend.display.${code}`);
  return disambiguationIndex === undefined
    ? translated
    : t("backend.display.disambiguated", { index: disambiguationIndex, label: translated });
}

/** 按稳定来源代码渲染数据根发现方式。 */
export function sourceDiscoveryLabel(
  t: TFunction,
  code: SourceDiscoveryCode | undefined,
): string {
  return code ? t(`backend.discovery.${code}`) : t("common.unknownError");
}

/** 使用结构化计数渲染扫描阶段，不解析 backend 的中文回退句子。 */
export function scanScopeLabel(t: TFunction, scan: ScanStatusDto): string {
  const progress = scan.scopeProgress ?? {
    directoriesScanned: 0,
    rootsCompleted: 0,
    rootsDiscovered: 0,
    rootsTotal: 0,
  };
  switch (scan.currentScopeCode) {
    case "registeredRoots":
    case "localFixedVolumes":
      return t(`backend.scanScope.${scan.currentScopeCode}`);
    case "discoveringVolumes":
    case "discoveryFinished":
      return t(`backend.scanScope.${scan.currentScopeCode}`, {
        directories: progress.directoriesScanned,
        roots: progress.rootsDiscovered,
      });
    case "indexingRoots":
      return t("backend.scanScope.indexingRoots", {
        completed: progress.rootsCompleted,
        total: progress.rootsTotal,
      });
    default:
      return t("common.unknownError");
  }
}
