/** Git 生命周期的远端推送与本地 Git 发布。 */

import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { CONTEXT_RELATIVE_PATH as RELEASE_CONTEXT_PATH, canonicalBytes, validateContext } from "../../desktop-prepare-release/scripts/release_context.mjs";
import { parseDocumentBytes as parseReleaseNotesBytes } from "../../desktop-prepare-release/scripts/release_notes.mjs";
import { inspect as inspectPostReleaseAction } from "../../desktop-switch-post-release-action/scripts/post_release_action.mjs";
import { pendingFailure } from "./git_publication_report.mjs";
import {
  HEX_OID_RE,
  LifecycleError,
  SHA256_RE,
  branchExists,
  configuredRemotes,
  currentBranchOrNone,
  currentHead,
  loadState,
  localTagTarget,
  mergeRegisteredBranches,
  preflightCycleResources,
  primaryRepository,
  remoteBranchOid,
  remoteDefaultBranch,
  remoteTagTarget,
  requireClean,
  resolveAdditionalRemoteTargets,
  runGit,
  saveState,
  selectRemote,
  shanghaiDate,
  switchToDefault,
  validBranch,
  validRemote,
  verifyLocalPosition,
  worktreeRecords,
} from "./git_lifecycle_core.mjs";

const RELEASE_CONTEXT_HELPER = ".agents/skills/desktop-prepare-release/scripts/release_context.mjs";
const HARNESS_VERSION_CLOCK_HELPER = ".agents/skills/desktop-prepare-release/scripts/harness_version_clock.mjs";
const HARNESS_INITIALIZATION_SKILL = ".agents/skills/desktop-instantiate-project/SKILL.md";
const PRODUCT_VERSION_HELPER = ".agents/skills/desktop-manage-version/scripts/version_gate.mjs";
const PRODUCT_VERSION_STATE = ".harness/version-state.json";
const RELEASE_NOTES_PATH = "release-notes.json";
/** 可发布版本：Harness 12 位时间版本，或 Minor/Patch 位于 0..99 的稳定三段版本。 */
const RELEASE_VERSION_RE = /^(?:\d{12}|(?:0|[1-9]\d*)\.(?:0|[1-9]\d?)\.(?:0|[1-9]\d?))$/u;

/** 工作区文件字节必须等于 HEAD 中同一路径 blob，仅容许 CRLF 检出换行；所有被执行或作为事实源的文件共用此判定。 */
function workingBytesMatchHead(repository, relative, working = readFileSync(join(repository.root, relative))) {
  const committed = runGit(repository.root, ["show", `HEAD:${relative}`], { check: false, bytes: true });
  if (committed.returncode !== 0) return false;
  return committed.stdout.equals(working) ||
    committed.stdout.equals(Buffer.from(working.toString("utf8").replaceAll("\r\n", "\n"), "utf8"));
}

/** Harness 发布时间版本必须有受管取号凭证；下游三段版本不走此路径。 */
async function verifyHarnessVersionStamp(repository, version, { checkCommittedVersion = false } = {}) {
  const versionFile = join(repository.root, "Version.md");
  const initializationSkill = join(repository.root, HARNESS_INITIALIZATION_SKILL);
  const hasVersionFile = existsSync(versionFile);
  const hasInitializationSkill = existsSync(initializationSkill);
  if (!hasVersionFile && !hasInitializationSkill && !/^\d{12}$/u.test(version)) return false;
  if (!hasVersionFile || !hasInitializationSkill || !/^\d{12}$/u.test(version)) {
    throw new LifecycleError("harness-version-stamp-invalid", "Harness release identity is incomplete or is not a timestamp version.");
  }
  const helperPath = join(repository.root, HARNESS_VERSION_CLOCK_HELPER);
  try {
    const metadata = lstatSync(helperPath);
    if (metadata.isSymbolicLink() || !metadata.isFile()) throw new Error("unsafe");
    if (!workingBytesMatchHead(repository, HARNESS_VERSION_CLOCK_HELPER)) throw new Error("helper changed");
    const clock = await import(`${pathToFileURL(helperPath).href}?binding=${Date.now()}-${Math.random()}`);
    if (typeof clock.readHarnessVersionStamp !== "function") throw new Error("missing reader");
    clock.readHarnessVersionStamp(repository.root, version);
    if (checkCommittedVersion && !workingBytesMatchHead(repository, "Version.md")) throw new Error("integrated version differs");
  } catch {
    throw new LifecycleError("harness-version-stamp-invalid", "Harness release requires its managed current-minute version stamp.");
  }
  return true;
}

/** 合并后复核最终提交的产品版本事实，防止后继分支盖过已冻结版本。 */
async function verifyFinalReleaseVersion(repository, version) {
  if (await verifyHarnessVersionStamp(repository, version, { checkCommittedVersion: true })) return;
  const cargoPath = join(repository.root, "Cargo.toml");
  const statePath = join(repository.root, PRODUCT_VERSION_STATE);
  const helperPath = join(repository.root, PRODUCT_VERSION_HELPER);
  // 三个版本事实源全部缺席时不是受管 Rust 下游；只存在部分时下方 lstat 失败关闭。
  if (![cargoPath, statePath, helperPath].some(existsSync)) return;
  try {
    for (const path of [cargoPath, statePath, helperPath]) {
      const metadata = lstatSync(path);
      if (metadata.isSymbolicLink() || !metadata.isFile()) throw new Error("unsafe version source");
    }
    if (!workingBytesMatchHead(repository, PRODUCT_VERSION_HELPER)) throw new Error("untracked version helper");
    const gate = await import(`${pathToFileURL(helperPath).href}?binding=${Date.now()}-${Math.random()}`);
    if (typeof gate.check !== "function") throw new Error("missing version check");
    const result = gate.check(repository.root, "release");
    if (result.passed !== true || result.current_version !== version) throw new Error("wrong final version");
    for (const relative of ["Cargo.toml", PRODUCT_VERSION_STATE]) {
      if (!workingBytesMatchHead(repository, relative)) throw new Error("integrated version source differs");
    }
  } catch {
    throw new LifecycleError("release-version-mismatch", "Integrated release HEAD does not match its frozen product version.");
  }
}

/** 验证已跟踪发布上下文及其固定调用身份。 */
export async function releaseContextBinding(repository, expectedSha256, identity) {
  if (!SHA256_RE.test(expectedSha256)) throw new LifecycleError("invalid-argument", "Release context SHA-256 is invalid.");
  const directory = join(repository.root, ".harness");
  const path = join(repository.root, RELEASE_CONTEXT_PATH);
  const helperPath = join(repository.root, RELEASE_CONTEXT_HELPER);
  try {
    const directoryMetadata = lstatSync(directory);
    const pathMetadata = lstatSync(path);
    const helperMetadata = lstatSync(helperPath);
    if (directoryMetadata.isSymbolicLink() || !directoryMetadata.isDirectory() ||
        pathMetadata.isSymbolicLink() || !pathMetadata.isFile() ||
        helperMetadata.isSymbolicLink() || !helperMetadata.isFile()) throw new Error("unsafe");
  } catch {
    throw new LifecycleError("release-context-invalid", "Tracked release context is unavailable or unsafe.");
  }
  let raw;
  let value;
  let canonical;
  try {
    if (!workingBytesMatchHead(repository, RELEASE_CONTEXT_HELPER)) {
      throw new LifecycleError("release-context-invalid", "Release context validator is not tracked by current HEAD.");
    }
    if (statSync(path).size > 1_048_576) throw new LifecycleError("release-context-invalid", "Tracked release context is too large.");
    raw = readFileSync(path);
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(raw));
    const module = await import(`${pathToFileURL(helperPath).href}?binding=${Date.now()}-${Math.random()}`);
    if (typeof module.validateContext !== "function" || typeof module.canonicalBytes !== "function") {
      throw new LifecycleError("release-context-invalid", "Release context validator interface is invalid.");
    }
    value = module.validateContext(value);
    canonical = module.canonicalBytes(value);
    if (value.releaseReview.selection === "enabled") {
      if (typeof module.verifyReviewScope !== "function") {
        throw new LifecycleError("release-context-invalid", "Release review scope verifier is unavailable.");
      }
      module.verifyReviewScope(repository.root, value);
    }
  } catch (error) {
    if (error instanceof LifecycleError) throw error;
    throw new LifecycleError("release-context-invalid", "Tracked release context cannot be validated.");
  }
  const normalized = Buffer.from(new TextDecoder("utf-8", { fatal: true }).decode(raw).replaceAll("\r\n", "\n"), "utf8");
  if (!normalized.equals(canonical)) throw new LifecycleError("release-context-invalid", "Release context bytes are not canonical.");
  if (createHash("sha256").update(canonical).digest("hex") !== expectedSha256) {
    throw new LifecycleError("release-context-mismatch", "Release context SHA-256 does not match.");
  }
  const expectedDate = `${identity.date.slice(0, 4)}-${identity.date.slice(4, 6)}-${identity.date.slice(6)}`;
  if (value.version !== identity.version || value.releaseDate !== expectedDate || value.expectedTag !== identity.tag) {
    throw new LifecycleError("release-context-mismatch", "Release context identity does not match the release command.");
  }
  if (value.schemaVersion !== 3) throw new LifecycleError("release-context-invalid", "Release context schema is not supported.");
  if (typeof value.defaultBranch !== "string" || !validBranch(repository, value.defaultBranch)) {
    throw new LifecycleError("release-context-invalid", "Release context default branch is invalid.");
  }
  const committed = runGit(repository.root, ["show", `HEAD:${RELEASE_CONTEXT_PATH}`], { check: false, bytes: true });
  if (committed.returncode !== 0 || !committed.stdout.equals(canonical)) {
    throw new LifecycleError("release-context-mismatch", "Release context bytes are not tracked by current HEAD.");
  }
  return value;
}

/** 确认冻结 HEAD 携带同一发布上下文。 */
function verifyHeadReleaseContextBytes(repository, expectedSha256, head) {
  const committed = runGit(repository.root, ["show", `${head}:${RELEASE_CONTEXT_PATH}`], { check: false, bytes: true });
  if (committed.returncode !== 0 || createHash("sha256").update(committed.stdout).digest("hex") !== expectedSha256) {
    throw new LifecycleError("release-context-mismatch", "Integrated release HEAD does not contain the bound release context bytes.");
  }
}

/** 已启用审查时，源码必须止于 sourceHead，之后仅允许两份冻结元数据。 */
function onlyReviewedReleaseMetadataChanged(repository, sourceHead, head) {
  const ancestry = runGit(repository.root, ["merge-base", "--is-ancestor", sourceHead, head], { check: false });
  if (ancestry.returncode !== 0) {
    throw new LifecycleError("release-review-scope-changed", "Reviewed source is not an ancestor of the release candidate.");
  }
  const changed = runGit(repository.root, ["diff", "--no-renames", "--name-only", "-z", sourceHead, head, "--"],
    { check: false, bytes: true });
  if (changed.returncode !== 0) {
    throw new LifecycleError("release-review-scope-changed", "Reviewed source differences could not be checked.");
  }
  const allowed = new Set([RELEASE_CONTEXT_PATH, RELEASE_NOTES_PATH]);
  const paths = changed.stdout.toString("utf8").split("\0").filter(Boolean);
  if (paths.some((path) => !allowed.has(path))) {
    throw new LifecycleError("release-review-scope-changed", "Unreviewed source changes were found after the reviewed source commit.");
  }
}

/** 绑定调用者的已审查源码与元数据，防止后来登记未审查分支。 */
function reviewedReleaseMetadata(repository, context, state) {
  if (context.releaseReview.selection !== "enabled") return null;
  const metadataHead = currentHead(repository);
  onlyReviewedReleaseMetadataChanged(repository, context.sourceHead, metadataHead);
  const notes = runGit(repository.root, ["show", `${metadataHead}:${RELEASE_NOTES_PATH}`], { check: false, bytes: true });
  if (notes.returncode !== 0) {
    throw new LifecycleError("release-review-scope-changed", "Reviewed release notes are missing from the metadata commit.");
  }
  for (const branch of state.cycle?.branches ?? []) {
    const included = runGit(repository.root,
      ["merge-base", "--is-ancestor", `refs/heads/${branch.name}`, metadataHead], { check: false });
    if (included.returncode !== 0) {
      throw new LifecycleError("release-review-scope-changed", `Registered branch ${branch.name} was not included in the reviewed source.`);
    }
  }
  return { sourceHead: context.sourceHead, releaseNotes: notes.stdout };
}

/** 合并后的主分支仍只包含已审查源码和完全相同的发布元数据。 */
function verifyReviewedFinalHead(repository, reviewed, head) {
  if (reviewed === null) return;
  onlyReviewedReleaseMetadataChanged(repository, reviewed.sourceHead, head);
  const notes = runGit(repository.root, ["show", `${head}:${RELEASE_NOTES_PATH}`], { check: false, bytes: true });
  if (notes.returncode !== 0 || !notes.stdout.equals(reviewed.releaseNotes)) {
    throw new LifecycleError("release-review-scope-changed", "Integrated release notes differ from the reviewed metadata commit.");
  }
}

/** 把发布结果报告转换成生命周期错误。 */
function pendingPublicationError(pending, index, kind, detail, outcome) {
  const [code, message] = pendingFailure(pending, index, kind, detail, outcome);
  return new LifecycleError(code, message);
}

/** 复读或推送一个冻结目标，并立即保存确认进度。 */
function confirmPendingPublishTarget(repository, state, index) {
  const pending = state.pendingPublish;
  const target = pending.targets[index];
  const head = pending.head;
  const singleTarget = pending.targets.length === 1;
  let remoteHead;
  try {
    remoteHead = remoteBranchOid(repository, target.remote, target.branch);
  } catch (error) {
    if (singleTarget) throw error;
    throw pendingPublicationError(pending, index, "verification-failed", "could not be reread before publication resumed", "outcome is uncertain");
  }
  if (target.confirmed && remoteHead !== head) {
    throw pendingPublicationError(pending, index, "verification-failed", "no longer matches the frozen published HEAD", "previous confirmation has changed");
  }
  if (remoteHead !== head) {
    const transportError = pendingPublicationError(pending, index, "push-failed", "push transport could not be confirmed", "outcome is uncertain");
    const pushed = runGit(repository.root, ["push", target.remote, `${head}:refs/heads/${target.branch}`], {
      check: false,
      code: singleTarget ? "git-error" : transportError.code,
      message: singleTarget ? "Git operation failed." : transportError.publicMessage,
    });
    try {
      remoteHead = remoteBranchOid(repository, target.remote, target.branch);
    } catch (error) {
      if (singleTarget && pushed.returncode === 0) throw error;
      throw transportError;
    }
    if (remoteHead !== head) {
      if (pushed.returncode !== 0) throw transportError;
      throw pendingPublicationError(pending, index, "verification-failed", "did not reread the frozen published HEAD", "outcome is uncertain");
    }
  }
  try {
    verifyLocalPosition(repository, state.defaultBranch, head);
  } catch {
    throw pendingPublicationError(pending, index, "local-state-changed", "was confirmed published but local Git state verification failed", "is confirmed published");
  }
  if (target.confirmed) return;
  target.confirmed = true;
  try {
    saveState(repository, state);
  } catch {
    throw pendingPublicationError(pending, index, "state-write-failed", "was confirmed published but lifecycle state could not be saved", "is confirmed published");
  }
}

/** 沿用已落盘 HEAD 和目标完成 publish。 */
function completePendingPublish(repository, state, merged = [], alreadyMerged = []) {
  const pending = state.pendingPublish;
  const head = pending.head;
  const primary = pending.targets[0];
  verifyLocalPosition(repository, primary.branch, head);
  const configured = new Set(configuredRemotes(repository));
  if (pending.targets.some((target) => !configured.has(target.remote))) {
    throw new LifecycleError("remote-not-found", "A frozen publication remote is not configured.");
  }
  for (let index = 0; index < pending.targets.length; index += 1) confirmPendingPublishTarget(repository, state, index);
  const publishedRemotes = pending.targets.map(({ remote, branch }) => ({ remote, branch }));
  verifyLocalPosition(repository, primary.branch, head);
  state.pendingPublish = null;
  saveState(repository, state);
  return {
    status: "published", branch: primary.branch, head, remote: primary.remote,
    worktree: repository.root, merged, alreadyMerged, publishedRemotes, tagged: false, cleaned: false,
  };
}

/** 合并并把同一 HEAD 非强制推送到全部目标。 */
export function publish(repository, explicitRemote, additionalRemotes = []) {
  requireClean(repository);
  const state = loadState(repository);
  if (state.pendingPublish !== null) {
    const primary = state.pendingPublish.targets[0];
    const frozenAdditional = state.pendingPublish.targets.slice(1).map((target) => target.remote);
    if ((explicitRemote !== undefined && explicitRemote !== null && explicitRemote !== primary.remote) ||
        JSON.stringify(additionalRemotes) !== JSON.stringify(frozenAdditional)) {
      throw new LifecycleError("publish-in-progress", "Publication retry arguments differ from the frozen targets.");
    }
    return completePendingPublish(repository, state);
  }
  if (state.cycle === null && state.lastRelease !== null) {
    throw new LifecycleError("release-already-complete", "Git release is complete; use push-release for its frozen HEAD and tag.");
  }
  if (state.cycle?.pendingRelease !== null && state.cycle !== null) {
    throw new LifecycleError("release-in-progress", "A release must finish before publication.");
  }
  const remote = selectRemote(repository, state, explicitRemote, { required: true });
  const defaultBranch = remoteDefaultBranch(repository, remote);
  const additionalTargets = resolveAdditionalRemoteTargets(repository, remote, additionalRemotes);
  preflightCycleResources(repository, state);
  runGit(repository.root, ["fetch", remote, `refs/heads/${defaultBranch}:refs/remotes/${remote}/${defaultBranch}`], {
    code: "remote-read-failed", message: "Git default branch could not be fetched.",
  });
  switchToDefault(repository, remote, defaultBranch);
  if (runGit(repository.root, ["merge", "--no-edit", `refs/remotes/${remote}/${defaultBranch}`], { check: false }).returncode !== 0) {
    throw new LifecycleError("merge-failed", "Git merge did not complete; inspect the worktree state.");
  }
  const [merged, alreadyMerged] = mergeRegisteredBranches(repository, state, defaultBranch);
  requireClean(repository);
  const head = currentHead(repository);
  const targets = [{ remote, branch: defaultBranch }, ...additionalTargets];
  state.remote = remote;
  state.defaultBranch = defaultBranch;
  state.pendingPublish = { head, targets: targets.map((target) => ({ ...target, confirmed: false })) };
  saveState(repository, state);
  return completePendingPublish(repository, state, merged, alreadyMerged);
}

/** publish 命令保持不创建标签、不清理的边界。 */
export function commandPublish(repository, args) {
  return publish(primaryRepository(repository), args.remote, args.alsoRemote);
}

/** 只在本地默认分支整合登记结果。 */
function prepareLocalRelease(repository, state) {
  requireClean(repository);
  const defaultBranch = state.defaultBranch;
  if (defaultBranch === null || !branchExists(repository, defaultBranch)) {
    throw new LifecycleError("local-default-unavailable", "Recorded local default branch is unavailable.");
  }
  preflightCycleResources(repository, state);
  if (currentBranchOrNone(repository) !== defaultBranch) {
    runGit(repository.root, ["switch", defaultBranch], { code: "switch-failed", message: "Git default branch could not be checked out." });
  }
  const [merged, alreadyMerged] = mergeRegisteredBranches(repository, state, defaultBranch);
  requireClean(repository);
  return { branch: defaultBranch, head: currentHead(repository), merged, alreadyMerged };
}

/** 规范化发布版本和日期并形成标签。 */
export function releaseIdentity(repository, version, date) {
  if (!version || version !== version.trim() || version.toLowerCase().startsWith("v")) {
    throw new LifecycleError("invalid-version", "Version must be provided without a v prefix.");
  }
  if ([...version].some((character) => character.codePointAt(0) < 32) || version.length > 128) {
    throw new LifecycleError("invalid-version", "Version is invalid.");
  }
  if (!RELEASE_VERSION_RE.test(version)) {
    throw new LifecycleError("invalid-version", "Version must be a Harness timestamp or stable MAJOR.MINOR.PATCH with Minor/Patch in 0..99.");
  }
  const releaseDate = date ?? shanghaiDate();
  if (!/^\d{8}$/.test(releaseDate)) throw new LifecycleError("invalid-date", "Release date must be a valid YYYYMMDD value.");
  const parsed = new Date(`${releaseDate.slice(0, 4)}-${releaseDate.slice(4, 6)}-${releaseDate.slice(6)}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime()) || parsed.toISOString().slice(0, 10).replaceAll("-", "") !== releaseDate) {
    throw new LifecycleError("invalid-date", "Release date must be a valid YYYYMMDD value.");
  }
  const tag = `v${version}-${releaseDate}`;
  if (runGit(repository.root, ["check-ref-format", `refs/tags/${tag}`], { check: false }).returncode !== 0) {
    throw new LifecycleError("invalid-version", "Version cannot form a valid Git tag.");
  }
  return { tag, date: releaseDate, version };
}

/** 创建或复用固定本地标签，发布到此即已完成 Git 引用门禁。 */
function ensureLocalReleaseTag(repository, tag, head) {
  const target = localTagTarget(repository, tag);
  if (target !== null && target !== head) throw new LifecycleError("tag-conflict", "Local tag already points to a different commit.");
  if (target === null && runGit(repository.root, ["tag", tag, head], { check: false }).returncode !== 0) {
    throw new LifecycleError("tag-create-failed", "Release tag could not be created.");
  }
  if (localTagTarget(repository, tag) !== head) throw new LifecycleError("tag-verification-failed", "Local release tag did not match local HEAD.");
}

/** 比较稳定发布身份。 */
function releaseRecordMatchesIdentity(record, identity) {
  return ["tag", "date", "version"].every((key) => record[key] === identity[key]);
}

/** 发布中断后必须沿用同一上下文摘要。 */
function requireMatchingReleaseContext(record, args) {
  if (record.releaseContextSha256 !== args.releaseContextSha256) {
    throw new LifecycleError("release-context-conflict", "Release context differs from lifecycle state.");
  }
}

/** 只为新发布冻结一次已确认选择；发布中断后不重新读取当前偏好。 */
function configuredPostReleaseAction(repository) {
  try { return inspectPostReleaseAction(repository.root, { requireConfigured: true }).post_release_action; }
  catch { throw new LifecycleError("post-release-action-invalid", "A confirmed schema v4 post-release action is required before release."); }
}

/** 比较两个稳定三段版本；任一不是三段版本时返回 null。 */
function compareStableVersions(left, right) {
  const parse = (value) => /^(\d+)\.(\d+)\.(\d+)$/u.exec(value)?.slice(1).map(BigInt) ?? null;
  const [a, b] = [parse(left), parse(right)];
  if (a === null || b === null) return null;
  for (let index = 0; index < 3; index += 1) if (a[index] !== b[index]) return a[index] > b[index] ? 1 : -1;
  return 0;
}

/** 最终 HEAD 的更新日志必须合法且首条正是本次发布版本，防止带着缺失或陈旧日志打 tag。 */
function verifyReleaseNotesVersion(repository, head, version) {
  const blob = runGit(repository.root, ["show", `${head}:${RELEASE_NOTES_PATH}`], { check: false, bytes: true });
  try {
    if (blob.returncode !== 0) throw new Error("missing release notes");
    const document = parseReleaseNotesBytes(blob.stdout);
    if (document.releases[0].version !== `v${version}`) throw new Error("stale release notes");
  } catch {
    throw new LifecycleError("release-notes-mismatch", "Release notes at the final HEAD must be valid and lead with the released version.");
  }
}

/** 整合本地登记分支并冻结主分支最终 HEAD。 */
async function freezePendingReleaseHead(repository, state, reviewed) {
  const pending = state.cycle.pendingRelease;
  const prepared = prepareLocalRelease(repository, state);
  if (pending.postReleaseAction !== null && configuredPostReleaseAction(repository) !== pending.postReleaseAction) {
    throw new LifecycleError("post-release-action-changed", "Merged release policy differs from the frozen post-release action.");
  }
  verifyReleaseNotesVersion(repository, prepared.head, pending.version);
  verifyHeadReleaseContextBytes(repository, pending.releaseContextSha256, prepared.head);
  verifyReviewedFinalHead(repository, reviewed, prepared.head);
  await verifyFinalReleaseVersion(repository, pending.version);
  pending.head = prepared.head;
  saveState(repository, state);
  return pending;
}

/** 复读主分支和本地标签后完成发布；登记资源留待独立处置。 */
async function completePendingRelease(repository, state, args, reviewed) {
  let pending = state.cycle.pendingRelease;
  requireMatchingReleaseContext(pending, args);
  // 刚冻结的 HEAD 已在 freeze 中完成审查与版本复核；只有恢复既有冻结 HEAD 时才需在此重跑。
  const resumed = pending.head !== null;
  if (!resumed) pending = await freezePendingReleaseHead(repository, state, reviewed);
  const defaultBranch = state.defaultBranch;
  if (defaultBranch === null || !branchExists(repository, defaultBranch)) {
    throw new LifecycleError("local-state-changed", "Recorded Git default branch is unavailable.");
  }
  requireClean(repository);
  if (currentBranchOrNone(repository) !== defaultBranch) {
    runGit(repository.root, ["switch", defaultBranch], { code: "switch-failed", message: "Git default branch could not be checked out." });
  }
  if (currentHead(repository) !== pending.head) {
    throw new LifecycleError("local-state-changed", "Git default branch changed after release HEAD was frozen.");
  }
  if (resumed) {
    verifyReleaseNotesVersion(repository, pending.head, pending.version);
    verifyReviewedFinalHead(repository, reviewed, pending.head);
    await verifyFinalReleaseVersion(repository, pending.version);
  }
  ensureLocalReleaseTag(repository, pending.tag, pending.head);
  verifyLocalPosition(repository, defaultBranch, pending.head);
  const preservedBranches = state.cycle.branches.map((entry) => ({ ...entry }));
  const preservedWorktrees = state.cycle.worktrees.map((entry) => ({ ...entry }));
  state.releasedResources.push({
    tag: pending.tag, head: pending.head,
    branches: preservedBranches, worktrees: preservedWorktrees,
  });
  state.lastRelease = { ...pending, defaultBranch };
  state.cycle = null;
  saveState(repository, state);
  return {
    status: "released", branch: defaultBranch, head: pending.head,
    releaseContextSha256: pending.releaseContextSha256, worktree: repository.root, tag: pending.tag,
    preservedBranches: preservedBranches.map((entry) => entry.name),
    preservedWorktrees: preservedWorktrees.map((entry) => entry.path),
  };
}

/** 发布只整合本地主分支并复读本地 tag；不访问远端或清理资源。 */
export async function commandRelease(repository, args) {
  const sourceRepository = repository;
  const identity = releaseIdentity(repository, args.version, args.date);
  requireClean(repository);
  const context = await releaseContextBinding(repository, args.releaseContextSha256, identity);
  const harnessSource = await verifyHarnessVersionStamp(repository, identity.version);
  const state = loadState(repository);
  const reviewed = reviewedReleaseMetadata(repository, context, state);
  if (state.pendingPublish !== null) throw new LifecycleError("publish-in-progress", "A publication must finish before release.");
  repository = primaryRepository(repository);
  const cycle = state.cycle;
  if (cycle?.pendingRelease) {
    const pending = cycle.pendingRelease;
    if (!releaseRecordMatchesIdentity(pending, identity)) {
      throw new LifecycleError("release-in-progress", "A different release is already in progress.");
    }
    requireMatchingReleaseContext(pending, args);
    if (context.defaultBranch !== state.defaultBranch) {
      throw new LifecycleError("release-context-mismatch", "Release context default branch differs from lifecycle state.");
    }
    return completePendingRelease(repository, state, args, reviewed);
  }
  const last = state.lastRelease;
  if (cycle === null && last !== null && releaseRecordMatchesIdentity(last, identity)) {
    requireMatchingReleaseContext(last, args);
    if (context.defaultBranch !== last.defaultBranch) {
      throw new LifecycleError("release-context-mismatch", "Release context default branch differs from completed release.");
    }
    if (!branchExists(repository, last.defaultBranch)) {
      throw new LifecycleError("local-state-changed", "Released default branch is unavailable.");
    }
    const branchHead = runGit(repository.root, ["rev-parse", "--verify", "refs/heads/" + last.defaultBranch + "^{commit}"]).stdout.trim();
    if (branchHead !== last.head) throw new LifecycleError("local-state-changed", "Released default branch no longer matches the tag HEAD.");
    ensureLocalReleaseTag(repository, last.tag, last.head);
    return {
      status: "already-released", branch: last.defaultBranch, head: last.head,
      releaseContextSha256: last.releaseContextSha256, worktree: repository.root, tag: last.tag,
      preservedBranches: [], preservedWorktrees: [],
    };
  }
  // 下游新发布必须高于最近一次 Git 发布，不能以新日期 tag 重发同一或更低版本；Harness 时间版本由取号凭证约束。
  if (!harnessSource && last !== null && compareStableVersions(identity.version, last.version) !== 1) {
    throw new LifecycleError("release-version-not-newer", "Release version must be newer than the last completed Git release.");
  }
  requireClean(repository);
  const defaultBranch = state.defaultBranch ?? context.defaultBranch;
  if (defaultBranch !== context.defaultBranch) {
    throw new LifecycleError("release-context-mismatch", "Release context default branch differs from lifecycle state.");
  }
  if (!branchExists(repository, defaultBranch)) {
    throw new LifecycleError("local-default-unavailable", "Recorded local default branch is unavailable.");
  }
  const postReleaseAction = harnessSource ? null : configuredPostReleaseAction(sourceRepository);
  state.defaultBranch = defaultBranch;
  preflightCycleResources(repository, state);
  if (state.cycle === null) state.cycle = { branches: [], worktrees: [], pendingRelease: null };
  state.cycle.pendingRelease = {
    ...identity, head: null, releaseContextSha256: args.releaseContextSha256,
    postReleaseAction,
  };
  saveState(repository, state);
  return completePendingRelease(repository, state, args, reviewed);
}

const RELEASE_BRANCH = "release";

/** Harness 源按当次用户请求推送，须由已发布提交中的时间版本和初始化入口共同证明身份。 */
function isHarnessSourceRelease(repository, last) {
  if (!/^\d{12}$/u.test(last.version)) return false;
  const version = runGit(repository.root, ["show", `${last.head}:Version.md`], { check: false });
  const initialization = runGit(repository.root, ["cat-file", "-e", `${last.head}:${HARNESS_INITIALIZATION_SKILL}`], { check: false });
  return version.returncode === 0 &&
    version.stdout.includes(`- 当前版本：\`${last.version}\``) &&
    initialization.returncode === 0;
}

/** 判断引用是否是 release 分支的大小写变体；大小写不敏感文件系统会把它们解析为同一分支。 */
function isReleaseCaseVariant(reference) {
  return reference.toLowerCase() === `refs/heads/${RELEASE_BRANCH}` && reference !== `refs/heads/${RELEASE_BRANCH}`;
}

/** 本地存在旧大写 Release 等变体时拒绝，防止借大小写折叠复用或移动旧分支。 */
function assertNoLocalReleaseCaseVariant(repository) {
  const listed = runGit(repository.root, ["for-each-ref", "--format=%(refname)", "refs/heads"]).stdout.split(/\r?\n/u);
  if (listed.some(isReleaseCaseVariant)) {
    throw new LifecycleError("local-release-conflict", "A case variant of the release branch exists locally; the legacy Release branch must not be used.");
  }
}

/** 一次读取远端默认主分支、release 与 tag；要求 advertised HEAD 可解析且没有 release 大小写冲突。 */
function remoteReleaseRefs(repository, remote, tag) {
  const result = runGit(repository.root, ["ls-remote", "--symref", remote, "HEAD", "refs/heads/*", `refs/tags/${tag}`, `refs/tags/${tag}^{}`], { check: false });
  if (result.returncode !== 0) throw new LifecycleError("remote-read-failed", "Git remote release refs cannot be read.");
  let defaultBranch = null;
  let advertisedHead = null;
  const heads = new Map();
  let branch = null;
  let direct = null;
  let peeled = null;
  for (const line of result.stdout.split(/\r?\n/u)) {
    if (!line.includes("\t")) continue;
    const [oid, reference] = line.split("\t", 2);
    if (reference === "HEAD" && oid.startsWith("ref: refs/heads/")) {
      const name = oid.slice("ref: refs/heads/".length);
      if (defaultBranch !== null || !validBranch(repository, name)) {
        throw new LifecycleError("remote-default-unavailable", "Remote default branch is invalid.");
      }
      defaultBranch = name;
      continue;
    }
    if (reference === "HEAD") {
      if (advertisedHead !== null || !HEX_OID_RE.test(oid)) {
        throw new LifecycleError("remote-read-failed", "Git remote HEAD response is invalid.");
      }
      advertisedHead = oid;
      continue;
    }
    if (isReleaseCaseVariant(reference)) {
      throw new LifecycleError("remote-release-conflict", "Remote has a case variant of the release branch; the legacy Release branch must not be used.");
    }
    if (reference.startsWith("refs/heads/")) {
      if (!HEX_OID_RE.test(oid) || heads.has(reference.slice("refs/heads/".length))) {
        throw new LifecycleError("remote-read-failed", "Git remote branch response is invalid.");
      }
      heads.set(reference.slice("refs/heads/".length), oid);
    }
    if (reference !== `refs/heads/${RELEASE_BRANCH}` && reference !== `refs/tags/${tag}` && reference !== `refs/tags/${tag}^{}`) continue;
    if (!HEX_OID_RE.test(oid)) throw new LifecycleError("remote-read-failed", "Git remote release ref response is invalid.");
    if (reference === `refs/heads/${RELEASE_BRANCH}`) branch = oid;
    else if (reference === `refs/tags/${tag}`) direct = oid;
    else peeled = oid;
  }
  if (defaultBranch === null || !heads.has(defaultBranch) || heads.get(defaultBranch) !== advertisedHead) {
    throw new LifecycleError("remote-default-unavailable", "Remote advertised default branch cannot be resolved; no refs were pushed.");
  }
  return { defaultBranch, defaultHead: heads.get(defaultBranch), branch, tag: peeled ?? direct };
}

/** 精确读取本地 release 分支，不把任意本地引用解释为受管发布。 */
function localReleaseBranchTarget(repository) {
  const result = runGit(repository.root, ["rev-parse", "--verify", "refs/heads/release^{commit}"], { check: false });
  return result.returncode === 0 ? result.stdout.trim() : null;
}

/** 历史 v2 发布迁移后缺少 releasedResources，仍可从原 tag 与上下文证明旧 HEAD。 */
function isVerifiedHistoricalReleaseHead(repository, state, head) {
  const listed = runGit(repository.root, ["for-each-ref", "--points-at", head, "--format=%(refname)", "refs/tags"]);
  for (const ref of listed.stdout.split(/\r?\n/u)) {
    if (!ref.startsWith("refs/tags/v")) continue;
    const tag = ref.slice("refs/tags/".length);
    const blob = runGit(repository.root, ["show", `${ref}:${RELEASE_CONTEXT_PATH}`], { check: false, bytes: true });
    if (blob.returncode !== 0) continue;
    try {
      const context = validateContext(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(blob.stdout)));
      if (context.expectedTag === tag && context.defaultBranch === state.lastRelease.defaultBranch &&
          canonicalBytes(context).equals(blob.stdout)) return true;
    } catch { /* 不是受管发布上下文，不作为旧发布证据 */ }
  }
  return false;
}

/** 只移动由已记录发布 HEAD 形成的本地 release 分支。 */
function ensureLocalReleaseBranch(repository, state, head) {
  const previous = localReleaseBranchTarget(repository);
  if (previous === head) return;
  if (previous !== null) {
    if (runGit(repository.root, ["merge-base", "--is-ancestor", previous, head], { check: false }).returncode !== 0 ||
        !(state.releasedResources.some((released) => released.head === previous) ||
          isVerifiedHistoricalReleaseHead(repository, state, previous))) {
      throw new LifecycleError("local-release-conflict", "Local release branch does not point to a verified prior release.");
    }
    if (worktreeRecords(repository).some((record) => record.branch === RELEASE_BRANCH)) {
      throw new LifecycleError("local-release-conflict", "Local release branch is checked out in a worktree.");
    }
  }
  const expected = previous ?? "0".repeat(head.length);
  const updated = runGit(repository.root, ["update-ref", "refs/heads/release", head, expected], { check: false });
  if (updated.returncode !== 0 || localReleaseBranchTarget(repository) !== head) {
    throw new LifecycleError("local-release-conflict", "Local release branch could not be fixed at the published HEAD.");
  }
}

/** 用户在发布完成后独立选择一个远端，逐项同步默认主分支、release 和 tag 到冻结 HEAD。 */
export function commandPushRelease(repository, args) {
  repository = primaryRepository(repository);
  requireClean(repository);
  const state = loadState(repository);
  const last = state.lastRelease;
  if (last === null) throw new LifecycleError("no-release", "No completed local release is recorded.");
  if (last.postReleaseAction === null && !isHarnessSourceRelease(repository, last)) {
    throw new LifecycleError("post-release-action-unbound", "This historical release has no confirmed post-release action.");
  }
  if (last.postReleaseAction !== null && last.postReleaseAction !== "push_release_branch") {
    throw new LifecycleError("post-release-action-mismatch", "Frozen post-release action is not push_release_branch.");
  }
  if (state.cycle?.pendingRelease || state.pendingPublish !== null) {
    throw new LifecycleError("lifecycle-in-progress", "Another Git lifecycle operation must finish first.");
  }
  const remote = args.remote;
  if (!validRemote(remote)) throw new LifecycleError("invalid-argument", "Remote name is invalid.");
  if (!configuredRemotes(repository).includes(remote)) {
    throw new LifecycleError("remote-not-found", "Requested Git remote is not configured.");
  }
  const branch = RELEASE_BRANCH;
  if (!branchExists(repository, last.defaultBranch) ||
      runGit(repository.root, ["rev-parse", "--verify", "refs/heads/" + last.defaultBranch + "^{commit}"]).stdout.trim() !== last.head) {
    throw new LifecycleError("local-state-changed", "Released local default branch changed before push.");
  }
  const checkoutBranch = currentBranchOrNone(repository);
  const checkoutHead = currentHead(repository);
  if (localTagTarget(repository, last.tag) !== last.head) {
    throw new LifecycleError("tag-conflict", "Released local tag no longer matches the frozen HEAD.");
  }
  assertNoLocalReleaseCaseVariant(repository);
  const existing = remoteReleaseRefs(repository, remote, last.tag);
  const existingTag = existing.tag;
  if (existingTag !== null && existingTag !== last.head) {
    throw new LifecycleError("tag-conflict", "Remote tag already points to a different commit.");
  }
  ensureLocalReleaseBranch(repository, state, last.head);
  const defaultBranch = existing.defaultBranch;
  const defaultAlreadyMatched = existing.defaultHead === last.head;
  const unattemptedRefs = defaultBranch === branch ? "tag was not attempted" : "release branch and tag were not attempted";
  if (!defaultAlreadyMatched) {
    const pushed = runGit(repository.root, ["push", remote, last.head + ":refs/heads/" + defaultBranch], { check: false });
    let defaultTarget;
    try {
      defaultTarget = remoteBranchOid(repository, remote, defaultBranch);
    } catch {
      throw new LifecycleError("release-push-uncertain", `Remote default branch push outcome is uncertain; ${unattemptedRefs}; local release remains complete.`);
    }
    if (defaultTarget !== last.head) {
      throw new LifecycleError(pushed.returncode !== 0 ? "release-push-failed" : "release-push-uncertain",
        `Remote default branch push could not be confirmed; ${unattemptedRefs}; local release remains complete.`);
    }
  }
  let branchTarget = defaultBranch === branch ? last.head : existing.branch;
  const branchAlreadyMatched = branchTarget === last.head;
  if (!branchAlreadyMatched) {
    runGit(repository.root, ["push", remote, last.head + ":refs/heads/" + branch], { check: false });
    try {
      branchTarget = remoteBranchOid(repository, remote, branch);
    } catch {
      throw new LifecycleError("release-push-partial", "Remote default branch was confirmed, but release branch push outcome is uncertain; tag was not attempted; local release remains complete.");
    }
    if (branchTarget !== last.head) {
      throw new LifecycleError("release-push-partial",
        "Remote default branch was confirmed, but release branch push could not be confirmed; tag was not attempted; local release remains complete.");
    }
  }
  let tagTarget = existingTag;
  const tagAlreadyMatched = tagTarget === last.head;
  if (!tagAlreadyMatched) {
    const pushed = runGit(repository.root, ["push", remote, "refs/tags/" + last.tag + ":refs/tags/" + last.tag], { check: false });
    try {
      tagTarget = remoteTagTarget(repository, remote, last.tag);
    } catch {
      throw new LifecycleError("release-push-partial", "Remote default and release branches were confirmed, but tag push outcome is uncertain; local release remains complete.");
    }
    if (tagTarget !== last.head) {
      throw new LifecycleError("release-push-partial", "Remote default and release branches were confirmed, but tag push could not be confirmed; local release remains complete.");
    }
  }
  let confirmedBranch;
  let confirmedTag;
  let confirmedDefaultBranch;
  let confirmedDefaultHead;
  try {
    ({ branch: confirmedBranch, tag: confirmedTag, defaultBranch: confirmedDefaultBranch,
      defaultHead: confirmedDefaultHead } = remoteReleaseRefs(repository, remote, last.tag));
  } catch {
    throw new LifecycleError("release-push-uncertain", "Remote release refs could not be reread together; local release remains complete.");
  }
  if (confirmedDefaultBranch !== defaultBranch || confirmedDefaultHead !== last.head ||
      confirmedBranch !== last.head || confirmedTag !== last.head) {
    throw new LifecycleError("release-push-uncertain", "Remote release refs changed during final verification; local release remains complete.");
  }
  try {
    requireClean(repository);
    if (currentBranchOrNone(repository) !== checkoutBranch || currentHead(repository) !== checkoutHead ||
        !branchExists(repository, last.defaultBranch) ||
        runGit(repository.root, ["rev-parse", "--verify", "refs/heads/" + last.defaultBranch + "^{commit}"]).stdout.trim() !== last.head ||
        localTagTarget(repository, last.tag) !== last.head || localReleaseBranchTarget(repository) !== last.head) {
      throw new Error("released local refs or checkout changed");
    }
  } catch {
    throw new LifecycleError("release-push-uncertain", "Local release refs or checkout changed during push; recorded release remains complete.");
  }
  return {
    status: defaultAlreadyMatched && branchAlreadyMatched && tagAlreadyMatched ? "already-pushed" : "release-pushed",
    defaultBranch, branch, head: last.head, tag: last.tag, remote,
  };
}
