/** 执行受审 Harness 升级计划中的单文件原子更新。 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { AUTO_MODES } from "./harness_upgrade_policy.mjs";
import { UpgradeError, assertSafePath, canonicalDirectory, safeRelativePath, snapshotFd, snapshotFile, validateSnapshot } from "./harness_upgrade_safety.mjs";
import { loadReviewedPlan } from "./harness_upgrade_core.mjs";
import { assertCurrentSnapshot, assertPlanStillCurrent } from "./harness_upgrade_preflight.mjs";

function sameDirectoryIdentity(left, right) { return left.dev === right.dev && left.ino === right.ino && left.isDirectory() && right.isDirectory(); }

/** 使用候选/目标双重快照、稳定父目录身份和同目录原子替换更新文件。 */
export function replaceExisting(candidateRoot, targetRoot, relative, candidateSnapshot, targetSnapshot) {
  const source = assertSafePath(candidateRoot, path.join(candidateRoot, ...relative.split("/")), "候选", { finalMayBeMissing: false });
  const destination = assertSafePath(targetRoot, path.join(targetRoot, ...relative.split("/")), "目标", { finalMayBeMissing: false });
  const targetParent = path.dirname(destination);
  const parentBefore = fs.statSync(targetParent);
  let sourceDescriptor;
  let destinationDescriptor;
  let temporaryDescriptor;
  const temporary = path.join(targetParent, `.harness-upgrade-${crypto.randomUUID()}.tmp`);
  try {
    sourceDescriptor = fs.openSync(source, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0));
    if (!isDeepStrictEqual(snapshotFd(sourceDescriptor), candidateSnapshot)) throw new UpgradeError(`候选在 apply 期间发生变化：${relative}`);
    destinationDescriptor = fs.openSync(destination, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0));
    if (!isDeepStrictEqual(snapshotFd(destinationDescriptor), targetSnapshot)) throw new UpgradeError(`目标在 apply 期间发生变化：${relative}`);
    fs.closeSync(destinationDescriptor); destinationDescriptor = undefined;
    temporaryDescriptor = fs.openSync(temporary, "wx", candidateSnapshot.mode);
    const digest = crypto.createHash("sha256");
    const buffer = Buffer.allocUnsafe(1024 * 1024);
    let readPosition = 0;
    for (;;) {
      const bytes = fs.readSync(sourceDescriptor, buffer, 0, buffer.length, readPosition);
      if (!bytes) break;
      const chunk = buffer.subarray(0, bytes); digest.update(chunk);
      let written = 0;
      while (written < bytes) written += fs.writeSync(temporaryDescriptor, chunk, written, bytes - written);
      readPosition += bytes;
    }
    fs.fchmodSync(temporaryDescriptor, candidateSnapshot.mode);
    fs.fsyncSync(temporaryDescriptor);
    fs.closeSync(temporaryDescriptor); temporaryDescriptor = undefined;
    if (digest.digest("hex") !== candidateSnapshot.sha256) throw new UpgradeError(`候选在复制期间发生变化：${relative}`);
    if ((fs.fstatSync(sourceDescriptor).mode & 0o7777) !== candidateSnapshot.mode) throw new UpgradeError(`候选 mode 在复制期间发生变化：${relative}`);
    assertSafePath(targetRoot, destination, "目标", { finalMayBeMissing: false });
    if (!sameDirectoryIdentity(parentBefore, fs.statSync(targetParent))) throw new UpgradeError(`目标父目录在 replace 前发生变化：${relative}`);
    destinationDescriptor = fs.openSync(destination, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0));
    if (!isDeepStrictEqual(snapshotFd(destinationDescriptor), targetSnapshot)) throw new UpgradeError(`目标在 replace 前发生变化：${relative}`);
    fs.closeSync(destinationDescriptor); destinationDescriptor = undefined;
    fs.renameSync(temporary, destination);
    try { const parentDescriptor = fs.openSync(targetParent, fs.constants.O_RDONLY); fs.fsyncSync(parentDescriptor); fs.closeSync(parentDescriptor); } catch { /* 平台可能不允许 fsync 目录 */ }
  } finally {
    if (sourceDescriptor !== undefined) fs.closeSync(sourceDescriptor);
    if (destinationDescriptor !== undefined) fs.closeSync(destinationDescriptor);
    if (temporaryDescriptor !== undefined) fs.closeSync(temporaryDescriptor);
    fs.rmSync(temporary, { force: true });
  }
}

/** 重算计划后一次只替换一个既有受管文件，避免批量部分应用。 */
export function applyPlan(planPath, approval, selectedPath) {
  if (approval !== "apply-managed-changes") throw new UpgradeError("apply 要求传入 --approval apply-managed-changes");
  const { plan } = loadReviewedPlan(planPath);
  if (plan.blocked) throw new UpgradeError("升级 plan 已被阻断；请在 apply 前解决 conflict");
  const pending = plan.actions.filter((item) => item && item.auto_apply === true);
  for (const item of pending) {
    const relative = safeRelativePath(item.path);
    if (!AUTO_MODES.has(item.mode) || item.classification !== "update") throw new UpgradeError(`plan 包含不安全的自动操作：${relative}`);
  }
  const selected = safeRelativePath(selectedPath);
  const matches = pending.filter((item) => item.path === selected);
  if (matches.length !== 1) throw new UpgradeError(`--path 必须精确指定一个已复核且可自动应用的 update：${selected}`);
  const item = matches[0];
  if (item.mode === "managed-self") {
    const normalPending = pending.filter((action) => action.mode !== "managed-self").map((action) => action.path).sort();
    if (normalPending.length) throw new UpgradeError(`managed-self 更新必须等待全部普通 managed 更新解决后再执行：${normalPending.join(", ")}`);
    const selfPending = pending.filter((action) => action.mode === "managed-self").map((action) => action.path).sort((left, right) => {
      const leftMain = path.basename(left) === "harness_upgrade.mjs" ? 1 : 0;
      const rightMain = path.basename(right) === "harness_upgrade.mjs" ? 1 : 0;
      return leftMain - rightMain || left.localeCompare(right, "en");
    });
    if (selected !== selfPending[0]) throw new UpgradeError(`managed-self 更新顺序要求先处理 ${selfPending[0]}，再处理 ${selected}`);
  }
  assertPlanStillCurrent(plan);
  const candidate = canonicalDirectory(plan.candidate_root, "候选根目录");
  const target = canonicalDirectory(plan.target_root, "目标根目录");
  const candidateSnapshot = validateSnapshot(item.candidate, `${selected}.candidate`);
  const targetSnapshot = validateSnapshot(item.target, `${selected}.target`);
  if (candidateSnapshot === null || targetSnapshot === null) throw new UpgradeError(`update 要求两个文件都已存在：${selected}`);
  assertCurrentSnapshot(candidate, selected, candidateSnapshot, "候选");
  assertCurrentSnapshot(target, selected, targetSnapshot, "目标");
  replaceExisting(candidate, target, selected, candidateSnapshot, targetSnapshot);
  return { applied: [selected], count: 1 };
}
