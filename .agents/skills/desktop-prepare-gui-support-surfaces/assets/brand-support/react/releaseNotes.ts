/** 发布日志资源支持的稳定语言集合。 */
export type ReleaseNotesLocale = "zh-CN" | "en-US";

/** 与 {@link ReleaseNotesLocale} 保持同步的运行时语言列表，供校验和回退逻辑复用。 */
export const RELEASE_NOTES_LOCALES: readonly ReleaseNotesLocale[] = ["zh-CN", "en-US"];

/** 把一个发布事实的中英文文案绑定为不可拆分的翻译对。 */
export interface LocalizedReleaseNoteItem {
  "zh-CN": string;
  "en-US": string;
}

/** 候选资源中保存的单个双语发布版本更新日志。 */
export interface LocalizedReleaseNoteEntry {
  releaseDate: string;
  version: string;
  featureOptimizations: readonly LocalizedReleaseNoteItem[];
  bugFixes: readonly LocalizedReleaseNoteItem[];
}

/** 设置页按当前语言选择后可展示的单个发布版本更新日志。 */
export interface ReleaseNoteEntry {
  releaseDate: string;
  version: string;
  featureOptimizations: readonly string[];
  bugFixes: readonly string[];
}

/** 设置页最多保留并展示最近五个正式发布版本。 */
export const MAX_VISIBLE_RELEASE_NOTE_VERSIONS = 5;

/** 每个版本的功能优化与问题修复各自最多展示十条。 */
export const MAX_VISIBLE_RELEASE_NOTE_ITEMS = 10;

/**
 * 把系统或 i18next 语言归一化为发布日志实际支持的 locale。
 * 当前只提供 zh-CN/en-US 两套翻译：zh-TW/zh-HK/zh-Hant 等其他中文变体按设计并入
 * zh-CN，而非回退英文；这是既定简化，不代表未处理的地区差异。
 */
export function resolveReleaseNotesLocale(
  language: string | undefined,
): ReleaseNotesLocale {
  return language?.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}

/** 按当前 locale 选择翻译并防御性裁剪固定展示上限。 */
export function selectVisibleReleaseNotes(
  releases: readonly LocalizedReleaseNoteEntry[],
  locale: ReleaseNotesLocale,
): ReleaseNoteEntry[] {
  return releases.slice(0, MAX_VISIBLE_RELEASE_NOTE_VERSIONS).map((release) => ({
    releaseDate: release.releaseDate,
    version: release.version,
    featureOptimizations: release.featureOptimizations
      .slice(0, MAX_VISIBLE_RELEASE_NOTE_ITEMS)
      .map((item) => item[locale]),
    bugFixes: release.bugFixes
      .slice(0, MAX_VISIBLE_RELEASE_NOTE_ITEMS)
      .map((item) => item[locale]),
  }));
}
