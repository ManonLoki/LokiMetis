/** 完成 Harness 升级后原子记录共同基线。 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { AUTO_MODES, SCHEMA_VERSION } from "./harness_upgrade_policy.mjs";
import { loadLock, loadOwnership } from "./harness_upgrade_ownership.mjs";
import { stableJson, loadReviewedPlan } from "./harness_upgrade_core.mjs";
import { UpgradeError, assertSafePath, canonicalDirectory, lstatOrNull, safeRelativePath } from "./harness_upgrade_safety.mjs";
import { assertPlanStillCurrent } from "./harness_upgrade_preflight.mjs";

/** 只在精确 `.harness` 目录内原子替换来源锁。 */
export function writeLockAtomic(target, lockPath, value) {
  assertSafePath(target, lockPath, "上游 lock", { finalMayBeMissing: true });
  const harnessDirectory = path.join(target, ".harness");
  const directoryStat = lstatOrNull(harnessDirectory);
  if (!directoryStat) fs.mkdirSync(harnessDirectory, { mode: 0o755 });
  const observedDirectory = fs.lstatSync(harnessDirectory);
  if (observedDirectory.isSymbolicLink() || !observedDirectory.isDirectory()) throw new UpgradeError(".harness 不是稳定目录");
  const directoryIdentity = { dev: observedDirectory.dev, ino: observedDirectory.ino };
  assertSafePath(target, harnessDirectory, "上游 lock 目录", { finalMayBeMissing: false });
  const existing = lstatOrNull(lockPath);
  if (existing && (existing.isSymbolicLink() || !existing.isFile())) throw new UpgradeError("上游 lock 不是普通文件");
  const temporary = path.join(harnessDirectory, `.upstream-lock-${crypto.randomUUID()}.tmp`);
  let descriptor;
  try {
    descriptor = fs.openSync(temporary, "wx", 0o600);
    fs.writeFileSync(descriptor, `${stableJson(value, 2)}\n`);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor); descriptor = undefined;
    assertSafePath(target, lockPath, "上游 lock", { finalMayBeMissing: true });
    const beforeReplace = lstatOrNull(lockPath);
    if (beforeReplace && (beforeReplace.isSymbolicLink() || !beforeReplace.isFile())) throw new UpgradeError("上游 lock 不是普通文件");
    const currentDirectory = fs.lstatSync(harnessDirectory);
    if (currentDirectory.isSymbolicLink() || !currentDirectory.isDirectory() || currentDirectory.dev !== directoryIdentity.dev || currentDirectory.ino !== directoryIdentity.ino) throw new UpgradeError(".harness 在 lock 写入期间发生变化");
    fs.renameSync(temporary, lockPath);
    try { const directoryFd = fs.openSync(harnessDirectory, fs.constants.O_RDONLY); fs.fsyncSync(directoryFd); fs.closeSync(directoryFd); } catch { /* 平台可能不允许 fsync 目录 */ }
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    fs.rmSync(temporary, { force: true });
  }
}

/** 把已复核且已完成应用/人工合并的计划记录为新共同基线。 */
export function recordLock(args) {
  const expected = args.bootstrap ? "bootstrap-verified-baseline" : "record-verified-baseline";
  if (args.approval !== expected) throw new UpgradeError(`record 要求传入 --approval ${expected}`);
  const { plan } = loadReviewedPlan(args.plan);
  const target = canonicalDirectory(plan.target_root, "目标根目录");
  if (args.sourceVersion !== plan.source_version || args.sourceCommit !== plan.source_commit) throw new UpgradeError("record 的源版本/commit 必须与已复核 plan 精确匹配");
  assertPlanStillCurrent(plan);
  loadOwnership(plan.ownership_path);
  const existingLock = loadLock(plan.lock_path);
  const manualPaths = new Set(plan.actions.filter((item) => item.classification === "manual_merge").map((item) => item.path));
  const resolvedManual = new Set((args.resolvedManual ?? []).map(safeRelativePath));
  if (manualPaths.size !== resolvedManual.size || [...manualPaths].some((item) => !resolvedManual.has(item))) throw new UpgradeError(`record 要求 --resolved-manual 精确列出已复核混合所有权操作的路径：预期=${JSON.stringify([...manualPaths].sort())}，实际=${JSON.stringify([...resolvedManual].sort())}`);
  if (plan.problems.length) throw new UpgradeError("存在路径或所有权问题的 plan 不能 record");
  if (args.bootstrap) {
    if (plan.baseline !== "missing" || existingLock !== null) throw new UpgradeError("bootstrap record 要求 lock 不存在");
    for (const item of plan.actions) {
      const classification = item.classification;
      if (["protected_candidate", "tombstone_candidate", "tombstone_present"].includes(classification)) throw new UpgradeError(`bootstrap 包含禁止路径：${item.path}`);
      if (AUTO_MODES.has(item.mode)) {
        if (["add", "delete", "update", "conflict", "collision"].includes(classification)) throw new UpgradeError(`bootstrap 的 managed 路径尚未解决：${item.path}`);
        if (classification === "bootstrap_conflict" && !isDeepStrictEqual(item.candidate, item.target)) throw new UpgradeError(`bootstrap 的 managed 重叠项必须先收敛：${item.path}`);
        if (item.blocked && classification !== "bootstrap_conflict") throw new UpgradeError(`bootstrap 包含不可复核的 blocker：${item.path}`);
      } else if (classification === "manual_add") throw new UpgradeError(`bootstrap 的 mixed 路径必须先创建并重新生成 plan：${item.path}`);
    }
  } else {
    if (plan.baseline !== "loaded" || existingLock === null) throw new UpgradeError("非 bootstrap record 要求已有 lock");
    if (plan.blocked) throw new UpgradeError("不能 record 已阻断的升级 plan");
    const unresolved = plan.actions.filter((item) => ["add", "delete", "update", "manual_add"].includes(item.classification)).map((item) => item.path);
    if (unresolved.length) throw new UpgradeError(`仍有 managed 操作未解决：${unresolved.join(", ")}`);
  }
  const previousEntries = existingLock === null ? {} : existingLock.entries;
  const entries = {};
  for (const item of plan.actions) {
    if (["protected", "tombstone"].includes(item.mode) || (item.candidate === null && item.target === null)) continue;
    if (AUTO_MODES.has(item.mode) && item.classification === "preserve_local" && Object.hasOwn(previousEntries, item.path)) entries[item.path] = previousEntries[item.path];
    else entries[item.path] = { mode: item.mode, candidate: item.candidate, target: item.target };
  }
  const lock = { schema_version: SCHEMA_VERSION, recorded_at: new Date().toISOString().replace("Z", "+00:00"), harness_source: { version: plan.source_version, commit: plan.source_commit }, entries };
  assertPlanStillCurrent(plan);
  writeLockAtomic(target, plan.lock_path, lock);
  return { recorded: plan.lock_path, entries: Object.keys(entries).length };
}
