import { invoke } from "@tauri-apps/api/core";

import { isRecord, type InterfaceLanguage } from "./api";

/** 发布说明支持的完整双语条目。 */
export interface LocalizedReleaseNoteItem {
  "zh-CN": string;
  "en-US": string;
}

/** 表示候选资源中的一个正式版本。 */
export interface LocalizedReleaseNoteEntry {
  releaseDate: string;
  version: string;
  featureOptimizations: LocalizedReleaseNoteItem[];
  bugFixes: LocalizedReleaseNoteItem[];
}

/** 描述由 Rust 边界验证后返回的版本 2 文档。 */
export interface ReleaseNotesDocument {
  schemaVersion: 2;
  releases: LocalizedReleaseNoteEntry[];
}

/** 表示设置页已经选择当前语言后的单个版本。 */
export interface VisibleReleaseNoteEntry {
  releaseDate: string;
  version: string;
  featureOptimizations: string[];
  bugFixes: string[];
}

/** 设置页最多展示最近五个正式版本。 */
export const MAX_VISIBLE_RELEASES = 5;

/** 每类变化最多展示十条。 */
export const MAX_VISIBLE_ITEMS = 10;

/** 固定更新日志 IPC 命令名，避免组件散落拼写。 */
export const LOAD_RELEASE_NOTES_COMMAND = "load_release_notes";

/** 允许测试替换更新日志的唯一窄命令。 */
export type ReleaseNotesInvoker = (command: string) => Promise<unknown>;

const invokeReleaseNotesCommand: ReleaseNotesInvoker = (command) =>
  invoke<unknown>(command);

/** 判断对象字段集合是否与固定 schema 完全相同。 */
function hasExactKeys(
  value: Record<string, unknown>,
  expected: readonly string[],
): boolean {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  return (
    actual.length === wanted.length && actual.every((key, index) => key === wanted[index])
  );
}

/** 验证严格 YYYY-MM-DD 日期，拒绝浏览器的宽松归一化。 */
function isCanonicalDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(value);
  if (match === null) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year === 0 || month < 1 || month > 12) return false;
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return day >= 1 && day <= (days[month - 1] ?? 0);
}

/** 验证只带一个小写 v 的受限语义版本。 */
function isReleaseVersion(value: string): boolean {
  const match = /^v(\d+)\.(\d+)\.(\d+)$/u.exec(value);
  return match !== null && match.slice(1).every((part) => Number(part) <= 100);
}

/** 验证非空、无首尾空白的本地化文本。 */
function isCleanText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.trim() === value;
}

/** 收窄一个变化分类的完整双语条目并拒绝重复内容。 */
function decodeItems(value: unknown): LocalizedReleaseNoteItem[] {
  if (!Array.isArray(value) || value.length > MAX_VISIBLE_ITEMS) {
    throw new Error("invalid release note items");
  }

  const items = value.map((item): LocalizedReleaseNoteItem => {
    if (
      !isRecord(item) ||
      !hasExactKeys(item, ["en-US", "zh-CN"]) ||
      !isCleanText(item["en-US"]) ||
      !isCleanText(item["zh-CN"])
    ) {
      throw new Error("invalid localized release note");
    }
    return { "en-US": item["en-US"], "zh-CN": item["zh-CN"] };
  });

  for (const locale of ["zh-CN", "en-US"] as const) {
    if (new Set(items.map((item) => item[locale])).size !== items.length) {
      throw new Error("duplicate localized release note");
    }
  }
  return items;
}

/** 在 WebView IPC 边界重新验证发布说明结构与排序。 */
export function decodeReleaseNotesDocument(value: unknown): ReleaseNotesDocument {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["releases", "schemaVersion"]) ||
    value.schemaVersion !== 2 ||
    !Array.isArray(value.releases) ||
    value.releases.length === 0 ||
    value.releases.length > MAX_VISIBLE_RELEASES
  ) {
    throw new Error("invalid release notes document");
  }

  const versions = new Set<string>();
  let previousDate: string | undefined;
  const releases = value.releases.map((entry): LocalizedReleaseNoteEntry => {
    if (
      !isRecord(entry) ||
      !hasExactKeys(entry, [
        "bugFixes",
        "featureOptimizations",
        "releaseDate",
        "version",
      ]) ||
      typeof entry.releaseDate !== "string" ||
      !isCanonicalDate(entry.releaseDate) ||
      typeof entry.version !== "string" ||
      !isReleaseVersion(entry.version) ||
      versions.has(entry.version) ||
      (previousDate !== undefined && previousDate < entry.releaseDate)
    ) {
      throw new Error("invalid release note entry");
    }

    const featureOptimizations = decodeItems(entry.featureOptimizations);
    const bugFixes = decodeItems(entry.bugFixes);
    if (featureOptimizations.length === 0 && bugFixes.length === 0) {
      throw new Error("empty release note entry");
    }
    versions.add(entry.version);
    previousDate = entry.releaseDate;
    return {
      bugFixes,
      featureOptimizations,
      releaseDate: entry.releaseDate,
      version: entry.version,
    };
  });

  return { releases, schemaVersion: 2 };
}

/** 通过唯一窄命令读取候选内发布说明。 */
export async function loadBundledReleaseNotes(
  invokeCommand: ReleaseNotesInvoker = invokeReleaseNotesCommand,
): Promise<ReleaseNotesDocument> {
  const value = await invokeCommand(LOAD_RELEASE_NOTES_COMMAND);
  return decodeReleaseNotesDocument(value);
}

/** 把 i18next 当前语言归一化为发布说明资源语言。 */
export function resolveReleaseNotesLocale(language: string | undefined): InterfaceLanguage {
  return language?.toLowerCase().startsWith("zh") === true ? "zh-CN" : "en-US";
}

/** 按当前语言选择文案，并再次施加防御性展示上限。 */
export function selectVisibleReleaseNotes(
  document: ReleaseNotesDocument,
  language: InterfaceLanguage,
): VisibleReleaseNoteEntry[] {
  return document.releases.slice(0, MAX_VISIBLE_RELEASES).map((release) => ({
    releaseDate: release.releaseDate,
    version: release.version,
    featureOptimizations: release.featureOptimizations
      .slice(0, MAX_VISIBLE_ITEMS)
      .map((item) => item[language]),
    bugFixes: release.bugFixes.slice(0, MAX_VISIBLE_ITEMS).map((item) => item[language]),
  }));
}

/** 把版本统一格式化为一个小写 v 前缀。 */
export function formatDisplayVersion(version: string): string {
  return `v${version.replace(/^[vV]/u, "")}`;
}
