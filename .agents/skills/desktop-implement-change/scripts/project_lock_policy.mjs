/** 读取下游锁文件选择，并在显式 tracked 模式下核对实际工作区的锁文件。 */

import { spawnSync } from "node:child_process";
import { lstatSync, readFileSync, realpathSync } from "node:fs";
import path from "node:path";

const POLICY_KEY = "dependency-lock-policy";
const POLICY_TABLE = ["workspace", "metadata", "agent-first-harness"];
const POLICY_PATH = [...POLICY_TABLE, POLICY_KEY];

function fileStat(target, label) {
  let stat;
  try { stat = lstatSync(target); }
  catch (error) { throw new Error(`${label} 不存在：${target} (${error.message})`); }
  if (stat.isSymbolicLink() || !stat.isFile()) throw new Error(`${label} 必须是普通非符号链接文件：${target}`);
  return stat;
}

function projectRoot(projectRootValue) {
  const supplied = path.resolve(projectRootValue);
  let stat;
  try { stat = lstatSync(supplied); }
  catch (error) { throw new Error(`项目根不存在：${supplied} (${error.message})`); }
  if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`项目根必须是普通目录：${supplied}`);
  return realpathSync(supplied);
}

/** 去掉 TOML 注释，并跳过多行字符串中的伪节名。只解析本 helper 自有的标量元数据。 */
function tomlLines(source) {
  const result = [];
  let multiline = null;
  for (const line of source.split(/\r?\n/u)) {
    let visible = "";
    let quote = null;
    let escaped = false;
    for (let index = 0; index < line.length;) {
      const nextThree = line.slice(index, index + 3);
      if (multiline !== null) {
        if (nextThree === multiline && !escaped) { multiline = null; index += 3; continue; }
        if (line[index] === "\\" && multiline === '"""') escaped = !escaped;
        else escaped = false;
        index += 1;
        continue;
      }
      if (quote !== null) {
        const character = line[index];
        visible += character;
        if (quote === '"' && escaped) escaped = false;
        else if (quote === '"' && character === "\\") escaped = true;
        else if (character === quote) quote = null;
        index += 1;
        continue;
      }
      if (nextThree === '"""' || nextThree === "'''") {
        multiline = nextThree;
        visible += nextThree;
        index += 3;
      } else if (line[index] === '"' || line[index] === "'") {
        quote = line[index];
        visible += line[index];
        index += 1;
      } else if (line[index] === "#") break;
      else { visible += line[index]; index += 1; }
    }
    result.push(visible.trim());
  }
  return result;
}

function tomlBasicKey(source) {
  let value = "";
  for (let index = 1; index < source.length - 1; index += 1) {
    const character = source[index];
    if (character !== "\\") {
      if (/[\u0000-\u001f\u007f]/u.test(character)) return null;
      value += character;
      continue;
    }
    const escape = source[++index];
    if (escape === "u" || escape === "U") {
      const digits = escape === "u" ? 4 : 8;
      const hex = source.slice(index + 1, index + 1 + digits);
      if (hex.length !== digits || !/^[0-9a-f]+$/iu.test(hex)) return null;
      const codePoint = Number.parseInt(hex, 16);
      if (codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff)) return null;
      value += String.fromCodePoint(codePoint);
      index += digits;
    } else {
      const escaped = { b: "\b", t: "\t", n: "\n", f: "\f", r: "\r", '"': '"', "\\": "\\" }[escape];
      if (escaped === undefined) return null;
      value += escaped;
    }
  }
  return value;
}

function tomlKeyPath(source) {
  const parts = [];
  let index = 0;
  while (index < source.length) {
    while (/\s/u.test(source[index] ?? "")) index += 1;
    const quote = source[index];
    let part;
    if (quote === '"' || quote === "'") {
      const start = index;
      index += 1;
      let escaped = false;
      while (index < source.length) {
        const character = source[index];
        if (quote === '"' && escaped) escaped = false;
        else if (quote === '"' && character === "\\") escaped = true;
        else if (character === quote) break;
        index += 1;
      }
      if (index === source.length) return null;
      const quoted = source.slice(start, index + 1);
      part = quote === '"' ? tomlBasicKey(quoted) : quoted.slice(1, -1);
      if (part === null) return null;
      index += 1;
    } else {
      const match = /^[A-Za-z0-9_-]+/u.exec(source.slice(index));
      if (!match) return null;
      part = match[0];
      index += part.length;
    }
    parts.push(part);
    while (/\s/u.test(source[index] ?? "")) index += 1;
    if (index === source.length) return parts;
    if (source[index] !== ".") return null;
    index += 1;
    if (index === source.length) return null;
  }
  return null;
}

function assignmentSeparator(line) {
  let quote = null;
  let escaped = false;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    if (quote !== null) {
      if (quote === '"' && escaped) escaped = false;
      else if (quote === '"' && character === "\\") escaped = true;
      else if (character === quote) quote = null;
    } else if (character === '"' || character === "'") quote = character;
    else if (character === "=") return index;
  }
  return -1;
}

function samePath(left, right) {
  return left?.length === right.length && left.every((part, index) => part === right[index]);
}

function isPolicyPrefix(pathParts) {
  return pathParts !== null && pathParts.length <= POLICY_PATH.length
    && pathParts.every((part, index) => part === POLICY_PATH[index]);
}

/** 缺省是 ignored；非法类型、未知值和重复定义一律失败关闭。 */
export function readDependencyLockPolicy(projectRootValue) {
  const root = projectRoot(projectRootValue);
  const manifest = path.join(root, "Cargo.toml");
  fileStat(manifest, "根 Cargo 清单");
  const source = new TextDecoder("utf-8", { fatal: true }).decode(readFileSync(manifest));
  let sectionCount = 0;
  let tablePath = [];
  const values = [];
  for (const line of tomlLines(source)) {
    if (line.startsWith("[")) {
      const arrayTable = line.startsWith("[[") && line.endsWith("]]");
      const table = line.endsWith("]") ? tomlKeyPath(line.slice(arrayTable ? 2 : 1, arrayTable ? -2 : -1)) : null;
      if (!table && line.includes(POLICY_KEY)) throw new Error(`Cargo ${POLICY_KEY} 表路径无效`);
      if (arrayTable && (samePath(table, POLICY_TABLE) || samePath(table, POLICY_PATH))) {
        throw new Error(`Cargo ${POLICY_KEY} 必须是字符串元数据`);
      }
      if (samePath(table, POLICY_PATH)) throw new Error(`Cargo ${POLICY_KEY} 必须是字符串元数据`);
      tablePath = table;
      if (samePath(table, POLICY_TABLE)) {
        sectionCount += 1;
      }
      continue;
    }
    if (!line) continue;
    const separator = assignmentSeparator(line);
    const key = separator < 0 ? null : tomlKeyPath(line.slice(0, separator).trim());
    const fullPath = key && tablePath ? [...tablePath, ...key] : null;
    const value = separator < 0 ? "" : line.slice(separator + 1).trim();
    if (samePath(fullPath, POLICY_PATH)) values.push(value);
    else if (key?.includes(POLICY_KEY) || (isPolicyPrefix(fullPath) && value.startsWith("{"))) {
      throw new Error(`Cargo ${POLICY_KEY} 元数据位置或语法无效`);
    }
  }
  if (sectionCount > 1 || values.length > 1) throw new Error(`Cargo ${POLICY_KEY} 元数据重复`);
  if (values.length === 0) return "ignored";
  const value = values[0];
  let policy;
  try {
    if (value.startsWith("'")) {
      if (!/^'[^']*'$/u.test(value)) throw new Error("invalid literal string");
      policy = value.slice(1, -1);
    } else policy = JSON.parse(value);
  } catch { throw new Error(`Cargo ${POLICY_KEY} 必须是 "ignored" 或 "tracked" 字符串`); }
  if (policy !== "ignored" && policy !== "tracked") {
    throw new Error(`Cargo ${POLICY_KEY} 必须是 "ignored" 或 "tracked" 字符串`);
  }
  return policy;
}

function safeRelative(value, label, { allowRoot = false } = {}) {
  if (allowRoot && value === ".") return ".";
  if (typeof value !== "string" || !value || value.includes("\\") || value.includes(":") || path.isAbsolute(value)
      || value.split("/").some((segment) => !segment || segment === "." || segment === "..")) {
    throw new Error(`${label} 必须是项目内规范相对路径`);
  }
  return value;
}

function checkedPath(root, relative, label, { directory = false } = {}) {
  let cursor = root;
  for (const segment of relative === "." ? [] : relative.split("/")) {
    cursor = path.join(cursor, segment);
    let stat;
    try { stat = lstatSync(cursor); }
    catch (error) { throw new Error(`${label} 不存在：${relative} (${error.message})`); }
    if (stat.isSymbolicLink()) throw new Error(`${label} 不得是符号链接：${relative}`);
  }
  const stat = lstatSync(cursor);
  if (directory ? !stat.isDirectory() : !stat.isFile()) throw new Error(`${label} 类型不正确：${relative}`);
  return cursor;
}

function runGit(root, git, args) {
  const result = spawnSync(git, ["-C", root, ...args], { encoding: "buffer", maxBuffer: 16 * 1024 * 1024 });
  if (result.error) throw new Error(`无法执行 Git：${result.error.message}`);
  return result;
}

function gitText(buffer) { return new TextDecoder("utf-8", { fatal: true }).decode(buffer); }

function assertGitRoot(root, git) {
  const result = runGit(root, git, ["rev-parse", "--show-toplevel"]);
  if (result.status !== 0 || path.resolve(gitText(result.stdout).trim()) !== root) {
    throw new Error(`项目根必须是独立 Git 顶层：${root}`);
  }
}

function assertTrackedNotIgnored(root, relative, git) {
  const tracked = runGit(root, git, ["ls-files", "--stage", "--cached", "--full-name", "-z", "--", `:(literal)${relative}`]);
  if (tracked.status !== 0) throw new Error(`无法核对锁文件 Git 索引：${relative}`);
  const records = gitText(tracked.stdout).split("\0");
  if (records.pop() !== "") throw new Error(`锁文件 Git 索引输出无效：${relative}`);
  if (records.length === 0) {
    throw new Error(`tracked 锁文件必须受 Git 跟踪：${relative}`);
  }
  const separator = records.length === 1 ? records[0].indexOf("\t") : -1;
  const entry = separator < 0 ? null : /^([0-7]{6}) ([0-9a-f]{40}|[0-9a-f]{64}) ([0-3])$/iu.exec(records[0].slice(0, separator));
  if (!entry || records[0].slice(separator + 1) !== relative || !["100644", "100755"].includes(entry[1]) || entry[3] !== "0") {
    throw new Error(`tracked 锁文件 Git 索引必须是唯一的 stage-0 普通文件：${relative}`);
  }
  const ignored = runGit(root, git, ["check-ignore", "-q", "--no-index", "--", relative]);
  if (ignored.status === 0) throw new Error(`tracked 锁文件不得被 Git 忽略：${relative}`);
  if (ignored.status !== 1) throw new Error(`无法核对锁文件 Git 忽略状态：${relative}`);
}

function cargoWorkspaceLock(root, manifest) {
  const result = spawnSync("cargo", ["locate-project", "--workspace", "--offline", "--message-format", "json",
    "--manifest-path", path.join(root, manifest)], { cwd: root, encoding: "buffer", maxBuffer: 16 * 1024 * 1024 });
  if (result.error) throw new Error(`无法执行 Cargo workspace 定位：${result.error.message}`);
  if (result.status !== 0) throw new Error(`Cargo 无法确认 workspace 根：${manifest} (${gitText(result.stderr).trim()})`);
  let located;
  try { located = JSON.parse(gitText(result.stdout)).root; }
  catch { throw new Error(`Cargo workspace 定位输出无效：${manifest}`); }
  if (typeof located !== "string" || !path.isAbsolute(located) || path.basename(located) !== "Cargo.toml") {
    throw new Error(`Cargo workspace 根清单路径无效：${manifest}`);
  }
  const relative = path.relative(root, located);
  if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error(`Cargo workspace 根不得离开项目：${manifest}`);
  }
  const workspaceManifest = relative.split(path.sep).join("/");
  checkedPath(root, workspaceManifest, "Cargo workspace 根清单");
  return path.posix.join(path.posix.dirname(workspaceManifest), "Cargo.lock");
}

/**
 * tracked 模式核对每个实际 Cargo workspace 的 Cargo.lock，及已选 GUI 根的 pnpm-lock.yaml。
 * rustTestManifests 可列 workspace 成员清单；无 GUI 时 guiRoot 留为 null。
 */
export function assertDependencyLocks(projectRootValue, { rustTestManifests = ["Cargo.toml"], guiRoot = null, git = "git" } = {}) {
  const root = projectRoot(projectRootValue);
  const policy = readDependencyLockPolicy(root);
  if (policy === "ignored") return { policy, locks: [] };
  if (!Array.isArray(rustTestManifests) || rustTestManifests.length === 0) {
    throw new Error("rustTestManifests 必须列出至少一个实际 Cargo workspace 清单");
  }
  assertGitRoot(root, git);
  const locks = new Set();
  for (const manifestValue of rustTestManifests) {
    const manifest = safeRelative(manifestValue, "Rust workspace 清单");
    if (path.posix.basename(manifest) !== "Cargo.toml") throw new Error(`Rust workspace 清单必须命名为 Cargo.toml：${manifest}`);
    checkedPath(root, manifest, "Rust workspace 清单");
    locks.add(cargoWorkspaceLock(root, manifest));
  }
  if (guiRoot !== null) {
    const relativeGui = safeRelative(guiRoot, "GUI 根", { allowRoot: true });
    checkedPath(root, relativeGui, "GUI 根", { directory: true });
    locks.add(path.posix.join(relativeGui, "pnpm-lock.yaml"));
  }
  for (const relative of [...locks].sort()) {
    checkedPath(root, relative, "tracked 锁文件");
    assertTrackedNotIgnored(root, relative, git);
  }
  return { policy, locks: [...locks].sort() };
}
