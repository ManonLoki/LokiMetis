#!/usr/bin/env node
/** 以可预览、无覆盖的方式统一替换项目身份文本和路径。 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const EXCLUDED_DIRECTORIES = new Set([
  ".git", ".idea", ".next", ".turbo", ".vscode", "build", "coverage", "dist",
  "node_modules", "release", "target", "vendor",
]);
const SNAKE_CASE = /^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$/;
const KEBAB_CASE = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;

function expandHome(value) {
  if (value === "~") return os.homedir();
  if (value.startsWith(`~${path.sep}`)) return path.join(os.homedir(), value.slice(2));
  return value;
}

function exists(value) {
  try { fs.lstatSync(value); return true; } catch (error) { if (error?.code === "ENOENT") return false; throw error; }
}

/** 建立去重且按旧文本长度降序排列的替换表，避免短前缀抢先匹配。 */
export function buildReplacements(values) {
  const pairs = [
    [values["old-display-name-zh"], values["new-display-name-zh"]],
    [values["old-display-name-en"], values["new-display-name-en"]],
    [values["old-id"], values["new-id"]],
    [values["old-kebab"], values["new-kebab"]],
  ];
  for (const raw of values.replace ?? []) {
    const separator = raw.indexOf("=");
    if (separator < 0) throw new Error(`--replace 值非法：${JSON.stringify(raw)}`);
    pairs.push([raw.slice(0, separator), raw.slice(separator + 1)]);
  }
  const normalized = new Map();
  for (const [oldValue, newValue] of pairs) {
    if (!oldValue || !newValue) throw new Error("替换值不得为空");
    if (oldValue === newValue) throw new Error(`旧值和新值必须不同：${JSON.stringify(oldValue)}`);
    const previous = normalized.get(oldValue);
    if (previous !== undefined && previous !== newValue) throw new Error(`以下旧值存在冲突的替换映射：${JSON.stringify(oldValue)}`);
    normalized.set(oldValue, newValue);
  }
  return [...normalized.entries()].sort((left, right) => right[0].length - left[0].length);
}

/** 按已排序映射替换文本，不推断大小写、词形或法律语义。 */
export function replaceExact(text, replacements) {
  let result = text;
  for (const [oldValue, newValue] of replacements) result = result.split(oldValue).join(newValue);
  return result;
}

/** 遍历项目维护树，同时拒绝符号链接并记录确定性排除目录。 */
export function inventory(root) {
  const files = [];
  const directories = [];
  const excluded = [];
  const visit = (current) => {
    const entries = fs.readdirSync(current, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, "en"));
    for (const entry of entries) {
      const entryPath = path.join(current, entry.name);
      const relative = path.relative(root, entryPath).split(path.sep).join("/");
      if (entry.isSymbolicLink()) throw new Error(`不允许符号链接：${relative}`);
      if (entry.isDirectory()) {
        if (EXCLUDED_DIRECTORIES.has(entry.name) && (entry.name !== "release" || current === root)) excluded.push(`${relative}/`);
        else { directories.push(entryPath); visit(entryPath); }
      } else {
        files.push(entryPath);
      }
    }
  };
  visit(root);
  return { files, directories, excluded };
}

/** 只把无 NUL 且可按 UTF-8 解码的文件视为可维护文本。 */
export function readText(file) {
  const payload = fs.readFileSync(file);
  if (payload.includes(0)) return { text: null, binary: true };
  try {
    return { text: new TextDecoder("utf-8", { fatal: true }).decode(payload), binary: false };
  } catch {
    return { text: null, binary: true };
  }
}

function relativePosix(root, value) {
  return path.relative(root, value).split(path.sep).join("/");
}

function isOutside(relative) {
  return relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative);
}

/** 只替换当前路径项名称，父目录由独立的深度倒序改名负责。 */
export function renamedPath(value, root, replacements) {
  const relative = path.relative(root, value);
  if (isOutside(relative)) throw new Error(`路径越界：${value}`);
  const destination = path.join(path.dirname(value), replaceExact(path.basename(value), replacements));
  const destinationRelative = path.relative(root, destination);
  if (isOutside(destinationRelative)) throw new Error(`目标路径越界：${destination}`);
  return destination;
}

/** 生成内容修改、路径改名、排除目录和二进制跳过项，不写入磁盘。 */
export function planChanges(root, replacements) {
  const { files, directories, excluded } = inventory(root);
  const contentChanges = [];
  const skippedBinary = [];
  for (const file of files) {
    const { text, binary } = readText(file);
    if (binary) { skippedBinary.push(relativePosix(root, file)); continue; }
    const updated = replaceExact(text, replacements);
    if (updated !== text) contentChanges.push([file, updated]);
  }
  const pathChanges = [];
  for (const item of [...files, ...directories]) {
    const destination = renamedPath(item, root, replacements);
    if (destination !== item) pathChanges.push([item, destination]);
  }
  const sources = new Set(pathChanges.map(([source]) => source));
  const destinations = new Set();
  for (const [, destination] of pathChanges) {
    if (destinations.has(destination)) throw new Error(`多个路径将映射到同一目标：${relativePosix(root, destination)}`);
    destinations.add(destination);
    if (exists(destination) && !sources.has(destination)) throw new Error(`目标已存在：${relativePosix(root, destination)}`);
  }
  return { contentChanges, pathChanges, excluded, skippedBinary };
}

/** 先原子替换文本，再按路径深度倒序改名，避免父目录先移动子项。 */
export function applyChanges(contentChanges, pathChanges) {
  for (const [file, updated] of contentChanges) {
    const mode = fs.statSync(file).mode;
    const temporary = `${file}.desktop-rename-project-identity.tmp`;
    if (exists(temporary)) throw new Error(`临时路径已存在：${temporary}`);
    try {
      fs.writeFileSync(temporary, updated, { encoding: "utf8", mode, flag: "wx" });
      fs.chmodSync(temporary, mode);
      fs.renameSync(temporary, file);
    } catch (error) {
      try { fs.rmSync(temporary, { force: true }); } catch { /* 保留原始错误 */ }
      throw error;
    }
  }
  for (const [source, destination] of [...pathChanges].sort((a, b) => b[0].split(path.sep).length - a[0].split(path.sep).length)) {
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.renameSync(source, destination);
  }
}

/** 仅在显式启用且根目录名精确等于旧标识时规划同级目录改名。 */
export function planRootRename(root, oldId, newId, enabled) {
  if (!enabled) return null;
  if (path.basename(root) !== oldId) throw new Error("--rename-root 要求项目根目录基本名称与 --old-id 相同");
  const destination = path.join(path.dirname(root), newId);
  if (exists(destination)) throw new Error(`根目录重命名目标已存在：${destination}`);
  return destination;
}

/** 复扫维护树中的旧身份文本和路径，确保应用后没有静默遗漏。 */
export function findResiduals(root, oldValues) {
  const { files, directories } = inventory(root);
  const residuals = [];
  for (const item of [...files, ...directories]) {
    const relative = relativePosix(root, item);
    if (oldValues.some((oldValue) => relative.includes(oldValue))) residuals.push(`path:${relative}`);
  }
  for (const file of files) {
    const { text, binary } = readText(file);
    if (!binary && oldValues.some((oldValue) => text.includes(oldValue))) residuals.push(`text:${relativePosix(root, file)}`);
  }
  return [...new Set(residuals)].sort();
}

function commandValues() {
  const stringOptions = ["root", "old-display-name-zh", "new-display-name-zh", "old-display-name-en", "new-display-name-en", "old-id", "new-id", "old-kebab", "new-kebab"];
  const options = Object.fromEntries(stringOptions.map((name) => [name, { type: "string" }]));
  options.replace = { type: "string", multiple: true, default: [] };
  options.apply = { type: "boolean", default: false };
  options["rename-root"] = { type: "boolean", default: false };
  const { values } = parseArgs({ options, strict: true });
  for (const name of stringOptions) if (!values[name]) throw new Error(`缺少必需参数 --${name}`);
  return values;
}

function main() {
  const values = commandValues();
  let root = fs.realpathSync(expandHome(values.root));
  const originalRoot = root;
  if (!fs.statSync(root).isDirectory()) throw new Error(`项目根目录不是目录：${root}`);
  if (!SNAKE_CASE.test(values["old-id"]) || !SNAKE_CASE.test(values["new-id"])) throw new Error("--old-id 和 --new-id 必须是 ASCII snake_case");
  if (!KEBAB_CASE.test(values["old-kebab"]) || !KEBAB_CASE.test(values["new-kebab"])) throw new Error("--old-kebab 和 --new-kebab 必须是小写 kebab-case");
  const replacements = buildReplacements(values);
  const rootDestination = planRootRename(root, values["old-id"], values["new-id"], values["rename-root"]);
  const { contentChanges, pathChanges, excluded, skippedBinary } = planChanges(root, replacements);
  if (values.apply) {
    applyChanges(contentChanges, pathChanges);
    if (rootDestination) { fs.renameSync(root, rootDestination); root = rootDestination; }
  }
  const residuals = values.apply ? findResiduals(root, replacements.map(([oldValue]) => oldValue)) : [];
  return {
    exitCode: residuals.length ? 2 : 0,
    result: {
      mode: values.apply ? "apply" : "preview",
      root,
      rootRename: rootDestination,
      replacements: replacements.map(([oldValue, newValue]) => ({ old: oldValue, new: newValue })),
      contentFiles: contentChanges.map(([file]) => relativePosix(originalRoot, file)),
      pathRenames: pathChanges.map(([source, destination]) => ({ from: relativePosix(originalRoot, source), to: relativePosix(originalRoot, destination) })),
      excludedDirectories: [...excluded].sort(),
      skippedBinaryFiles: [...skippedBinary].sort(),
      residuals,
    },
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    const { exitCode, result } = main();
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    process.exitCode = exitCode;
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ error: error.message })}\n`);
    process.exitCode = 1;
  }
}
