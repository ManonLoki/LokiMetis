#!/usr/bin/env node

/** 按 Rust、前端与通用文本配置报告重构候选并拒绝硬超限文件。 */

import { lstatSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const RUST_REVIEW_THRESHOLD = 400;
export const RUST_HARD_LINE_LIMIT = 800;
export const FRONTEND_REVIEW_THRESHOLD = 500;
export const FRONTEND_HARD_LINE_LIMIT = 1000;
export const DEFAULT_REVIEW_THRESHOLD = 500;
export const DEFAULT_HARD_LINE_LIMIT = 2000;

export const LINE_LIMIT_PROFILES = Object.freeze({
  rust: { label: "Rust 代码", reviewThreshold: RUST_REVIEW_THRESHOLD, hardLimit: RUST_HARD_LINE_LIMIT },
  frontend: { label: "前端代码", reviewThreshold: FRONTEND_REVIEW_THRESHOLD, hardLimit: FRONTEND_HARD_LINE_LIMIT },
  maintained_text: { label: "其他人工维护文本", reviewThreshold: DEFAULT_REVIEW_THRESHOLD, hardLimit: DEFAULT_HARD_LINE_LIMIT },
});

export const FRONTEND_CODE_SUFFIXES = new Set([
  ".astro", ".cjs", ".css", ".cts", ".html", ".js", ".jsx", ".less", ".mjs",
  ".mts", ".sass", ".scss", ".svelte", ".ts", ".tsx", ".vue",
]);
const GENERATED_LOCKFILE_NAMES = new Set([
  "Cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock",
]);
const GENERATED_LOCKFILE_PATHS = new Set([".harness/upstream-lock.json"]);

export function lstatOrNull(target) {
  try { return lstatSync(target); } catch (error) {
    if (error?.code === "ENOENT" || error?.code === "ENOTDIR") return null;
    throw error;
  }
}

export function canonicalRoot(root) {
  const stat = lstatOrNull(root);
  if (stat?.isSymbolicLink()) return [null, [`项目根不得是符号链接: ${root}`]];
  let resolved;
  try { resolved = realpathSync(root); } catch (error) { return [null, [`无法解析项目根 ${root}: ${error.message}`]]; }
  if (!lstatSync(resolved).isDirectory()) return [null, [`项目根不是目录: ${resolved}`]];
  return [resolved, []];
}

export function gitVisiblePaths(root) {
  let result;
  try {
    result = spawnSync("git", ["-C", root, "ls-files", "--cached", "--others", "--exclude-standard", "-z"], {
      encoding: "buffer",
      maxBuffer: 64 * 1024 * 1024,
    });
  } catch (error) {
    return [[], [`无法执行 Git 文件枚举: ${error.message}`]];
  }
  if (result.error) return [[], [`无法执行 Git 文件枚举: ${result.error.message}`]];
  if (result.status !== 0) {
    const detail = new TextDecoder("utf-8", { fatal: false }).decode(result.stderr).trim();
    return [[], [`Git 文件枚举失败(${result.status}): ${detail || "无诊断"}`]];
  }
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const paths = [];
  try {
    let start = 0;
    for (let index = 0; index < result.stdout.length; index += 1) {
      if (result.stdout[index] === 0) {
        if (index > start) paths.push(decoder.decode(result.stdout.subarray(start, index)));
        start = index + 1;
      }
    }
  } catch (error) {
    return [[], [`Git 路径不是 UTF-8，无法安全检查: ${error.message}`]];
  }
  if (paths.length !== new Set(paths).size) return [[], ["Git 文件枚举包含重复路径"]];
  return [paths.sort(), []];
}

function safeCandidate(root, relative) {
  if (!relative || path.isAbsolute(relative)) return [null, `Git 返回非法相对路径: ${JSON.stringify(relative)}`, null];
  const candidate = path.join(root, relative);
  const lexical = path.resolve(candidate);
  const rel = path.relative(root, lexical);
  if (rel === ".." || rel.startsWith(`..${path.sep}`) || path.isAbsolute(rel)) {
    return [null, `Git 路径越过项目根: ${JSON.stringify(relative)}`, null];
  }
  let stat;
  try { stat = lstatOrNull(candidate); } catch (error) { return [null, `无法检查 Git 路径 ${JSON.stringify(relative)}: ${error.message}`, null]; }
  if (!stat) return [null, null, "missing"];
  if (stat.isSymbolicLink()) return [null, null, "symlink"];
  if (!stat.isFile()) return [null, `Git 路径不是普通文件: ${JSON.stringify(relative)}`, null];
  return [candidate, null, null];
}

function readText(candidate, relative) {
  let payload;
  try { payload = readFileSync(candidate); } catch (error) { return [null, `无法读取 ${JSON.stringify(relative)}: ${error.message}`]; }
  if (payload.includes(0)) return [null, null];
  try { return [new TextDecoder("utf-8", { fatal: true }).decode(payload), null]; }
  catch (error) { return [null, `无 NUL 的 Git 文件不是 UTF-8，无法分类 ${JSON.stringify(relative)}: ${error.message}`]; }
}

export function lineLimitProfile(relative) {
  const suffix = path.extname(relative).toLowerCase();
  const profileName = suffix === ".rs" ? "rust" : FRONTEND_CODE_SUFFIXES.has(suffix) ? "frontend" : "maintained_text";
  const profile = LINE_LIMIT_PROFILES[profileName];
  return [profileName, profile.reviewThreshold, profile.hardLimit];
}

export function physicalLineCount(text) {
  if (text.length === 0) return 0;
  const parts = text.split(/\r\n|[\n\r\v\f\x1c-\x1e\x85\u2028\u2029]/u);
  if (parts.at(-1) === "") parts.pop();
  return parts.length;
}

export function inspectRepository(root) {
  const [canonical, errors] = canonicalRoot(root);
  const report = {
    ok: false,
    root: String(root),
    lineLimitProfiles: LINE_LIMIT_PROFILES,
    checkedTextFiles: 0,
    excludedGeneratedFiles: [],
    skippedBinaryFiles: 0,
    skippedMissingFiles: 0,
    skippedSymlinks: 0,
    reviewCandidates: [],
    violations: [],
    errors,
  };
  if (canonical === null) return report;
  report.root = canonical;
  const [relativePaths, gitErrors] = gitVisiblePaths(canonical);
  report.errors.push(...gitErrors);
  if (gitErrors.length > 0) return report;
  for (const relative of relativePaths) {
    const [candidate, pathError, skipReason] = safeCandidate(canonical, relative);
    if (pathError) { report.errors.push(pathError); continue; }
    if (candidate === null) {
      if (skipReason === "symlink") report.skippedSymlinks += 1;
      else if (skipReason === "missing") report.skippedMissingFiles += 1;
      continue;
    }
    if (GENERATED_LOCKFILE_NAMES.has(path.basename(candidate)) || GENERATED_LOCKFILE_PATHS.has(relative)) {
      report.excludedGeneratedFiles.push(relative);
      continue;
    }
    const [text, readError] = readText(candidate, relative);
    if (readError) { report.errors.push(readError); continue; }
    if (text === null) { report.skippedBinaryFiles += 1; continue; }
    report.checkedTextFiles += 1;
    const lines = physicalLineCount(text);
    const [profile, threshold, limit] = lineLimitProfile(relative);
    const item = { path: relative, lines, profile, threshold, limit };
    if (lines > limit) report.violations.push(item);
    else if (lines > threshold) report.reviewCandidates.push(item);
  }
  report.excludedGeneratedFiles.sort();
  report.reviewCandidates.sort((left, right) => left.path.localeCompare(right.path));
  report.violations.sort((left, right) => left.path.localeCompare(right.path));
  report.errors.sort();
  report.ok = report.errors.length === 0 && report.violations.length === 0;
  return report;
}

export function parseArguments(args) {
  let root = process.cwd();
  let json = false;
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === "--json") json = true;
    else if (args[index] === "--root" && args[index + 1] !== undefined) { root = args[index + 1]; index += 1; }
    else throw new Error(`unsupported argument: ${args[index]}`);
  }
  return { root, json };
}

export function main(args = process.argv.slice(2), inspector = inspectRepository) {
  const options = parseArguments(args);
  const report = inspector(options.root);
  if (options.json) process.stdout.write(`${JSON.stringify(report)}\n`);
  else {
    for (const error of report.errors) process.stderr.write(`ERROR: ${error}\n`);
    for (const candidate of report.reviewCandidates) {
      const profile = LINE_LIMIT_PROFILES[candidate.profile];
      process.stderr.write(
        `REVIEW: ${profile.label}超过 ${candidate.threshold} 行，建议按职责重构并复核高内聚、职责单一和职责相近性: ${candidate.path} (${candidate.lines} lines)\n`,
      );
    }
    for (const violation of report.violations) {
      const profile = LINE_LIMIT_PROFILES[violation.profile];
      const rustHint = violation.profile === "rust" ? "；拆分 Rust 模块时必须使用 <module>/mod.rs 目录结构" : "";
      process.stderr.write(`ERROR: ${profile.label}超过 ${violation.limit} 行，必须按职责拆分${rustHint}: ${violation.path} (${violation.lines} lines)\n`);
    }
    if (report.ok) {
      process.stdout.write(
        `File line-limit check passed: ${report.checkedTextFiles} maintained text file(s), profiles=rust(400/800), frontend(500/1000), maintained-text(500/2000).\n`,
      );
    }
  }
  if (report.errors.length > 0) return 2;
  return report.ok ? 0 : 1;
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  try { process.exitCode = main(); }
  catch (error) { process.stderr.write(`ERROR: ${error.message}\n`); process.exitCode = 2; }
}
