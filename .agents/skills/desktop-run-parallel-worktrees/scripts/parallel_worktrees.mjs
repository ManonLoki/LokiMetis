#!/usr/bin/env node
/** 安全创建、检查、验证和移除 Harness 并行协作使用的 Git Worktree。 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const LIFECYCLE_SCRIPT_RELATIVE = path.join(".agents", "skills", "desktop-manage-git-lifecycle", "scripts", "git_lifecycle.mjs");
const IDENTIFIER = /^[a-z0-9][a-z0-9_-]{0,63}$/;
const OBJECT_ID = /^[0-9a-f]{40,64}$/;
const STATE_SCHEMA_VERSION = 1;

export class WorkflowError extends Error {
  constructor(code, message, exitCode = 2) { super(message); this.code = code; this.exitCode = exitCode; }
}

function expandHome(value) {
  if (value === "~") return os.homedir();
  if (value.startsWith(`~${path.sep}`)) return path.join(os.homedir(), value.slice(2));
  return value;
}

function lstatOrNull(value) {
  try { return fs.lstatSync(value); } catch (error) { if (error?.code === "ENOENT") return null; throw error; }
}

function realpathAllowMissing(value) {
  const absolute = path.resolve(value);
  let existing = absolute;
  while (!lstatOrNull(existing)) {
    const parent = path.dirname(existing);
    if (parent === existing) break;
    existing = parent;
  }
  return path.join(fs.realpathSync(existing), path.relative(existing, absolute));
}

/** 在指定 Git 工作树执行命令，并把失败转换为稳定错误。 */
export function runGit(root, ...arguments_) {
  let check = true;
  if (typeof arguments_.at(-1) === "object") ({ check = true } = arguments_.pop());
  const result = spawnSync("git", ["-C", root, ...arguments_], { encoding: null });
  if (result.error) throw new WorkflowError("git_failed", result.error.message, 3);
  let stdout;
  let stderr;
  try {
    const decoder = new TextDecoder("utf-8", { fatal: true });
    stdout = decoder.decode(result.stdout ?? Buffer.alloc(0));
    stderr = decoder.decode(result.stderr ?? Buffer.alloc(0));
  } catch {
    throw new WorkflowError("git_output_invalid", "Git 返回了非 UTF-8 输出", 3);
  }
  const completed = { returncode: result.status ?? 1, stdout, stderr };
  if (check && completed.returncode !== 0) throw new WorkflowError("git_failed", completed.stderr.trim() || completed.stdout.trim() || "Git 命令失败", 3);
  return completed;
}

/** 解析显式目录，并要求它自身就是 Git 顶层目录。 */
export function gitTopLevel(rawRoot, label, missingCode) {
  const root = realpathAllowMissing(expandHome(rawRoot));
  const stat = lstatOrNull(root);
  if (!stat?.isDirectory()) throw new WorkflowError(missingCode, `${label}不是目录：${root}`);
  const top = fs.realpathSync(runGit(root, "rev-parse", "--show-toplevel").stdout.trim());
  if (top !== root) throw new WorkflowError(missingCode.replace(/_missing$/u, "") + "_not_git_top_level", `${label}${root} 继承了 Git 顶层目录 ${top}`);
  return root;
}

export function repositoryCommonDir(worktree) {
  const raw = runGit(worktree, "rev-parse", "--git-common-dir").stdout.trim();
  return fs.realpathSync(path.isAbsolute(raw) ? raw : path.join(worktree, raw));
}

export function parsedWorktrees(projectRoot) {
  const records = [];
  let current = {};
  for (const line of runGit(projectRoot, "worktree", "list", "--porcelain").stdout.split(/\r?\n/)) {
    if (!line) { if (Object.keys(current).length) { records.push(current); current = {}; } continue; }
    const separator = line.indexOf(" ");
    const key = separator < 0 ? line : line.slice(0, separator);
    current[key] = separator < 0 ? true : line.slice(separator + 1);
  }
  if (Object.keys(current).length) records.push(current);
  return records;
}

export function recordForWorktree(projectRoot, worktree) {
  for (const record of parsedWorktrees(projectRoot)) if (typeof record.worktree === "string" && realpathAllowMissing(record.worktree) === worktree) return record;
  return null;
}

/** 要求显式保存项目根就是该仓库的 primary Worktree。 */
export function canonicalProjectRoot(rawRoot) {
  const root = gitTopLevel(rawRoot, "保存项目根目录", "project_root_missing");
  const records = parsedWorktrees(root);
  if (typeof records[0]?.worktree !== "string" || fs.realpathSync(records[0].worktree) !== root) throw new WorkflowError("project_root_not_primary_worktree", `保存项目根 ${root} 不是该仓库登记的 primary Worktree`, 4);
  return { projectRoot: root, commonDir: repositoryCommonDir(root) };
}

export function validateIdentifier(label, value) {
  if (!IDENTIFIER.test(value)) throw new WorkflowError("invalid_identifier", `${label}必须匹配 ${IDENTIFIER.source}：${JSON.stringify(value)}`);
  return value;
}

/** 绑定同一仓库内已登记且附着在任意具名分支的 Task source。 */
export function canonicalSourceContext(projectRoot, commonDir, rawSource) {
  const sourceWorktree = gitTopLevel(rawSource, "源 Worktree ", "source_worktree_missing");
  if (repositoryCommonDir(sourceWorktree) !== commonDir) throw new WorkflowError("source_repository_mismatch", `源 Worktree ${sourceWorktree} 不属于保存项目 ${projectRoot} 的同一 Git 仓库`, 4);
  const record = recordForWorktree(projectRoot, sourceWorktree);
  if (!record) throw new WorkflowError("source_worktree_not_registered", `源 Worktree 未登记在保存项目仓库中：${sourceWorktree}`, 4);
  const branchResult = runGit(sourceWorktree, "symbolic-ref", "--quiet", "--short", "HEAD", { check: false });
  const sourceBranch = branchResult.returncode === 0 ? branchResult.stdout.trim() : "";
  if (!sourceBranch) throw new WorkflowError("source_branch_unavailable", "源 Worktree 必须附着在具名分支，不能使用 HEAD 分离状态", 4);
  if (record.branch !== `refs/heads/${sourceBranch}`) throw new WorkflowError("source_branch_registry_mismatch", `源 Worktree 登记分支 ${JSON.stringify(record.branch)} 与当前具名分支 ${JSON.stringify(sourceBranch)} 不一致`, 4);
  return { projectRoot, commonDir, sourceWorktree, sourceBranch };
}

export function requireExactCwd(expected, code, context) {
  const actual = fs.realpathSync(process.cwd());
  if (actual !== expected) throw new WorkflowError(code, `${context}必须从 ${expected} 运行，实际 cwd 为 ${actual}`, 4);
  return actual;
}

export function requirePlainDirectory(value, code, label, create = false) {
  let stat = lstatOrNull(value);
  if (stat?.isSymbolicLink()) throw new WorkflowError(code, `${label}不得是符号链接：${value}`, 4);
  if (stat && !stat.isDirectory()) throw new WorkflowError(code, `${label}不是目录：${value}`, 4);
  if (create && !stat) { fs.mkdirSync(value); stat = fs.lstatSync(value); if (stat.isSymbolicLink() || !stat.isDirectory()) throw new WorkflowError(code, `${label}未建立为普通目录：${value}`, 4); }
}

export function managedWorktreeRoot(projectRoot, create = false) {
  const anchor = path.join(path.dirname(projectRoot), ".codex-worktrees");
  const projectContainer = path.join(anchor, path.basename(projectRoot));
  requirePlainDirectory(anchor, "worktree_container_unsafe", "Worktree 总容器");
  if (create && !lstatOrNull(anchor)) requirePlainDirectory(anchor, "worktree_container_unsafe", "Worktree 总容器", true);
  requirePlainDirectory(projectContainer, "worktree_container_unsafe", "项目 Worktree 容器");
  if (create && !lstatOrNull(projectContainer)) requirePlainDirectory(projectContainer, "worktree_container_unsafe", "项目 Worktree 容器", true);
  return path.resolve(projectContainer);
}

export function stateRoot(commonDir, create = false) {
  const root = path.join(commonDir, "codex-parallel-worktrees");
  requirePlainDirectory(root, "unit_state_root_unsafe", "并行单元状态目录");
  if (create && !lstatOrNull(root)) requirePlainDirectory(root, "unit_state_root_unsafe", "并行单元状态目录", true);
  return root;
}

export function unitIdentity(context, task, unit, create = false) {
  const safeTask = validateIdentifier("任务", task);
  const safeUnit = validateIdentifier("单元", unit);
  const worktreeRoot = managedWorktreeRoot(context.projectRoot, create);
  const taskRoot = path.join(worktreeRoot, safeTask);
  requirePlainDirectory(taskRoot, "worktree_container_unsafe", "Task Worktree 容器");
  if (create && !lstatOrNull(taskRoot)) requirePlainDirectory(taskRoot, "worktree_container_unsafe", "Task Worktree 容器", true);
  const states = stateRoot(context.commonDir, create);
  const taskStates = path.join(states, safeTask);
  requirePlainDirectory(taskStates, "unit_state_root_unsafe", "Task 状态目录");
  if (create && !lstatOrNull(taskStates)) requirePlainDirectory(taskStates, "unit_state_root_unsafe", "Task 状态目录", true);
  return { context, task: safeTask, unit: safeUnit, branch: `codex/unit-${safeTask}-${safeUnit}`, worktreePath: path.join(taskRoot, safeUnit), statePath: path.join(taskStates, `${safeUnit}.json`) };
}

/** 把用户路径限制为 forward-slash repo-relative 路径并防止 symlink 逃逸。 */
export function normalizeRepoRelative(raw, worktree, code) {
  if (!raw || raw.includes("\\") || raw.includes("\0")) throw new WorkflowError(code, `路径必须是非空 forward-slash repo-relative 路径：${JSON.stringify(raw)}`, 4);
  if (raw.startsWith("/") || raw === "." || raw.split("/").some((part) => !part || part === "." || part === "..")) throw new WorkflowError(code, `路径不得为根、绝对路径或包含 . / ..：${JSON.stringify(raw)}`, 4);
  const resolvedRoot = fs.realpathSync(worktree);
  const resolved = realpathAllowMissing(path.join(worktree, ...raw.split("/")));
  if (resolved !== resolvedRoot && !resolved.startsWith(`${resolvedRoot}${path.sep}`)) throw new WorkflowError(code, `路径 ${JSON.stringify(raw)} 经解析后越出 Worktree ${resolvedRoot}`, 4);
  return raw;
}

export function pathContains(owner, candidate) { return candidate === owner || candidate.startsWith(`${owner}/`); }
export function ownershipOverlaps(left, right) { return pathContains(left, right) || pathContains(right, left); }

export function normalizeOwnership(rawTargets, source) {
  if (!rawTargets.length) throw new WorkflowError("ownership_required", "create 至少需要一个 --write-target", 4);
  const normalized = [...new Set(rawTargets.map((raw) => normalizeRepoRelative(raw, source, "ownership_path_invalid")))].sort();
  for (let index = 0; index < normalized.length; index += 1) for (const right of normalized.slice(index + 1)) if (ownershipOverlaps(normalized[index], right)) throw new WorkflowError("ownership_overlap_within_unit", `同一单元所有权不得重叠：${normalized[index]} 与 ${right}`, 4);
  return normalized;
}

export function statePayload(state) {
  const { identity } = state;
  const { context } = identity;
  return { schemaVersion: STATE_SCHEMA_VERSION, projectRoot: context.projectRoot, commonDir: context.commonDir, sourceWorktree: context.sourceWorktree, sourceBranch: context.sourceBranch, task: identity.task, unit: identity.unit, branch: identity.branch, worktreePath: identity.worktreePath, baseHead: state.baseHead, ownership: [...state.ownership] };
}

export function readStateDocument(statePath) {
  const stat = lstatOrNull(statePath);
  if (!stat?.isFile() || stat.isSymbolicLink()) throw new WorkflowError("unit_state_missing", `单元状态不存在或不是普通文件：${statePath}`, 4);
  let payload;
  try { payload = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(fs.readFileSync(statePath))); } catch (error) { throw new WorkflowError("unit_state_invalid", `无法读取单元状态 ${statePath}：${error.message}`, 4); }
  if (!payload || Array.isArray(payload) || typeof payload !== "object" || payload.schemaVersion !== STATE_SCHEMA_VERSION) throw new WorkflowError("unit_state_invalid", `单元状态 schema 无效：${statePath}`, 4);
  return payload;
}

export function loadUnitState(identity) {
  const payload = readStateDocument(identity.statePath);
  const expected = { projectRoot: identity.context.projectRoot, commonDir: identity.context.commonDir, sourceWorktree: identity.context.sourceWorktree, sourceBranch: identity.context.sourceBranch, task: identity.task, unit: identity.unit, branch: identity.branch, worktreePath: identity.worktreePath };
  for (const [key, value] of Object.entries(expected)) if (payload[key] !== value) throw new WorkflowError("unit_state_identity_mismatch", `单元状态字段 ${key} 为 ${JSON.stringify(payload[key])}，预期为 ${JSON.stringify(value)}`, 4);
  if (typeof payload.baseHead !== "string" || !OBJECT_ID.test(payload.baseHead)) throw new WorkflowError("unit_state_invalid", "单元状态 baseHead 无效", 4);
  if (!Array.isArray(payload.ownership) || !payload.ownership.length || !payload.ownership.every((item) => typeof item === "string")) throw new WorkflowError("unit_state_invalid", "单元状态 ownership 无效", 4);
  const ownership = payload.ownership.map((item) => normalizeRepoRelative(item, identity.worktreePath, "unit_state_invalid"));
  if (new Set(ownership).size !== ownership.length) throw new WorkflowError("unit_state_invalid", "单元状态 ownership 包含重复路径", 4);
  return { identity, baseHead: payload.baseHead, ownership };
}

function withTaskStateLock(identity, callback) {
  const lock = path.join(path.dirname(identity.statePath), ".lock");
  try { fs.mkdirSync(lock); } catch (error) { if (error?.code === "EEXIST") throw new WorkflowError("task_state_locked", `Task 状态正由另一操作持有，或存在需人工检查的遗留锁：${lock}`, 4); throw error; }
  try { return callback(); } finally { try { fs.rmdirSync(lock); } catch { /* 锁损坏由后续操作可见 */ } }
}

export function rejectRegisteredOwnershipOverlap(state) {
  const directory = path.dirname(state.identity.statePath);
  for (const name of fs.readdirSync(directory).filter((entry) => entry.endsWith(".json")).sort()) {
    const statePath = path.join(directory, name);
    if (statePath === state.identity.statePath) throw new WorkflowError("unit_state_exists", `单元状态已存在：${statePath}`, 4);
    const payload = readStateDocument(statePath);
    if (!Array.isArray(payload.ownership) || !payload.ownership.every((item) => typeof item === "string")) throw new WorkflowError("unit_state_invalid", `单元状态 ownership 无效：${statePath}`, 4);
    for (const left of state.ownership) for (const right of payload.ownership) if (ownershipOverlaps(left, right)) throw new WorkflowError("ownership_overlap_across_units", `所有权 ${left} 与已有单元 ${path.basename(statePath, ".json")} 的 ${right} 重叠`, 4);
  }
}

export function writeUnitState(state) {
  try { fs.writeFileSync(state.identity.statePath, `${JSON.stringify(statePayload(state))}\n`, { encoding: "utf8", flag: "wx" }); }
  catch (error) { if (error?.code === "EEXIST") throw new WorkflowError("unit_state_exists", `单元状态已存在：${state.identity.statePath}`, 4); throw new WorkflowError("unit_state_write_failed", `无法写入单元状态：${error.message}`, 4); }
}

export function lifecycleScript(projectRoot) {
  const script = path.join(projectRoot, LIFECYCLE_SCRIPT_RELATIVE);
  const stat = lstatOrNull(script);
  if (!stat?.isFile() || stat.isSymbolicLink()) throw new WorkflowError("git_lifecycle_helper_missing", `Git 生命周期 helper 不存在或不是普通文件：${script}`, 4);
  return script;
}

export function trackLifecycleWorktree(state) {
  const { identity } = state;
  const script = lifecycleScript(identity.context.projectRoot);
  const result = spawnSync(process.execPath, [script, "track-worktree", "--project-root", identity.context.projectRoot, "--worktree", identity.worktreePath], { cwd: identity.context.projectRoot, encoding: "utf8", timeout: 60_000, input: "" });
  if (result.error) throw new WorkflowError("lifecycle_worktree_tracking_failed", `无法运行 Git 生命周期 helper：${result.error.message}`, 4);
  let payload = {};
  try { payload = JSON.parse(result.stdout || "{}"); } catch { payload = {}; }
  if (result.status !== 0) throw new WorkflowError("lifecycle_worktree_tracking_failed", `Git 生命周期 Worktree 登记失败（${typeof payload.code === "string" ? payload.code : "unknown"}）：${typeof payload.message === "string" ? payload.message : "Git 生命周期 helper 返回非零状态"}`, 4);
  return payload && !Array.isArray(payload) && typeof payload === "object" ? payload : {};
}

export function cleanupEmptyUnitContainers(identity) {
  const candidates = [path.dirname(identity.statePath), path.dirname(path.dirname(identity.statePath)), path.dirname(identity.worktreePath), path.dirname(path.dirname(identity.worktreePath)), path.dirname(path.dirname(path.dirname(identity.worktreePath)))];
  for (const directory of candidates) try { fs.rmdirSync(directory); } catch { /* 非空或不存在则保留 */ }
}

export function rollbackCreatedUnit(state) {
  const { identity } = state;
  const failures = [];
  if (lstatOrNull(identity.statePath)) try { fs.unlinkSync(identity.statePath); } catch (error) { failures.push(`无法删除单元状态：${error.message}`); }
  const worktree = runGit(identity.context.projectRoot, "worktree", "remove", identity.worktreePath, { check: false });
  if (worktree.returncode !== 0) failures.push(worktree.stderr.trim() || "无法移除新建 Worktree");
  const branch = runGit(identity.context.projectRoot, "branch", "-D", identity.branch, { check: false });
  if (branch.returncode !== 0) failures.push(branch.stderr.trim() || "无法删除新建单元分支");
  if (failures.length) throw new WorkflowError("unit_create_rollback_failed", failures.join("；"), 4);
}

export function inspectProject(projectRoot, commonDir) {
  const status = runGit(projectRoot, "status", "--porcelain=v1", "--untracked-files=all").stdout;
  const head = runGit(projectRoot, "rev-parse", "--verify", "HEAD", { check: false });
  const branch = runGit(projectRoot, "branch", "--show-current").stdout.trim();
  return { projectRoot, commonDir, clean: !status.trim(), branch: branch || null, head: head.returncode === 0 ? head.stdout.trim() : null, worktrees: parsedWorktrees(projectRoot) };
}

export function findExactUnitWorktree(identity) {
  const record = recordForWorktree(identity.context.projectRoot, realpathAllowMissing(identity.worktreePath));
  if (!record) throw new WorkflowError("worktree_not_found", `未找到受管 Worktree：${identity.worktreePath}`, 4);
  const expectedBranch = `refs/heads/${identity.branch}`;
  if (record.branch !== expectedBranch) throw new WorkflowError("worktree_branch_mismatch", `Worktree 路径属于 ${record.branch}，预期为 ${expectedBranch}`, 4);
  return record;
}

export function createUnit(identity, rawOwnership) {
  const source = identity.context.sourceWorktree;
  if (runGit(source, "status", "--porcelain=v1", "--untracked-files=all").stdout.trim()) throw new WorkflowError("source_worktree_dirty", "源 Task Worktree 存在已跟踪或未跟踪修改；不得自动贮藏或提交", 4);
  const head = runGit(source, "rev-parse", "--verify", "HEAD", { check: false });
  if (head.returncode !== 0) throw new WorkflowError("source_head_missing", "源 Task Worktree 没有已提交的 HEAD", 4);
  const state = { identity, baseHead: head.stdout.trim(), ownership: normalizeOwnership(rawOwnership, source) };
  let tracking = {};
  try {
    withTaskStateLock(identity, () => {
      rejectRegisteredOwnershipOverlap(state);
      if (runGit(identity.context.projectRoot, "show-ref", "--verify", "--quiet", `refs/heads/${identity.branch}`, { check: false }).returncode === 0) throw new WorkflowError("branch_exists", `分支已存在：${identity.branch}`, 4);
      if (lstatOrNull(identity.worktreePath)) throw new WorkflowError("worktree_path_exists", `Worktree 路径已存在：${identity.worktreePath}`, 4);
      runGit(source, "worktree", "add", "-b", identity.branch, identity.worktreePath, "HEAD");
      try { writeUnitState(state); tracking = trackLifecycleWorktree(state); }
      catch (error) { try { rollbackCreatedUnit(state); } catch (rollbackError) { throw new WorkflowError("unit_create_rollback_failed", `${error.message}；回滚失败：${rollbackError.message}`, 4); } throw error; }
    });
  } catch (error) { if (error instanceof WorkflowError) cleanupEmptyUnitContainers(identity); throw error; }
  return { created: true, ...statePayload(state), lifecycleTracking: tracking };
}

export function validateUnitContext(identity, requireUnitCwd) {
  if (requireUnitCwd) requireExactCwd(identity.worktreePath, "unit_cwd_mismatch", "单元操作");
  const actualRoot = fs.realpathSync(runGit(identity.worktreePath, "rev-parse", "--show-toplevel").stdout.trim());
  if (actualRoot !== identity.worktreePath) throw new WorkflowError("unit_git_root_mismatch", `单元 Git 顶层目录为 ${actualRoot}，预期为 ${identity.worktreePath}`, 4);
  const branch = runGit(identity.worktreePath, "symbolic-ref", "--quiet", "--short", "HEAD", { check: false });
  const actualBranch = branch.returncode === 0 ? branch.stdout.trim() : "";
  if (actualBranch !== identity.branch) throw new WorkflowError("unit_branch_mismatch", `单元分支为 ${actualBranch || "HEAD 分离状态"}，预期为 ${identity.branch}`, 4);
  findExactUnitWorktree(identity);
  return actualRoot;
}

export function parseNameStatus(raw) {
  const tokens = raw.split("\0"); if (tokens.at(-1) === "") tokens.pop();
  const paths = new Set(); let index = 0;
  while (index < tokens.length) {
    const status = tokens[index++];
    if (!status || index >= tokens.length) throw new WorkflowError("git_output_invalid", "无法解析 git diff --name-status 输出", 3);
    paths.add(tokens[index++]);
    if (["R", "C"].includes(status[0])) { if (index >= tokens.length) throw new WorkflowError("git_output_invalid", "重命名或复制记录缺少目标路径", 3); paths.add(tokens[index++]); }
  }
  return paths;
}

export function parsePorcelainStatus(raw) {
  const tokens = raw.split("\0"); if (tokens.at(-1) === "") tokens.pop();
  const paths = new Set(); let index = 0;
  while (index < tokens.length) {
    const record = tokens[index++];
    if (record.length < 4 || record[2] !== " ") throw new WorkflowError("git_output_invalid", "无法解析 git status porcelain 输出", 3);
    const status = record.slice(0, 2); paths.add(record.slice(3));
    if (["R", "C"].includes(status[0]) || ["R", "C"].includes(status[1])) { if (index >= tokens.length) throw new WorkflowError("git_output_invalid", "状态重命名记录缺少源路径", 3); paths.add(tokens[index++]); }
  }
  return paths;
}

export function normalizeGitPath(raw) {
  if (raw.startsWith("/") || raw === "." || raw.split("/").some((part) => !part || part === "." || part === "..")) throw new WorkflowError("git_output_invalid", `Git 返回了越界路径：${JSON.stringify(raw)}`, 3);
  return raw;
}

export function actualChangedPaths(state) {
  const worktree = state.identity.worktreePath;
  const committed = runGit(worktree, "diff", "--name-status", "-z", "--find-renames", `${state.baseHead}..HEAD`, "--").stdout;
  const pending = runGit(worktree, "status", "--porcelain=v1", "-z", "--untracked-files=all").stdout;
  return [...new Set([...parseNameStatus(committed), ...parsePorcelainStatus(pending)].map(normalizeGitPath))].sort();
}

export function verifyRegisteredChanges(state) {
  const changed = actualChangedPaths(state);
  for (const changedPath of changed) if (!state.ownership.some((owner) => pathContains(owner, changedPath))) throw new WorkflowError("actual_change_outside_ownership", `实际改动 ${changedPath} 不属于登记范围 ${JSON.stringify(state.ownership)}`, 4);
  return changed;
}

export function guardUnit(identity, rawTargets) {
  const state = loadUnitState(identity); validateUnitContext(identity, true); const changed = verifyRegisteredChanges(state);
  if (!rawTargets.length) throw new WorkflowError("write_target_required", "guard 至少需要一个 --write-target", 4);
  const targets = rawTargets.map((raw) => normalizeRepoRelative(raw, identity.worktreePath, "write_target_outside_worktree"));
  for (const target of targets) if (!state.ownership.some((owner) => pathContains(owner, target))) throw new WorkflowError("write_target_not_owned", `写入目标 ${target} 不属于登记范围 ${JSON.stringify(state.ownership)}`, 4);
  return { guarded: true, ...statePayload(state), writeTargets: targets, changedPaths: changed };
}

export function verifyUnit(identity, requireUnitCwd = true) {
  const state = loadUnitState(identity); validateUnitContext(identity, requireUnitCwd); const changed = verifyRegisteredChanges(state);
  return { verified: true, ...statePayload(state), changedPaths: changed };
}

export function removeUnit(identity) {
  const state = loadUnitState(identity); validateUnitContext(identity, false); const changed = verifyRegisteredChanges(state);
  if (runGit(identity.worktreePath, "status", "--porcelain=v1", "--untracked-files=all").stdout.trim()) throw new WorkflowError("worktree_dirty", "受管 Worktree 包含已跟踪或未跟踪修改", 4);
  withTaskStateLock(identity, () => { try { fs.unlinkSync(identity.statePath); } catch (error) { throw new WorkflowError("unit_state_remove_failed", `无法删除受管状态 ${identity.statePath}：${error.message}`, 4); } });
  return { removed: true, stateRemoved: true, resourcesRetained: true, worktreeRetained: true, branchRetained: true, branch: identity.branch, worktreePath: identity.worktreePath, changedPaths: changed };
}

function parseCommand(argv) {
  const command = argv[0];
  if (!["inspect", "create", "guard", "verify", "remove"].includes(command)) throw new WorkflowError("invalid_command", "必须提供 inspect、create、guard、verify 或 remove 子命令");
  const options = { "project-root": { type: "string" } };
  if (command !== "inspect") Object.assign(options, { "source-worktree": { type: "string" }, task: { type: "string" }, unit: { type: "string" } });
  if (["create", "guard"].includes(command)) options["write-target"] = { type: "string", multiple: true, default: [] };
  let values;
  try { ({ values } = parseArgs({ args: argv.slice(1), options, strict: true })); }
  catch (error) { throw new WorkflowError("invalid_arguments", error.message); }
  if (!values["project-root"] || (command !== "inspect" && (!values["source-worktree"] || !values.task || !values.unit))) throw new WorkflowError("invalid_arguments", "缺少必需命令参数");
  return { command, values };
}

export function execute(argv) {
  const { command, values } = parseCommand(argv);
  const { projectRoot, commonDir } = canonicalProjectRoot(values["project-root"]);
  if (command === "inspect") { requireExactCwd(projectRoot, "project_cwd_mismatch", "inspect 操作"); return { ok: true, operation: "inspect", ...inspectProject(projectRoot, commonDir) }; }
  const task = validateIdentifier("任务", values.task);
  const context = canonicalSourceContext(projectRoot, commonDir, values["source-worktree"]);
  const identity = unitIdentity(context, task, values.unit, command === "create");
  if (["create", "remove"].includes(command)) requireExactCwd(context.sourceWorktree, "source_cwd_mismatch", `${command} 操作`);
  let result;
  if (command === "create") result = createUnit(identity, values["write-target"]);
  else if (command === "guard") result = guardUnit(identity, values["write-target"]);
  else if (command === "verify") result = verifyUnit(identity);
  else result = removeUnit(identity);
  return { ok: true, operation: command, ...result };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try { process.stdout.write(`${JSON.stringify(execute(process.argv.slice(2)))}\n`); }
  catch (error) {
    const failure = error instanceof WorkflowError ? error : new WorkflowError("unexpected_error", error.message, 3);
    process.stdout.write(`${JSON.stringify({ ok: false, error: { code: failure.code, message: failure.message } })}\n`);
    process.exitCode = failure.exitCode;
  }
}
