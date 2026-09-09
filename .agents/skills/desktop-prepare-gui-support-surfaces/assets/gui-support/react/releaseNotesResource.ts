import { invoke } from "@tauri-apps/api/core";

import {
  MAX_VISIBLE_RELEASE_NOTE_ITEMS,
  MAX_VISIBLE_RELEASE_NOTE_VERSIONS,
  RELEASE_NOTES_LOCALES,
  type LocalizedReleaseNoteEntry,
  type LocalizedReleaseNoteItem,
} from "./releaseNotes";

export const LOAD_RELEASE_NOTES_COMMAND = "load_release_notes";

/** 表示 Tauri 命令返回的固定更新日志文档。 */
export interface ReleaseNotesDocument {
  schemaVersion: 2;
  releases: LocalizedReleaseNoteEntry[];
}

/** 允许测试替换窄命令调用，而不引入通用文件系统权限。 */
export type ReleaseNotesInvoker = (command: string) => Promise<unknown>;

const MAX_CARGO_SEMVER_MAJOR = "18446744073709551615";

const invokeReleaseNotesCommand: ReleaseNotesInvoker = (command) =>
  invoke<unknown>(command);

/** 判断未知 IPC 值是否为普通记录对象。 */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** 要求对象字段集合与固定 schema 完全一致。 */
function hasExactKeys(
  value: Record<string, unknown>,
  expected: readonly string[],
): boolean {
  const actual = Object.keys(value).sort();
  const sortedExpected = [...expected].sort();
  return (
    actual.length === expected.length &&
    actual.every((key, index) => key === sortedExpected[index])
  );
}

/** 校验规范日期，避免 WebView 对非法日期作宽松归一化。 */
function isCanonicalReleaseDate(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(value);
  if (!match) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year === 0 || month < 1 || month > 12) return false;
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const days = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return day >= 1 && day <= (days[month - 1] ?? 0);
}

/** 按十进制字符串比较 major，避免 JavaScript Number 丢失 Cargo u64 边界精度。 */
function isCargoSemverMajor(value: string): boolean {
  const normalized = value.replace(/^0+(?=\d)/u, "");
  return (
    normalized.length < MAX_CARGO_SEMVER_MAJOR.length ||
    (normalized.length === MAX_CARGO_SEMVER_MAJOR.length &&
      normalized <= MAX_CARGO_SEMVER_MAJOR)
  );
}

/** 要求版本只带一个小写 v，并满足 Cargo u64 major、历史低位边界或时间版本。 */
function isDisplayVersion(value: string): boolean {
  const harnessVersion = /^v\d{12}$/u;
  if (harnessVersion.test(value)) return true;
  const semantic = /^v(\d+)\.(\d+)\.(\d+)$/u.exec(value);
  return (
    semantic !== null &&
    isCargoSemverMajor(semantic[1] ?? "") &&
    semantic.slice(2).every((part) => Number(part) <= 100)
  );
}

/** 判断某个语言字段是否为非空且无首尾空白的文本。 */
function isCleanLocalizedText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.trim() === value;
}

/** 收窄单个分类的完整双语、逐语言去重且不超过十条的翻译对。 */
function decodeItems(value: unknown, field: string): LocalizedReleaseNoteItem[] {
  if (!Array.isArray(value) || value.length > MAX_VISIBLE_RELEASE_NOTE_ITEMS) {
    throw new Error(`invalid release notes ${field}`);
  }
  const items = value.map((item) => {
    if (
      !isRecord(item) ||
      !hasExactKeys(item, RELEASE_NOTES_LOCALES) ||
      !RELEASE_NOTES_LOCALES.every((locale) => isCleanLocalizedText(item[locale]))
    ) {
      throw new Error(`invalid release notes ${field} item`);
    }
    return { "en-US": item["en-US"] as string, "zh-CN": item["zh-CN"] as string };
  });
  if (
    RELEASE_NOTES_LOCALES.some(
      (locale) => new Set(items.map((entry) => entry[locale])).size !== items.length,
    )
  ) {
    throw new Error(`duplicate release notes ${field} item`);
  }
  return items;
}

/** 在 WebView IPC 边界再次验证 Rust 返回的更新日志结构。 */
export function decodeReleaseNotesDocument(value: unknown): ReleaseNotesDocument {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["releases", "schemaVersion"]) ||
    value.schemaVersion !== 2 ||
    !Array.isArray(value.releases) ||
    value.releases.length === 0 ||
    value.releases.length > MAX_VISIBLE_RELEASE_NOTE_VERSIONS
  ) {
    throw new Error("invalid release notes document");
  }
  const versions = new Set<string>();
  let previousDate: string | undefined;
  const releaseValues: unknown[] = value.releases;
  const releases = releaseValues.map((entry): LocalizedReleaseNoteEntry => {
    if (
      !isRecord(entry) ||
      !hasExactKeys(entry, [
        "bugFixes",
        "featureOptimizations",
        "releaseDate",
        "version",
      ]) ||
      typeof entry.releaseDate !== "string" ||
      !isCanonicalReleaseDate(entry.releaseDate) ||
      typeof entry.version !== "string" ||
      !isDisplayVersion(entry.version) ||
      versions.has(entry.version) ||
      (previousDate !== undefined && previousDate < entry.releaseDate)
    ) {
      throw new Error("invalid release notes entry");
    }
    const featureOptimizations = decodeItems(
      entry.featureOptimizations,
      "featureOptimizations",
    );
    const bugFixes = decodeItems(entry.bugFixes, "bugFixes");
    if (featureOptimizations.length === 0 && bugFixes.length === 0) {
      throw new Error("empty release notes entry");
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

/** 通过唯一窄 Tauri 命令读取候选内同一份更新日志资源。 */
export async function loadBundledReleaseNotes(
  invokeCommand: ReleaseNotesInvoker = invokeReleaseNotesCommand,
): Promise<LocalizedReleaseNoteEntry[]> {
  const value = await invokeCommand(LOAD_RELEASE_NOTES_COMMAND);
  return decodeReleaseNotesDocument(value).releases;
}
