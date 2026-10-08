/** Harness 升级写入前的快照与计划复验。 */

import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { UpgradeError, assertSafePath, canonicalDirectory, lstatOrNull, requireGitRoot, safeRelativePath, snapshotFile, validateSnapshot } from "./harness_upgrade_safety.mjs";

export function assertCurrentSnapshot(root, relative, expected, label) {
  const target = assertSafePath(root, path.join(root, ...relative.split("/")), label, { finalMayBeMissing: expected === null });
  if (expected === null) { if (lstatOrNull(target) !== null) throw new UpgradeError(`${label}意外存在：${relative}`); return target; }
  const stat = lstatOrNull(target);
  if (!stat?.isFile() || stat.isSymbolicLink()) throw new UpgradeError(`${label}不是普通文件：${relative}`);
  const observed = snapshotFile(target);
  if (!isDeepStrictEqual(observed, expected)) throw new UpgradeError(`${label}在 plan 后发生变化：${relative}；预期 ${JSON.stringify(expected)}，实际 ${JSON.stringify(observed)}`);
  return target;
}

export function assertPlanStillCurrent(plan) {
  const candidate = canonicalDirectory(plan.candidate_root, "候选根目录");
  const target = canonicalDirectory(plan.target_root, "目标根目录");
  if (!isDeepStrictEqual(snapshotFile(plan.ownership_path), validateSnapshot(plan.ownership_snapshot, "ownership_snapshot"))) throw new UpgradeError("所有权 manifest 在 plan 后发生变化");
  const expectedLock = validateSnapshot(plan.lock_snapshot, "lock_snapshot");
  if (expectedLock === null) { if (lstatOrNull(plan.lock_path) !== null) throw new UpgradeError("上游 lock 在 plan 后出现"); }
  else if (!isDeepStrictEqual(snapshotFile(plan.lock_path), expectedLock)) throw new UpgradeError("上游 lock 在 plan 后发生变化");
  if (!isDeepStrictEqual(requireGitRoot(target), plan.target_git)) throw new UpgradeError("目标 Git 身份或工作树状态在 plan 后发生变化");
  assertCurrentSnapshot(target, "Cargo.toml", validateSnapshot(plan.interface_manifest_snapshot, "interface_manifest_snapshot"), "目标接口清单");
  for (const item of plan.actions) {
    const relative = safeRelativePath(item.path);
    assertCurrentSnapshot(candidate, relative, validateSnapshot(item.candidate, `${relative}.candidate`), "候选");
    assertCurrentSnapshot(target, relative, validateSnapshot(item.target, `${relative}.target`), "目标");
  }
}
