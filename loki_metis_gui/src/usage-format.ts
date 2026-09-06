import type { Completeness, Confidence, Freshness, ProviderKind } from './api/usage';
import { appI18n } from './i18n';

/** 看板数字与日期格式化使用的界面语言。 */
type SupportedLocale = 'zh-CN' | 'en-US';

// `Intl.NumberFormat`/`Intl.DateTimeFormat` 都是浏览器内置的国际化格式化
// 器，构造它们本身有一定开销（需要加载对应 locale 的格式规则）；
// 下面每种格式各自按 locale 缓存一份构造好的实例（cachedFormatter），
// 避免同一个 locale 的格式化器被反复创建——这些数字/日期展示函数在
// 列表渲染场景下可能被调用几十上百次。
const numberFormatters = new Map<string, Intl.NumberFormat>();
const compactFormatters = new Map<string, Intl.NumberFormat>();
const decimalFormatters = new Map<string, Intl.NumberFormat>();
const creditFormatters = new Map<string, Intl.NumberFormat>();
const percentFormatters = new Map<string, Intl.NumberFormat>();
const dateTimeFormatters = new Map<string, Intl.DateTimeFormat>();
const calendarDateFormatters = new Map<string, Intl.DateTimeFormat>();

/** 解析格式化 locale，缺失时使用当前 i18n 语言。 */
function currentLocale(locale?: SupportedLocale): SupportedLocale {
  if (locale) return locale;
  return appI18n.resolvedLanguage === 'zh-CN' ? 'zh-CN' : 'en-US';
}

/** 按 locale 复用不可变格式化器，避免重复构造。 */
function cachedFormatter<T>(cache: Map<string, T>, locale: SupportedLocale, create: () => T): T {
  const existing = cache.get(locale);
  if (existing) return existing;
  const formatter = create();
  cache.set(locale, formatter);
  return formatter;
}

/** 在指定 locale 下读取格式化辅助文案。 */
function translated(key: string, locale: SupportedLocale): string {
  return appI18n.t(key, { lng: locale });
}

/** 把大整数 Token 按当前 locale 格式化为稳定数字展示。 */
export function formatTokens(value: number | null, targetLocale?: SupportedLocale): string {
  const locale = currentLocale(targetLocale);
  if (value === null) return translated('common.notProvided', locale);
  return cachedFormatter(numberFormatters, locale, () => new Intl.NumberFormat(locale)).format(
    value,
  );
}

/** 把 Token 按十进制 K/M/B 缩写为固定两位小数；不足 1,000 仍走精确整数。 */
export function formatCompactTokens(value: number, targetLocale?: SupportedLocale): string {
  const locale = currentLocale(targetLocale);
  const units = [
    { suffix: 'B', threshold: 1_000_000_000 },
    { suffix: 'M', threshold: 1_000_000 },
    { suffix: 'K', threshold: 1_000 },
  ] as const;
  const absoluteValue = Math.abs(value);
  const unitIndex = units.findIndex((unit) => absoluteValue >= unit.threshold);
  if (unitIndex === -1) {
    return formatTokens(value, locale);
  }

  let selectedIndex = unitIndex;
  let selectedUnit = units[selectedIndex];
  if (!selectedUnit) {
    return formatTokens(value, locale);
  }
  // 先乘后除，避免 (value/threshold)*100 的 IEEE-754 半入把 1005 收成 1.00K。
  let rounded = Math.round((value * 100) / selectedUnit.threshold) / 100;
  // 四舍五入可能把 999.995K 推到 1000.00K；升一级单位重新取整直到落回 [1, 1000) 或到达最大单位。
  while (Math.abs(rounded) >= 1_000 && selectedIndex > 0) {
    selectedIndex -= 1;
    const nextUnit = units[selectedIndex];
    if (!nextUnit) {
      break;
    }
    selectedUnit = nextUnit;
    rounded = Math.round((value * 100) / selectedUnit.threshold) / 100;
  }
  const formatter = cachedFormatter(
    compactFormatters,
    locale,
    () =>
      new Intl.NumberFormat(locale, {
        maximumFractionDigits: 2,
        minimumFractionDigits: 2,
      }),
  );
  return `${formatter.format(rounded)}${selectedUnit.suffix}`;
}

/** 把 Unix 毫秒时间按当前 locale 转换为本机时间，空值明确显示未知。 */
export function formatObservedAt(value: number | null, targetLocale?: SupportedLocale): string {
  const locale = currentLocale(targetLocale);
  if (value === null) {
    return translated('common.noRecordsYet', locale);
  }
  return cachedFormatter(
    dateTimeFormatters,
    locale,
    () => new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }),
  ).format(value);
}

/** 格式化无时刻的 YYYY-MM-DD 日历日期，并固定 UTC 避免跨时区偏移一天。 */
// 用 `Date.UTC(...)` 构造再检查三个字段（年/月/日）是否原样往返
// 保持不变：JS 的 Date 对非法日历值（比如 2 月 30 日）会自动“溢出进位”
// 成一个看似合法但实际不是用户输入的日期，这里通过往返比较拦截这种
// 情况，返回“未知”而不是展示一个被悄悄改写过的日期；用 UTC 而不是
// 本地时区构造，是为了避免日期字符串本身就没有时刻信息时，被本地
// 时区解释成前一天或后一天。
export function formatCalendarDate(value: string, targetLocale?: SupportedLocale): string {
  const locale = currentLocale(targetLocale);
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return translated('common.unknown', locale);
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(Date.UTC(year, month - 1, day));
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return translated('common.unknown', locale);
  }
  return cachedFormatter(
    calendarDateFormatters,
    locale,
    () => new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeZone: 'UTC' }),
  ).format(date);
}

/** 把基点比例格式化为百分比，空值保持“不适用”。 */
export function formatBasisPoints(value: number | null, targetLocale?: SupportedLocale): string {
  const locale = currentLocale(targetLocale);
  if (value === null) return translated('common.notApplicable', locale);
  return cachedFormatter(
    percentFormatters,
    locale,
    () => new Intl.NumberFormat(locale, { maximumFractionDigits: 1, style: 'percent' }),
  ).format(value / 10_000);
}

/** 把 WorkBuddy 积分消耗按当前 locale 格式化为固定两位小数；空值保持“未提供”。 */
export function formatCredits(value: number | null, targetLocale?: SupportedLocale): string {
  const locale = currentLocale(targetLocale);
  if (value === null) return translated('common.notProvided', locale);
  return cachedFormatter(
    creditFormatters,
    locale,
    () => new Intl.NumberFormat(locale, { maximumFractionDigits: 2, minimumFractionDigits: 2 }),
  ).format(value);
}

/** 把索引字节数转换为适合设置页的紧凑体积。 */
export function formatBytes(value: number | null, targetLocale?: SupportedLocale): string {
  const locale = currentLocale(targetLocale);
  if (value === null) {
    return translated('common.unknown', locale);
  }
  const decimal = cachedFormatter(
    decimalFormatters,
    locale,
    () => new Intl.NumberFormat(locale, { maximumFractionDigits: 1, minimumFractionDigits: 1 }),
  );
  if (value < 1024) {
    return `${formatTokens(value, locale)} B`;
  }
  if (value < 1024 * 1024) {
    return `${decimal.format(value / 1024)} KB`;
  }
  return `${decimal.format(value / 1024 / 1024)} MB`;
}

/** 把稳定 provider 类型映射为不会误导口径的本地化来源名。 */
export function providerLabel(provider: ProviderKind, targetLocale?: SupportedLocale): string {
  return translated(`format.provider.${provider}`, currentLocale(targetLocale));
}

/** 把事实新鲜度映射为本地化状态。 */
export function freshnessLabel(freshness: Freshness, targetLocale?: SupportedLocale): string {
  return translated(`format.freshness.${freshness}`, currentLocale(targetLocale));
}

/** 把覆盖完整度映射为本地化状态。 */
export function completenessLabel(
  completeness: Completeness,
  targetLocale?: SupportedLocale,
): string {
  return translated(`format.completeness.${completeness}`, currentLocale(targetLocale));
}

/** 把事实置信度映射为本地化质量提示。 */
export function confidenceLabel(confidence: Confidence, targetLocale?: SupportedLocale): string {
  return translated(`format.confidence.${confidence}`, currentLocale(targetLocale));
}
