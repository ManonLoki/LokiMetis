#!/usr/bin/env node

/** 读取并原子更新终端下游的发布后动作选择。 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { assertDependencyLocks } from "../../desktop-implement-change/scripts/project_lock_policy.mjs";

const POLICY_RELATIVE = path.join("docs", "AGENT_POLICY.md");
/** 发布后动作的唯一合法取值；生命周期与仓库校验器共享同一集合。 */
export const POST_RELEASE_ACTIONS = Object.freeze(["local_package", "push_release_branch"]);
const ACTIONS = new Set(POST_RELEASE_ACTIONS);
const V3_FIELDS = ["schema_version", "confirmed_by", "confirmed_at", "decision_mode", "superpowers", "user_owned_tasks", "parallel_worktree_subagents", "acceptance_smoke", "e2e_hint"];
const V4_FIELDS = [...V3_FIELDS, "post_release_action"];
const PREFERENCES = ["superpowers", "user_owned_tasks", "parallel_worktree_subagents", "acceptance_smoke", "e2e_hint"];
const REQUIRED_BODY = [
  "- `post_release_action`：`local_package`",
  "发布后动作直接读取 `post_release_action`",
  "冻结已确认的 `post_release_action`",
  "`push-release --remote <name>`",
  "远端默认主分支、`release` 分支和 tag",
];
const STALE_BODY = [
  "推送与打包分别由发布后的用户请求决定",
  "发布后是否推送、是否打包只由用户各自的明确请求决定",
  "流程没有发布中转分支",
  "发布后用户另外明确要求推送",
  "远端默认主分支不得因此改变",
  "不重新合并或改动远端默认主分支",
  "远端 advertised 默认分支不因该路径移动",
];

export class ActionError extends Error {
  constructor(message) { super(message); this.name = "ActionError"; }
}

function regularFile(target, label) {
  let stat;
  try { stat = fs.lstatSync(target); } catch { throw new ActionError(`${label} 不存在：${target}`); }
  if (!stat.isFile() || stat.isSymbolicLink()) throw new ActionError(`${label} 必须是普通非符号链接文件：${target}`);
  return stat;
}

/** 判断 confirmed_at 是否为日历合法的 ISO 日期或带日期前缀的可解析时间戳。 */
export function validConfirmedAt(value) {
  if (/^\d{4}-\d{2}-\d{2}$/u.test(value)) {
    const [year, month, day] = value.split("-").map(Number);
    const date = new Date(Date.UTC(year, month - 1, day));
    return date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
  }
  return /^\d{4}-\d{2}-\d{2}T/u.test(value) && !Number.isNaN(Date.parse(value));
}

function projectRoot(raw) {
  if (typeof raw !== "string" || !raw || !path.isAbsolute(raw)) throw new ActionError("--project-root 必须是绝对路径");
  const unresolved = path.resolve(raw);
  let stat;
  try { stat = fs.lstatSync(unresolved); } catch { throw new ActionError(`项目根不存在：${unresolved}`); }
  if (!stat.isDirectory() || stat.isSymbolicLink()) throw new ActionError("项目根必须是普通非符号链接目录");
  const root = fs.realpathSync(unresolved);
  if (root !== unresolved) throw new ActionError("项目根路径包含符号链接");
  const git = spawnSync("git", ["-C", root, "rev-parse", "--show-toplevel"], { encoding: "utf8" });
  if (git.status !== 0 || path.resolve(git.stdout.trim()) !== root) throw new ActionError("必须在独立下游 Git 顶层目录执行");
  if (fs.existsSync(path.join(root, "Version.md")) && fs.existsSync(path.join(root, ".agents", "skills", "desktop-instantiate-project", "SKILL.md"))) {
    throw new ActionError("Harness 模板源不能设置下游发布后动作");
  }
  const docs = path.join(root, "docs");
  const docsStat = fs.lstatSync(docs);
  if (!docsStat.isDirectory() || docsStat.isSymbolicLink()) throw new ActionError("docs 必须是普通非符号链接目录");
  return root;
}

/** 解析受管策略文档的字节、换行及字段结构，供运行时与 Harness 校验器共用。 */
export function parseAgentPolicyDocument(bytes) {
  let source;
  try { source = typeof bytes === "string" ? bytes : new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
  catch { throw new ActionError("Agent 策略必须是有效 UTF-8"); }
  if (source.includes("\r") && source.replaceAll("\r\n", "").includes("\r")) throw new ActionError("Agent 策略包含混合换行");
  if (source.includes("\r\n") && source.replaceAll("\r\n", "").includes("\n")) throw new ActionError("Agent 策略包含混合换行");
  const newline = source.includes("\r\n") ? "\r\n" : "\n";
  const normalized = source.replaceAll("\r\n", "\n");
  const match = /^---\n([\s\S]*?)\n---\n/u.exec(normalized);
  if (!match) throw new ActionError("Agent 策略缺少完整 YAML frontmatter");
  const lines = match[1].split("\n");
  const fields = new Map();
  for (const line of lines) {
    const field = /^([a-z][a-z0-9_]*): ([^\n]*)$/u.exec(line);
    if (!field) throw new ActionError(`不支持的 Agent 策略字段行：${line}`);
    if (fields.has(field[1])) throw new ActionError(`重复的 Agent 策略字段：${field[1]}`);
    fields.set(field[1], field[2]);
  }
  const schema = fields.get("schema_version");
  const expected = schema === "3" ? V3_FIELDS : schema === "4" ? V4_FIELDS : null;
  if (!expected || fields.size !== expected.length || expected.some((field) => !fields.has(field))) {
    throw new ActionError("Agent 策略 schema 或字段集合不受支持；不得推断发布后动作");
  }
  return { source, normalized, newline, lines, fields, schema };
}

function readPolicy(root) {
  const file = path.join(root, POLICY_RELATIVE);
  const stat = regularFile(file, "Agent 策略");
  const bytes = fs.readFileSync(file);
  const document = parseAgentPolicyDocument(bytes);
  const { fields, schema } = document;
  if (fields.get("decision_mode") !== "reuse_then_infer_then_ask") throw new ActionError("Agent 策略 decision_mode 无效");
  for (const field of PREFERENCES) if (!["enabled", "disabled"].includes(fields.get(field))) throw new ActionError(`下游 Agent 策略 ${field} 尚未确认`);
  const confirmedBy = fields.get("confirmed_by")?.trim().toLowerCase();
  if (!confirmedBy || ["pending", "unknown", "unset", "n/a"].includes(confirmedBy)) throw new ActionError("下游 Agent 策略 confirmed_by 尚未确认");
  if (!validConfirmedAt(fields.get("confirmed_at") ?? "")) throw new ActionError("下游 Agent 策略 confirmed_at 必须是真实 ISO 日期或时间戳");
  if (schema === "4" && !ACTIONS.has(fields.get("post_release_action"))) throw new ActionError("post_release_action 必须是 local_package 或 push_release_branch");
  const policy = { file, stat, bytes, ...document };
  if (schema === "4") assertCurrentPolicyBody(policy);
  if (schema === "4" && fields.get("post_release_action") === "local_package") assertLocalPackageSupported(root);
  return policy;
}

/** 判断策略正文是否已合并当前发布后动作规则；模板源与下游共用同一判定。 */
export function policyBodyIsCurrent(body) {
  return policyBodyProblems(body).length === 0;
}

/** 给受保护正文迁移提供可定位的缺失和过时片段。 */
export function policyBodyProblems(body) {
  return [
    ...REQUIRED_BODY.filter((fragment) => !body.includes(fragment)).map((fragment) => `缺少：${fragment}`),
    ...STALE_BODY.filter((fragment) => body.includes(fragment)).map((fragment) => `过时：${fragment}`),
  ];
}

function assertCurrentPolicyBody(policy) {
  const body = policy.normalized.slice(policy.normalized.indexOf("\n---\n", 4) + 5);
  const problems = policyBodyProblems(body);
  if (problems.length > 0) {
    throw new ActionError(`Agent 策略正文尚未合并发布后动作新规则；先保留本地自定义内容并完成受保护正文迁移。${problems.join("；")}`);
  }
}

/** 按发布运行时接受的 Cargo metadata 语法读取必需的接口或平台数组。 */
export function parseReleaseMetadataArray(source, key) {
  const lines = source.split(/\r?\n/u);
  const sections = lines.flatMap((line, index) => line.trim() === "[workspace.metadata.agent-first-harness]" ? [index] : []);
  if (sections.length !== 1) throw new ActionError("Cargo 发布接口元数据缺失或重复");
  const start = sections[0];
  const endOffset = lines.slice(start + 1).findIndex((line) => /^\[/u.test(line.trim()));
  const section = lines.slice(start + 1, endOffset < 0 ? undefined : start + 1 + endOffset);
  const matches = section.map((line) => new RegExp(`^${key}\\s*=\\s*(\\[[^\\]]*\\])\\s*$`, "u").exec(line)).filter(Boolean);
  if (matches.length !== 1) throw new ActionError(`Cargo ${key} 元数据缺失或重复`);
  let values;
  try { values = JSON.parse(matches[0][1]); } catch { throw new ActionError(`Cargo ${key} 元数据无效`); }
  if (!Array.isArray(values) || values.length === 0 || values.some((value) => typeof value !== "string")) {
    throw new ActionError(`Cargo ${key} 元数据无效`);
  }
  const allowed = key === "interfaces" ? ["cli", "tui", "mcp", "gui"] : ["windows", "macos", "linux"];
  if (new Set(values).size !== values.length || values.some((value) => !allowed.includes(value))) {
    throw new ActionError(`Cargo ${key} 元数据无效`);
  }
  return values;
}

function metadataArray(root, key) {
  const file = path.join(root, "Cargo.toml");
  regularFile(file, "Cargo 工作区清单");
  return parseReleaseMetadataArray(fs.readFileSync(file, "utf8"), key);
}

/** 按字符串、注释及容器边界读取语句，避免说明文字伪装成桌面框架事实。 */
function releaseCargoStatements(source) {
  const statements = [];
  const brackets = [];
  let current = "";
  let quote = null;
  let triple = false;
  let escaped = false;
  for (let index = 0; index < source.length; index += 1) {
    const character = source[index];
    if (quote !== null) {
      current += character;
      if (quote === '"' && escaped) escaped = false;
      else if (quote === '"' && character === "\\") escaped = true;
      else if (character === quote && (!triple || source.slice(index, index + 3) === quote.repeat(3))) {
        if (triple) { current += quote.repeat(2); index += 2; }
        quote = null;
      }
    } else if (character === '"' || character === "'") {
      quote = character;
      triple = source.slice(index, index + 3) === character.repeat(3);
      current += triple ? character.repeat(3) : character;
      if (triple) index += 2;
    } else if (character === "#") {
      while (index + 1 < source.length && source[index + 1] !== "\n") index += 1;
    } else if (character === "\n" && brackets.length === 0) {
      if (current.trim()) statements.push(current.trim());
      current = "";
    } else {
      if (character === "[" || character === "{") brackets.push(character);
      if ((character === "]" || character === "}") && brackets.pop() !== (character === "]" ? "[" : "{")) {
        throw new ActionError("Cargo gui-framework 解析遇到不匹配的容器边界");
      }
      current += character;
    }
  }
  if (quote !== null || brackets.length > 0) throw new ActionError("Cargo gui-framework 解析遇到未结束的字符串或容器");
  if (current.trim()) statements.push(current.trim());
  return statements;
}

/** 仅接受裸键、单行引号键和点分路径；无法确定的键语法失败关闭。 */
function releaseCargoKey(value) {
  const parts = [];
  let remaining = value.trim();
  while (remaining) {
    const match = /^(?:([A-Za-z0-9_-]+)|'([^'\r\n]*)'|("(?:[^"\\\r\n]|\\.)*"))\s*/u.exec(remaining);
    if (!match) throw new ActionError("Cargo gui-framework 解析遇到不支持的键语法");
    try { parts.push(match[1] ?? match[2] ?? JSON.parse(match[3])); }
    catch { throw new ActionError("Cargo gui-framework 解析遇到不支持的引号键转义"); }
    remaining = remaining.slice(match[0].length);
    if (!remaining) return { parts, value: null };
    if (remaining.startsWith("=")) return { parts, value: remaining.slice(1).trim() };
    if (!remaining.startsWith(".")) throw new ActionError("Cargo gui-framework 解析遇到不支持的键语法");
    remaining = remaining.slice(1).trimStart();
  }
  throw new ActionError("Cargo gui-framework 解析遇到空键或残缺点分路径");
}

/** 只缺省兼容 Tauri；引号键等价，表化、错位、重复及未知受管语法不得猜测。 */
export function parseReleaseGuiFramework(source) {
  const metadataPath = ["workspace", "metadata", "agent-first-harness"];
  const frameworkPath = [...metadataPath, "gui-framework"];
  let section = [];
  let metadataSections = 0;
  let framework;
  for (const statement of releaseCargoStatements(source)) {
    if (statement.startsWith("[")) {
      const array = statement.startsWith("[[");
      if (!statement.endsWith(array ? "]]" : "]")) throw new ActionError("Cargo gui-framework 表语法无效");
      const parsed = releaseCargoKey(statement.slice(array ? 2 : 1, array ? -2 : -1));
      if (parsed.value !== null) throw new ActionError("Cargo gui-framework 表语法无效");
      section = parsed.parts;
      if (section.includes("gui-framework")) throw new ActionError("Cargo gui-framework 必须是值，不能是表");
      if (section.length === metadataPath.length && section.every((part, index) => part === metadataPath[index])) {
        if (array || ++metadataSections > 1) throw new ActionError("Cargo 发布接口元数据缺失或重复");
      }
      continue;
    }
    const { parts, value } = releaseCargoKey(statement);
    if (value === null) throw new ActionError("Cargo gui-framework 解析遇到缺少赋值的字段");
    const fullPath = [...section, ...parts];
    if (parts.includes("gui-framework")) {
      if (fullPath.length !== frameworkPath.length || !fullPath.every((part, index) => part === frameworkPath[index])) {
        throw new ActionError("Cargo gui-framework 必须位于 workspace.metadata.agent-first-harness");
      }
      if (framework !== undefined) throw new ActionError("Cargo gui-framework 元数据重复");
      try { framework = /^'[^'\r\n]*'$/u.test(value) ? value.slice(1, -1) : JSON.parse(value); }
      catch { throw new ActionError("Cargo gui-framework 元数据无效"); }
      if (!["tauri", "gpui"].includes(framework)) throw new ActionError("Cargo gui-framework 元数据无效");
    } else if (fullPath.length <= metadataPath.length && fullPath.every((part, index) => part === metadataPath[index])) {
      throw new ActionError("Cargo gui-framework 所属元数据必须使用独立表，不能使用内联值");
    }
  }
  if (metadataSections !== 1) throw new ActionError("Cargo 发布接口元数据缺失或重复");
  return framework ?? "tauri";
}

function optionalMetadata(root, key) {
  const lines = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8").split(/\r?\n/u);
  const start = lines.findIndex((line) => line.trim() === "[workspace.metadata.agent-first-harness]");
  if (start < 0) throw new ActionError("Cargo 发布接口元数据缺失");
  const end = lines.findIndex((line, index) => index > start && /^\s*\[/u.test(line));
  const section = lines.slice(start + 1, end < 0 ? undefined : end);
  const matches = section.flatMap((line) => {
    const match = new RegExp(`^\\s*${key}\\s*=\\s*(.+?)\\s*$`, "u").exec(line);
    return match ? [match[1]] : [];
  });
  if (matches.length === 0) return null;
  if (matches.length !== 1) throw new ActionError(`Cargo ${key} 元数据重复`);
  try { return JSON.parse(matches[0]); }
  catch { throw new ActionError(`Cargo ${key} 元数据无效`); }
}

function safeProjectRelative(value, label, { allowRoot = false } = {}) {
  if (allowRoot && value === ".") return ".";
  if (typeof value !== "string" || !value || value.includes("\\") || value.includes(":") || path.isAbsolute(value)
      || value.split("/").some((segment) => !segment || segment === "." || segment === "..")) {
    throw new ActionError(`${label} 必须是项目内规范相对路径`);
  }
  return value;
}

function checkedProjectPath(root, relative, label, directory = false) {
  let cursor = root;
  for (const segment of relative === "." ? [] : relative.split("/")) {
    cursor = path.join(cursor, segment);
    let stat;
    try { stat = fs.lstatSync(cursor); }
    catch { throw new ActionError(`${label} 不存在：${relative}`); }
    if (stat.isSymbolicLink()) throw new ActionError(`${label} 不得是符号链接：${relative}`);
  }
  const stat = fs.lstatSync(cursor);
  if (directory ? !stat.isDirectory() : !stat.isFile()) throw new ActionError(`${label} 类型不正确：${relative}`);
  return cursor;
}

/** 要求清单的精确项目相对路径已进入 Git 索引，避免同名通配路径误判。 */
function assertTrackedFile(root, relative, label) {
  const tracked = spawnSync("git", ["-C", root, "ls-files", "--cached", "--full-name", "-z", "--", `:(literal)${relative}`], { encoding: "utf8" });
  if (tracked.status !== 0 || tracked.stdout !== `${relative}\0`) {
    throw new ActionError(`${label} 必须受 Git 跟踪：${relative}`);
  }
}

/** 按唯一的 Tauri 配置定位与之配对的 GUI Cargo 清单。 */
function guiTauriLayout(root, guiRelative) {
  const layouts = [
    { cargo: path.posix.join(guiRelative, "Cargo.toml"), config: path.posix.join(guiRelative, "tauri.conf.json") },
    { cargo: path.posix.join(guiRelative, "src-tauri/Cargo.toml"), config: path.posix.join(guiRelative, "src-tauri/tauri.conf.json") },
  ];
  const present = layouts.filter(({ config }) => {
    try { fs.lstatSync(path.join(root, config)); return true; }
    catch (error) { if (error.code === "ENOENT") return false; throw error; }
  });
  if (present.length !== 1) throw new ActionError("Tauri 配置必须在 GUI 根目录或 src-tauri 中且只能存在一份");
  const selected = present[0];
  checkedProjectPath(root, selected.config, "Tauri 配置");
  checkedProjectPath(root, selected.cargo, "GUI Rust Cargo 清单");
  return selected;
}

function assertNonEmptyRustWorkspace(file, relative) {
  const source = fs.readFileSync(file, "utf8");
  const workspace = /^\s*\[workspace\]\s*$/mu.exec(source);
  if (!workspace || /^\s*\[package\]\s*$/mu.test(source)) return;
  const rest = source.slice(workspace.index + workspace[0].length);
  const nextSection = /^\s*\[/mu.exec(rest);
  const section = rest.slice(0, nextSection?.index);
  if (!/^\s*members\s*=/mu.test(section) || /^\s*members\s*=\s*\[\s*\]\s*(?:#.*)?$/mu.test(section)) {
    throw new ActionError(`Rust 测试清单是空 workspace，须声明实际测试清单：${relative}`);
  }
}

/** 两种 GUI 框架共用非空且精确受跟踪的 Rust 测试清单边界。 */
function checkedRustTestManifests(root) {
  const manifests = optionalMetadata(root, "rust-test-manifests") ?? ["Cargo.toml"];
  if (!Array.isArray(manifests) || manifests.length === 0 || new Set(manifests).size !== manifests.length) {
    throw new ActionError("rust-test-manifests 必须是非空且不重复的路径数组");
  }
  return manifests.map((manifest) => {
    const relative = safeProjectRelative(manifest, "rust-test-manifests");
    if (path.posix.basename(relative) !== "Cargo.toml") throw new ActionError("rust-test-manifests 只能指向 Cargo.toml");
    const manifestPath = checkedProjectPath(root, relative, "Rust 测试清单");
    assertTrackedFile(root, relative, "Rust 测试清单");
    assertNonEmptyRustWorkspace(manifestPath, relative);
    return relative;
  });
}

function assertGuiPackageReady(root) {
  const relative = safeProjectRelative(optionalMetadata(root, "gui-root") ?? `${path.basename(root)}_gui`, "gui-root", { allowRoot: true });
  const gui = checkedProjectPath(root, relative, "GUI 根目录", true);
  const guiFile = (name, label) => checkedProjectPath(root, path.posix.join(relative, name), label);
  const packageFile = guiFile("package.json", "GUI package.json");
  const layout = guiTauriLayout(root, relative);
  assertTrackedFile(root, "Cargo.toml", "Cargo 工作区清单");
  assertTrackedFile(root, path.posix.join(relative, "package.json"), "GUI package.json");
  assertTrackedFile(root, layout.cargo, "GUI Rust Cargo 清单");
  assertTrackedFile(root, layout.config, "Tauri 配置");
  let packageJson;
  try { packageJson = JSON.parse(fs.readFileSync(packageFile, "utf8")); }
  catch { throw new ActionError("GUI package.json 必须是有效 JSON"); }
  if (!packageJson?.dependencies?.["@tauri-apps/cli"] && !packageJson?.devDependencies?.["@tauri-apps/cli"]) {
    throw new ActionError("GUI package.json 缺少项目本地 @tauri-apps/cli");
  }
  const manifests = checkedRustTestManifests(root);
  const lockManifests = [...new Set(["Cargo.toml", ...manifests, layout.cargo])];
  assertDependencyLocks(root, { rustTestManifests: lockManifests, guiRoot: relative });
  if (relative !== "." && !gui.startsWith(`${root}${path.sep}`)) throw new ActionError("GUI 根目录越出项目");
}

/** 核对 GPUI 原生打包所用 Rust 清单；不要求 Tauri、前端或 pnpm 文件。 */
function assertGpuiPackageReady(root) {
  const configuredRoot = optionalMetadata(root, "gui-root");
  const projectId = optionalMetadata(root, "project-id");
  if (projectId !== null && (typeof projectId !== "string" || !/^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$/u.test(projectId))) {
    throw new ActionError("Cargo project-id 必须是 ASCII snake_case 标识");
  }
  const relative = safeProjectRelative(configuredRoot ?? `${projectId ?? path.basename(root)}_gui`, "gui-root", { allowRoot: true });
  checkedProjectPath(root, relative, "GPUI GUI 根目录", true);
  const guiManifest = path.posix.join(relative, "Cargo.toml");
  const guiFile = checkedProjectPath(root, guiManifest, "GPUI Cargo 清单");
  if (!/^\s*\[package\]\s*$/mu.test(fs.readFileSync(guiFile, "utf8"))) throw new ActionError("GPUI Cargo 清单必须声明原生 GUI package");
  assertTrackedFile(root, "Cargo.toml", "Cargo 工作区清单");
  assertTrackedFile(root, guiManifest, "GPUI Cargo 清单");
  const manifests = checkedRustTestManifests(root);
  assertDependencyLocks(root, { rustTestManifests: [...new Set(["Cargo.toml", ...manifests, guiManifest])] });
}

/** 判断接口、框架与平台是否有适用的 CLI、Tauri 或 GPUI 本地打包 Skill。 */
export function localPackageSupported(interfaces, platforms, guiFramework = "tauri") {
  if (!["tauri", "gpui"].includes(guiFramework)) throw new ActionError("Cargo gui-framework 元数据无效");
  return interfaces.includes("cli") || (interfaces.includes("gui") && platforms.some((platform) => ["macos", "windows"].includes(platform)));
}

function assertLocalPackageSupported(root) {
  const interfaces = metadataArray(root, "interfaces");
  const framework = parseReleaseGuiFramework(fs.readFileSync(path.join(root, "Cargo.toml"), "utf8"));
  if (!localPackageSupported(interfaces, metadataArray(root, "target-platforms"), framework)) {
    throw new ActionError("当前接口/目标平台没有现有本地打包 Skill；请选择 push_release_branch");
  }
  if (interfaces.includes("gui") && framework === "tauri") assertGuiPackageReady(root);
  else if (interfaces.includes("gui") && framework === "gpui") assertGpuiPackageReady(root);
  else if (interfaces.includes("cli")) assertDependencyLocks(root, { rustTestManifests: ["Cargo.toml"] });
}

function state(policy) {
  const action = policy.schema === "4" ? policy.fields.get("post_release_action") : null;
  return { schema_version: Number(policy.schema), status: action === null ? "selection_required" : "configured", post_release_action: action };
}

function atomicWrite(policy, next) {
  const temporary = path.join(path.dirname(policy.file), `.AGENT_POLICY-${crypto.randomUUID()}.tmp`);
  let descriptor;
  try {
    descriptor = fs.openSync(temporary, "wx", policy.stat.mode & 0o777);
    fs.writeFileSync(descriptor, next, "utf8");
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor); descriptor = undefined;
    const currentStat = regularFile(policy.file, "Agent 策略");
    if (currentStat.dev !== policy.stat.dev || currentStat.ino !== policy.stat.ino || !fs.readFileSync(policy.file).equals(policy.bytes)) {
      throw new ActionError("Agent 策略在读取后变化；请重新检查选择");
    }
    fs.renameSync(temporary, policy.file);
    try { const parent = fs.openSync(path.dirname(policy.file), fs.constants.O_RDONLY); fs.fsyncSync(parent); fs.closeSync(parent); } catch { /* 部分平台不能同步目录 */ }
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    try { fs.rmSync(temporary, { force: true }); } catch { /* 写入错误按原异常报告 */ }
  }
}

function withPolicyLock(root, operation) {
  const lock = path.join(root, "docs", ".AGENT_POLICY.post-release.lock");
  let descriptor;
  try { descriptor = fs.openSync(lock, "wx", 0o600); }
  catch { throw new ActionError("Agent 策略正在被修改，或上次修改留下锁；不得并发覆盖"); }
  try { return operation(); }
  finally {
    fs.closeSync(descriptor);
    fs.rmSync(lock, { force: true });
  }
}

export function inspect(rawRoot, { requireConfigured = false } = {}) {
  const result = state(readPolicy(projectRoot(rawRoot)));
  if (requireConfigured && result.status !== "configured") throw new ActionError("旧项目尚未选择 post_release_action；发布后流程不得标为完成");
  return result;
}

export function setAction(rawRoot, action, expectedAction, { confirmedChoice = false } = {}) {
  if (!confirmedChoice) throw new ActionError("写入要求 --confirmed-user-choice；先取得用户本次明确选择");
  if (!ACTIONS.has(action)) throw new ActionError("--action 必须是 local_package 或 push_release_branch");
  if (!["missing", ...ACTIONS].includes(expectedAction)) throw new ActionError("--expected-action 必须是 missing 或当前合法动作");
  const root = projectRoot(rawRoot);
  return withPolicyLock(root, () => {
    const policy = readPolicy(root);
    const previous = policy.schema === "4" ? policy.fields.get("post_release_action") : "missing";
    if (previous !== expectedAction) throw new ActionError(`发布后动作已变化：预期 ${expectedAction}，实际 ${previous}`);
    if (previous === action) return { ...state(policy), changed: false, previous_action: previous };
    assertCurrentPolicyBody(policy);
    if (action === "local_package") assertLocalPackageSupported(root);
    const lines = [...policy.lines];
    if (policy.schema === "3") {
      lines[lines.findIndex((line) => line.startsWith("schema_version:"))] = "schema_version: 4";
      lines.push(`post_release_action: ${action}`);
    } else {
      lines[lines.findIndex((line) => line.startsWith("post_release_action:"))] = `post_release_action: ${action}`;
    }
    const body = policy.normalized.slice(policy.normalized.indexOf("\n---\n", 4) + 5);
    const next = `---\n${lines.join("\n")}\n---\n${body}`.replaceAll("\n", policy.newline);
    atomicWrite(policy, next);
    const verified = inspect(root, { requireConfigured: true });
    if (verified.post_release_action !== action || verified.schema_version !== 4) throw new ActionError("写入后复核发布后动作失败");
    return { ...verified, changed: true, previous_action: previous };
  });
}

function parseArgs(argv) {
  const [command, ...rest] = argv;
  if (!["inspect", "check", "set"].includes(command)) throw new ActionError("命令只能是 inspect、check 或 set");
  const allowed = command === "set" ? new Set(["--project-root", "--action", "--expected-action", "--confirmed-user-choice"]) : new Set(["--project-root"]);
  const values = new Map();
  for (let index = 0; index < rest.length; index += 1) {
    const key = rest[index];
    if (!allowed.has(key) || values.has(key)) throw new ActionError(`未知或重复参数：${key}`);
    if (key === "--confirmed-user-choice") values.set(key, true);
    else {
      const value = rest[++index];
      if (!value || value.startsWith("--")) throw new ActionError(`参数 ${key} 缺少值`);
      values.set(key, value);
    }
  }
  if (!values.has("--project-root")) throw new ActionError("缺少 --project-root");
  if (command === "set" && (!values.has("--action") || !values.has("--expected-action"))) throw new ActionError("set 需要 --action 与 --expected-action");
  return { command, values };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { command, values } = parseArgs(process.argv.slice(2));
    const root = values.get("--project-root");
    const result = command === "set"
      ? setAction(root, values.get("--action"), values.get("--expected-action"), { confirmedChoice: values.has("--confirmed-user-choice") })
      : inspect(root, { requireConfigured: command === "check" });
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ error: error instanceof Error ? error.message : String(error) })}\n`);
    process.exitCode = 2;
  }
}
