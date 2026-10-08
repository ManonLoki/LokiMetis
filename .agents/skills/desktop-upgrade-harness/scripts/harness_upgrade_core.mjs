/** 为下游 Harness 工程升级生成三方计划并安全更新既有受管文件。 */

import fs from "node:fs";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { explicitOwnershipModeForNode, loadLock, loadOwnership, ownershipMode, scanTree } from "./harness_upgrade_ownership.mjs";
import { inapplicableGuiCandidate, readInterfaceSelection } from "./harness_upgrade_interfaces.mjs";
import { AUTO_MODES, BLOCKING_CLASSES, MANUAL_CLASSES, OWNERSHIP_RELATIVE, REQUIRED_MANAGED_SOURCE_PATHS, SCHEMA_VERSION, SOURCE_ONLY_PATHS } from "./harness_upgrade_policy.mjs";
import { UpgradeError, assertSafePath, canonicalDirectory, isWithin, loadJson, lstatOrNull, requireControlPaths, requireGitRoot, requireSourceIdentity, safeRelativePath, snapshotFile } from "./harness_upgrade_safety.mjs";

export function stableJson(value, indent = 0) {
  const sort = (item) => {
    if (Array.isArray(item)) return item.map(sort);
    if (item && typeof item === "object") return Object.fromEntries(Object.keys(item).sort().map((key) => [key, sort(item[key])]));
    return item;
  };
  return JSON.stringify(sort(value), null, indent);
}

export function classifyManaged(candidate, target, baseline, { lockLoaded }) {
  if (baseline === null) {
    if (candidate !== null && target === null) return "add";
    if (candidate !== null && target !== null) return isDeepStrictEqual(candidate, target) ? "converged" : (lockLoaded ? "collision" : "bootstrap_conflict");
    return "preserve_local";
  }
  const candidateChanged = !isDeepStrictEqual(candidate, baseline.candidate);
  const targetChanged = !isDeepStrictEqual(target, baseline.target);
  if (candidateChanged && targetChanged) return isDeepStrictEqual(candidate, target) ? "converged" : "conflict";
  if (targetChanged) return "preserve_local";
  if (!candidateChanged) return "unchanged";
  if (candidate === null) return "delete";
  if (target === null && baseline.target !== null) return "conflict";
  return "update";
}

export function classifyMixed(candidate, target, baseline) {
  if (baseline === null) {
    if (candidate === null) return "preserve_local";
    if (isDeepStrictEqual(candidate, target)) return "converged";
    return target !== null ? "manual_merge" : "manual_add";
  }
  if (isDeepStrictEqual(candidate, baseline.candidate)) return !isDeepStrictEqual(target, baseline.target) ? "preserve_local" : "unchanged";
  if (isDeepStrictEqual(candidate, target)) return "converged";
  return "manual_merge";
}

function overlaps(left, right) { return left === right || left.startsWith(`${right}${path.sep}`) || right.startsWith(`${left}${path.sep}`); }
/** 生成完整只读升级计划，不更改候选、目标或来源锁。 */
export function buildPlan(sourceRoot, sourceVersion, sourceCommit, candidateRoot, targetRoot, ownershipPath, lockPath) {
  const { source, identity: sourceIdentity } = requireSourceIdentity(sourceRoot, sourceVersion, sourceCommit);
  const candidate = canonicalDirectory(candidateRoot, "候选根目录");
  const target = canonicalDirectory(targetRoot, "目标根目录");
  const roots = [source, candidate, target];
  for (let index = 0; index < roots.length; index += 1) for (const right of roots.slice(index + 1)) if (overlaps(roots[index], right)) throw new UpgradeError("源、候选和目标根目录必须彼此独立且不能嵌套");
  const gitIdentity = requireGitRoot(target);
  const interfaceSelection = readInterfaceSelection(target);
  const { ownership, lockFile } = requireControlPaths(target, ownershipPath, lockPath);
  const { defaultMode, rules, missingManagedRules } = loadOwnership(ownership, { allowRecoverableManagedOmissions: true });
  const candidateTree = scanTree(candidate, { targetTree: false });
  const lock = loadLock(lockFile);
  const entries = lock === null ? {} : lock.entries;
  const retainPaths = [
    ...Object.keys(candidateTree.files), ...Object.keys(entries),
    ...rules.filter(([, mode]) => mode === "tombstone").map(([pattern]) => pattern.split("*")[0].replace(/\/$/u, "")),
  ];
  const targetTree = scanTree(target, { targetTree: true, retainPaths });
  const problems = candidateTree.unsafe.map((entry) => `候选包含符号链接、特殊文件或 Git 元数据：${entry}`);
  const candidateOwnership = OWNERSHIP_RELATIVE.split(path.sep).join("/");
  if (!candidateTree.files[candidateOwnership]) problems.push(`候选缺少所有权 manifest：${candidateOwnership}`);
  else {
    try { loadOwnership(path.join(candidate, OWNERSHIP_RELATIVE)); }
    catch (error) { problems.push(`候选所有权 manifest 无效：${error.message}`); }
  }

  for (const relative of SOURCE_ONLY_PATHS) {
    if (Object.hasOwn(candidateTree.nodes, relative)) problems.push(`候选不得传播上游专用检查器：${relative}`);
    if (Object.hasOwn(targetTree.nodes, relative)) problems.push(`目标须先移除旧版上游专用检查器：${relative}`);
  }

  for (const rawPath of REQUIRED_MANAGED_SOURCE_PATHS) {
    const relative = safeRelativePath(rawPath);
    const sourcePath = assertSafePath(source, path.join(source, ...relative.split("/")), `源 Harness 必需传播路径 ${relative}`, { finalMayBeMissing: true });
    const stat = lstatOrNull(sourcePath);
    if (stat === null) continue;
    if (!stat.isFile() || stat.isSymbolicLink()) problems.push(`源 Harness 必需传播路径不是普通文件：${relative}`);
    else if (!candidateTree.files[relative]) problems.push(`候选缺少源 Harness 必需 managed 路径：${relative}`);
    else if (candidateTree.files[relative].sha256 !== snapshotFile(sourcePath).sha256) problems.push(`候选的源 Harness 必需 managed 路径内容不匹配：${relative}`);
  }
  for (const [relative, isDirectory] of Object.entries(candidateTree.nodes)) {
    const mode = explicitOwnershipModeForNode(relative, { isDirectory, rules });
    if (["protected", "tombstone"].includes(mode)) problems.push(`候选包含禁止的 ${mode} 路径：${relative}`);
    if (inapplicableGuiCandidate(relative, interfaceSelection)) problems.push(`候选包含不适用于目标 GUI 框架的工程路径：${relative}`);
  }
  for (const [relative, isDirectory] of Object.entries(targetTree.nodes)) if (explicitOwnershipModeForNode(relative, { isDirectory, rules }) === "tombstone") problems.push(`目标包含 tombstone 路径：${relative}`);

  const relevant = new Set([...Object.keys(candidateTree.files), ...Object.keys(entries)]);
  for (const relative of Object.keys(targetTree.files)) if (ownershipMode(relative, defaultMode, rules) === "tombstone" || Object.hasOwn(entries, relative)) relevant.add(relative);
  for (const unsafe of targetTree.unsafe) if ([...relevant].some((relative) => relative === unsafe || relative.startsWith(`${unsafe}/`))) problems.push(`目标相关路径是符号链接或特殊文件：${unsafe}`);

  const actions = [];
  for (const rawPath of [...relevant].sort()) {
    const relative = safeRelativePath(rawPath);
    const mode = ownershipMode(relative, defaultMode, rules);
    const candidateSnapshot = candidateTree.files[relative] ?? null;
    const targetSnapshot = targetTree.files[relative] ?? null;
    const baseline = entries[relative] ?? null;
    if (baseline !== null && baseline.mode !== mode) problems.push(`${relative} 的所有权 mode 已变化：${baseline.mode} -> ${mode}`);
    let classification;
    if (mode === "tombstone") classification = candidateSnapshot !== null ? "tombstone_candidate" : (targetSnapshot !== null ? "tombstone_present" : "tombstone_absent");
    else if (mode === "protected") classification = candidateSnapshot !== null ? "protected_candidate" : "protected";
    else if (AUTO_MODES.has(mode)) classification = classifyManaged(candidateSnapshot, targetSnapshot, baseline, { lockLoaded: lock !== null });
    else classification = classifyMixed(candidateSnapshot, targetSnapshot, baseline);
    actions.push({ path: relative, mode, classification, candidate: candidateSnapshot, target: targetSnapshot, baseline_candidate: baseline?.candidate ?? null, baseline_target: baseline?.target ?? null, auto_apply: AUTO_MODES.has(mode) && classification === "update", blocked: BLOCKING_CLASSES.has(classification) });
  }
  return {
    schema_version: SCHEMA_VERSION, source_root: source, source_version: sourceVersion, source_commit: sourceCommit, source_git: sourceIdentity,
    candidate_root: candidate, target_root: target, ownership_path: ownership, ownership_snapshot: snapshotFile(ownership),
    lock_path: lockFile, lock_snapshot: lock === null ? null : snapshotFile(lockFile), target_git: gitIdentity,
    target_interfaces: interfaceSelection.interfaces, gui_framework: interfaceSelection.guiFramework,
    interface_manifest_snapshot: interfaceSelection.manifestSnapshot,
    baseline: lock === null ? "missing" : "loaded", ownership_repairs: missingManagedRules,
    blocked: problems.length > 0 || actions.some((item) => item.blocked),
    manual_required: actions.some((item) => MANUAL_CLASSES.has(item.classification)), problems: problems.sort(), actions,
  };
}

/** 只在候选/目标之外独占创建新计划，禁止覆盖任何现有文件。 */
export function writeNewPlan(outputPath, value, forbiddenRoots) {
  const parent = canonicalDirectory(path.dirname(outputPath), "plan 输出父目录");
  const output = path.join(parent, path.basename(outputPath));
  for (const root of forbiddenRoots) if (isWithin(output, root)) throw new UpgradeError(`plan 输出必须位于 ${root} 之外：${output}`);
  if (lstatOrNull(output) !== null) throw new UpgradeError(`plan 输出已存在，拒绝覆盖：${output}`);
  let descriptor;
  try {
    descriptor = fs.openSync(output, "wx", 0o600);
    fs.writeFileSync(descriptor, `${stableJson(value, 2)}\n`);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor); descriptor = undefined;
    try { const parentFd = fs.openSync(parent, fs.constants.O_RDONLY); fs.fsyncSync(parentFd); fs.closeSync(parentFd); } catch { /* 某些平台不允许 fsync 目录 */ }
  } catch (error) { if (descriptor !== undefined) fs.closeSync(descriptor); throw new UpgradeError(`无法创建 plan 输出 ${output}：${error.message}`); }
}

/** 重算计划并要求与受审 JSON 完全一致，拒绝篡改与状态漂移。 */
export function loadReviewedPlan(planPath) {
  const reviewed = loadJson(planPath, "升级 plan");
  if (reviewed.schema_version !== SCHEMA_VERSION) throw new UpgradeError(`升级 plan 的 schema_version 必须为 ${SCHEMA_VERSION}`);
  for (const key of ["source_root", "source_version", "source_commit", "candidate_root", "target_root", "ownership_path", "lock_path"]) if (!Object.hasOwn(reviewed, key)) throw new UpgradeError("升级 plan 缺少 provenance 字段");
  const recomputed = buildPlan(reviewed.source_root, reviewed.source_version, reviewed.source_commit, reviewed.candidate_root, reviewed.target_root, reviewed.ownership_path, reviewed.lock_path);
  if (!isDeepStrictEqual(reviewed, recomputed)) throw new UpgradeError("已复核 plan 不再匹配源、所有权、lock、Git 身份、候选或目标；请生成并复核新 plan");
  return { plan: reviewed, recomputed };
}
