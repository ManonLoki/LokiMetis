#!/usr/bin/env node
/** 维护可打包并供已选关于页展示的最近十个实际发布版本更新日志。 */

import {
  closeSync,
  existsSync,
  fsyncSync,
  lstatSync,
  openSync,
  readFileSync,
  realpathSync,
  renameSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { randomBytes } from "node:crypto";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

export const SCHEMA_VERSION = 2;
export const MAX_RELEASES = 10;
export const MAX_ITEMS_PER_SECTION = 10;
export const MAX_RELEASE_NOTES_BYTES = 1024 * 1024;
const MAX_SEMVER_MAJOR = "18446744073709551615";
export const SUPPORTED_LOCALES = ["zh-CN", "en-US"];
const ROOT_KEYS = ["schemaVersion", "releases"];
const ENTRY_KEYS = ["releaseDate", "version", "featureOptimizations", "bugFixes"];
const ITEM_KEYS = [...SUPPORTED_LOCALES];
const RENDER_COPY = {
  "zh-CN": {
    title: "更新日志",
    featureOptimizations: "功能优化",
    bugFixes: "问题修复",
    none: "无",
  },
  "en-US": {
    title: "Release notes",
    featureOptimizations: "Feature optimizations",
    bugFixes: "Bug fixes",
    none: "None",
  },
};
const SEMVER = /^([0-9]+)\.([0-9]+)\.([0-9]+)$/;
const HARNESS_VERSION = /^[0-9]{12}$/;
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true });

/** 表示更新日志文件或命令输入违反稳定契约。 */
export class ReleaseNotesError extends Error {
  /** 保存可公开、稳定的更新日志错误。 */
  constructor(message) {
    super(message);
    this.name = "ReleaseNotesError";
  }
}

/** 以十进制文本比较非负整数，避免数值精度与位数限制。 */
function decimalAtMost(value, maximum) {
  const significant = value.replace(/^0+/, "") || "0";
  return significant.length < maximum.length ||
    (significant.length === maximum.length && significant <= maximum);
}

/** 把机器版本规范化为仅带一个小写 v 的展示版本。 */
export function normalizeDisplayVersion(value) {
  if (typeof value !== "string") throw new ReleaseNotesError("version must be a string");
  let normalized = value.trim();
  while (normalized.slice(0, 1).toLowerCase() === "v") normalized = normalized.slice(1);
  const semantic = SEMVER.exec(normalized);
  if (semantic) {
    const [, major, minor, patch] = semantic;
    if (!decimalAtMost(major, MAX_SEMVER_MAJOR)) {
      throw new ReleaseNotesError(`semantic version major must be within 0..${MAX_SEMVER_MAJOR}`);
    }
    if (!decimalAtMost(minor, "99") || !decimalAtMost(patch, "99")) {
      throw new ReleaseNotesError("semantic version minor and patch components must be within 0..99");
    }
  } else if (!HARNESS_VERSION.test(normalized)) {
    throw new ReleaseNotesError("version must be x.y.z or a 12-digit Harness time version");
  }
  return `v${normalized}`;
}

/** 要求发布日期使用规范 ISO 日历日期。 */
function validateReleaseDate(value) {
  if (typeof value !== "string") {
    throw new ReleaseNotesError("releaseDate must be a string");
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    throw new ReleaseNotesError("releaseDate must be YYYY-MM-DD");
  }
  const parsed = new Date(`${value}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== value) {
    throw new ReleaseNotesError("releaseDate must be YYYY-MM-DD");
  }
  return value;
}

/** 确认对象精确包含指定字段。 */
function hasExactKeys(value, expected) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const actual = Object.keys(value).sort();
  const sorted = [...expected].sort();
  return actual.length === sorted.length && actual.every((key, index) => key === sorted[index]);
}

/** 校验单个分类的双语条目、逐语言去重和十条上限。 */
function validateItems(value, field) {
  if (!Array.isArray(value)) throw new ReleaseNotesError(`${field} must be an array`);
  if (value.length > MAX_ITEMS_PER_SECTION) {
    throw new ReleaseNotesError(`${field} must contain at most ${MAX_ITEMS_PER_SECTION} items`);
  }
  const seen = Object.fromEntries(SUPPORTED_LOCALES.map((locale) => [locale, new Set()]));
  return value.map((item) => {
    if (!hasExactKeys(item, ITEM_KEYS)) {
      throw new ReleaseNotesError(`${field} items must contain exactly zh-CN and en-US`);
    }
    const localized = {};
    for (const locale of SUPPORTED_LOCALES) {
      const text = item[locale];
      if (typeof text !== "string" || text.trim().length === 0) {
        throw new ReleaseNotesError(`${field} ${locale} items must be non-empty strings`);
      }
      const normalized = text.trim();
      if (normalized.startsWith("\uFEFF") || normalized.endsWith("\uFEFF")) {
        throw new ReleaseNotesError(`${field} ${locale} items must not contain a boundary BOM`);
      }
      if (seen[locale].has(normalized)) {
        throw new ReleaseNotesError(`${field} must not contain duplicate ${locale} items`);
      }
      seen[locale].add(normalized);
      localized[locale] = normalized;
    }
    return localized;
  });
}

/** 按位置配对中英文条目并拒绝任一语言缺项。 */
function pairLocalizedItems(zhCn, enUs, field) {
  if (zhCn.length !== enUs.length) {
    throw new ReleaseNotesError(`${field} must provide the same number of zh-CN and en-US items`);
  }
  return zhCn.map((text, index) => ({ "zh-CN": text, "en-US": enUs[index] }));
}

/** 校验并规范化一个版本的日期、展示版本与两类条目。 */
export function validateReleaseEntry(value) {
  if (!hasExactKeys(value, ENTRY_KEYS)) {
    throw new ReleaseNotesError("release entry keys do not match the schema");
  }
  const featureOptimizations = validateItems(value.featureOptimizations, "featureOptimizations");
  const bugFixes = validateItems(value.bugFixes, "bugFixes");
  if (featureOptimizations.length === 0 && bugFixes.length === 0) {
    throw new ReleaseNotesError("each release must contain at least one actual change");
  }
  return {
    releaseDate: validateReleaseDate(value.releaseDate),
    version: normalizeDisplayVersion(value.version),
    featureOptimizations,
    bugFixes,
  };
}

/** 校验整个更新日志的 schema、顺序、唯一版本和十版上限。 */
export function validateDocument(value) {
  if (!hasExactKeys(value, ROOT_KEYS)) {
    throw new ReleaseNotesError("release notes root keys do not match the schema");
  }
  if (!Number.isInteger(value.schemaVersion) || value.schemaVersion !== SCHEMA_VERSION) {
    throw new ReleaseNotesError(`schemaVersion must be ${SCHEMA_VERSION}`);
  }
  if (!Array.isArray(value.releases)) throw new ReleaseNotesError("releases must be an array");
  if (value.releases.length === 0) throw new ReleaseNotesError("releases must contain at least one entry");
  if (value.releases.length > MAX_RELEASES) {
    throw new ReleaseNotesError(`releases must contain at most ${MAX_RELEASES} entries`);
  }
  const releases = value.releases.map(validateReleaseEntry);
  const versions = releases.map((entry) => entry.version);
  if (new Set(versions).size !== versions.length) {
    throw new ReleaseNotesError("release versions must be unique");
  }
  const dates = releases.map((entry) => entry.releaseDate);
  if (JSON.stringify(dates) !== JSON.stringify([...dates].sort().reverse())) {
    throw new ReleaseNotesError("releases must be ordered newest first");
  }
  return { schemaVersion: SCHEMA_VERSION, releases };
}

/** 解析 JSON 并在任意对象层级拒绝重复字段。 */
function parseJsonWithoutDuplicateKeys(text) {
  let index = 0;
  const whitespace = /[\u0020\u000A\u000D\u0009]/;
  const skip = () => { while (index < text.length && whitespace.test(text[index])) index += 1; };
  const fail = () => { throw new SyntaxError(`invalid JSON at offset ${index}`); };
  const parseString = () => {
    const start = index;
    if (text[index] !== '"') fail();
    index += 1;
    while (index < text.length) {
      const character = text[index];
      if (character === '"') {
        index += 1;
        return JSON.parse(text.slice(start, index));
      }
      if (character === "\\") {
        index += 2;
      } else {
        if (character.codePointAt(0) < 0x20) fail();
        index += 1;
      }
    }
    fail();
  };
  const parseValue = () => {
    skip();
    const character = text[index];
    if (character === '"') return parseString();
    if (character === "{") {
      index += 1;
      skip();
      const value = {};
      const keys = new Set();
      if (text[index] === "}") { index += 1; return value; }
      while (index < text.length) {
        skip();
        const key = parseString();
        if (keys.has(key)) throw new ReleaseNotesError(`duplicate release notes JSON key: ${key}`);
        keys.add(key);
        skip();
        if (text[index] !== ":") fail();
        index += 1;
        value[key] = parseValue();
        skip();
        if (text[index] === "}") { index += 1; return value; }
        if (text[index] !== ",") fail();
        index += 1;
      }
      fail();
    }
    if (character === "[") {
      index += 1;
      skip();
      const value = [];
      if (text[index] === "]") { index += 1; return value; }
      while (index < text.length) {
        value.push(parseValue());
        skip();
        if (text[index] === "]") { index += 1; return value; }
        if (text[index] !== ",") fail();
        index += 1;
      }
      fail();
    }
    const remainder = text.slice(index);
    for (const [literal, value] of [["true", true], ["false", false], ["null", null]]) {
      if (remainder.startsWith(literal)) { index += literal.length; return value; }
    }
    const number = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(remainder);
    if (number) {
      index += number[0].length;
      const value = Number(number[0]);
      if (!Number.isFinite(value)) fail();
      return value;
    }
    fail();
  };
  const value = parseValue();
  skip();
  if (index !== text.length) fail();
  return value;
}

/** 对文件或 Git blob 的原始字节执行同一套严格更新日志校验。 */
export function parseDocumentBytes(bytes) {
  if (!(bytes instanceof Uint8Array)) throw new ReleaseNotesError("release notes must be bytes");
  if (bytes.byteLength > MAX_RELEASE_NOTES_BYTES) {
    throw new ReleaseNotesError("release notes exceed the 1 MiB resource limit");
  }
  let value;
  try {
    value = parseJsonWithoutDuplicateKeys(UTF8_DECODER.decode(bytes));
  } catch (error) {
    if (error instanceof ReleaseNotesError) throw error;
    throw new ReleaseNotesError(`cannot read valid release notes: ${error.message}`);
  }
  const normalized = validateDocument(value);
  if (!isDeepStrictEqual(value, normalized)) {
    throw new ReleaseNotesError("release notes file contains noncanonical values");
  }
  return normalized;
}

/** 读取普通 UTF-8 JSON 文件并校验规范值。 */
export function loadDocument(path) {
  let metadata;
  try {
    metadata = lstatSync(path);
  } catch {
    throw new ReleaseNotesError(`release notes must be a regular file: ${path}`);
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    throw new ReleaseNotesError(`release notes must be a regular file: ${path}`);
  }
  if (metadata.size > MAX_RELEASE_NOTES_BYTES) {
    throw new ReleaseNotesError("release notes exceed the 1 MiB resource limit");
  }
  try {
    return parseDocumentBytes(readFileSync(path));
  } catch (error) {
    if (error instanceof ReleaseNotesError) throw error;
    throw new ReleaseNotesError(`cannot read valid release notes: ${error.message}`);
  }
}

/** 在同目录原子写入完整更新日志。 */
function writeDocument(path, document) {
  const parent = dirname(path);
  let parentMetadata;
  try {
    parentMetadata = lstatSync(parent);
  } catch {
    throw new ReleaseNotesError(`release notes parent must be a regular directory: ${parent}`);
  }
  if (parentMetadata.isSymbolicLink() || !parentMetadata.isDirectory()) {
    throw new ReleaseNotesError(`release notes parent must be a regular directory: ${parent}`);
  }
  if (existsSync(path)) {
    const target = lstatSync(path);
    if (target.isSymbolicLink() || !target.isFile()) {
      throw new ReleaseNotesError(`release notes target must be a regular file: ${path}`);
    }
  }
  const encoded = Buffer.from(`${JSON.stringify(validateDocument(document), null, 2)}\n`, "utf8");
  if (encoded.length > MAX_RELEASE_NOTES_BYTES) {
    throw new ReleaseNotesError("release notes exceed the 1 MiB resource limit");
  }
  let temporary;
  let descriptor;
  try {
    for (let attempt = 0; attempt < 100; attempt += 1) {
      temporary = `${path}.${process.pid}.${randomBytes(8).toString("hex")}.tmp`;
      try {
        descriptor = openSync(temporary, "wx", 0o600);
        break;
      } catch (error) {
        if (error.code !== "EEXIST") throw error;
      }
    }
    if (descriptor === undefined) throw new ReleaseNotesError("cannot allocate release notes temporary file");
    writeFileSync(descriptor, encoded);
    fsyncSync(descriptor);
    closeSync(descriptor);
    descriptor = undefined;
    renameSync(temporary, path);
    temporary = undefined;
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
    if (temporary && existsSync(temporary)) unlinkSync(temporary);
  }
}

/** 新增或替换当前版本，置顶后只保留最近十个实际发布版本。 */
export function upsertRelease(path, options) {
  const existing = existsSync(path) || (() => {
    try { return lstatSync(path).isSymbolicLink(); } catch { return false; }
  })()
    ? loadDocument(path)
    : { schemaVersion: SCHEMA_VERSION, releases: [] };
  const entry = validateReleaseEntry({
    releaseDate: options.releaseDate,
    version: options.version,
    featureOptimizations: pairLocalizedItems(
      options.featureOptimizationsZhCn,
      options.featureOptimizationsEnUs,
      "featureOptimizations",
    ),
    bugFixes: pairLocalizedItems(options.bugFixesZhCn, options.bugFixesEnUs, "bugFixes"),
  });
  const releases = existing.releases.filter((item) => item.version !== entry.version);
  const document = { schemaVersion: SCHEMA_VERSION, releases: [entry, ...releases].slice(0, MAX_RELEASES) };
  writeDocument(path, document);
  return validateDocument(document);
}

/** 按指定语言渲染最近十个实际发布版本的更新日志。 */
export function renderDocument(document, locale) {
  const normalized = validateDocument(document);
  if (!SUPPORTED_LOCALES.includes(locale)) {
    throw new ReleaseNotesError(`locale must be one of ${SUPPORTED_LOCALES.join(", ")}`);
  }
  const copy = RENDER_COPY[locale];
  return normalized.releases.map((entry) => {
    const features = entry.featureOptimizations.map((item) => item[locale]);
    const fixes = entry.bugFixes.map((item) => item[locale]);
    return [
      `-----------${copy.title} ${entry.releaseDate} ${entry.version}----------`,
      "",
      `###${copy.featureOptimizations}`,
      "",
      ...(features.length ? features : [copy.none]).map((item) => `- ${item}`),
      "",
      `###${copy.bugFixes}`,
      "",
      ...(fixes.length ? fixes : [copy.none]).map((item) => `- ${item}`),
    ].join("\n");
  }).join("\n\n");
}

/** 把 kebab-case CLI 选项转换为内部 camelCase 字段。 */
function optionName(token) {
  return token.slice(2).replace(/-([a-z])/g, (_match, letter) => letter.toUpperCase());
}

/** 解析 check、render 与 upsert 的固定非交互参数。 */
function parseArguments(argv) {
  if (argv.length === 0 || !["check", "render", "upsert"].includes(argv[0])) {
    throw Object.assign(new Error("command is required"), { cliExit: 2 });
  }
  const args = { command: argv[0] };
  const repeated = new Set([
    "featureOptimizationZhCn", "featureOptimizationEnUs", "bugFixZhCn", "bugFixEnUs",
  ]);
  for (const field of repeated) args[field] = [];
  const allowed = {
    check: new Set(["file", "expectedVersion"]),
    render: new Set(["file", "locale"]),
    upsert: new Set(["file", "releaseDate", "version", ...repeated]),
  }[args.command];
  for (let index = 1; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--") || index + 1 >= argv.length || argv[index + 1].startsWith("--")) {
      throw Object.assign(new Error(`invalid argument: ${token}`), { cliExit: 2 });
    }
    const name = optionName(token);
    if (!allowed.has(name)) throw Object.assign(new Error(`invalid argument: ${token}`), { cliExit: 2 });
    if (repeated.has(name)) args[name].push(argv[index + 1]);
    else args[name] = argv[index + 1];
    index += 1;
  }
  if (args.file === undefined) throw Object.assign(new Error("--file is required"), { cliExit: 2 });
  if (args.command === "render" && !SUPPORTED_LOCALES.includes(args.locale)) {
    throw Object.assign(new Error("--locale must be zh-CN or en-US"), { cliExit: 2 });
  }
  if (args.command === "upsert" && (args.releaseDate === undefined || args.version === undefined)) {
    throw Object.assign(new Error("--release-date and --version are required"), { cliExit: 2 });
  }
  return args;
}

/** 执行维护命令并以稳定文本输出结果。 */
export function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseArguments(argv);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return error.cliExit ?? 2;
  }
  try {
    const path = resolve(args.file);
    if (args.command === "upsert") {
      const document = upsertRelease(path, {
        releaseDate: args.releaseDate,
        version: args.version,
        featureOptimizationsZhCn: args.featureOptimizationZhCn,
        featureOptimizationsEnUs: args.featureOptimizationEnUs,
        bugFixesZhCn: args.bugFixZhCn,
        bugFixesEnUs: args.bugFixEnUs,
      });
      process.stdout.write(`release-notes.updated=${document.releases[0].version} retained=${document.releases.length}\n`);
      return 0;
    }
    const document = loadDocument(path);
    if (args.command === "render") {
      process.stdout.write(`${renderDocument(document, args.locale)}\n`);
      return 0;
    }
    if (args.expectedVersion !== undefined) {
      const expected = normalizeDisplayVersion(args.expectedVersion);
      const actual = document.releases[0]?.version ?? null;
      if (actual !== expected) {
        throw new ReleaseNotesError(`latest release notes version is ${JSON.stringify(actual)}, expected ${JSON.stringify(expected)}`);
      }
    }
    process.stdout.write(`release-notes.valid=true retained=${document.releases.length}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`release-notes.error=${error.message}\n`);
    return 1;
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  process.exitCode = main();
}
