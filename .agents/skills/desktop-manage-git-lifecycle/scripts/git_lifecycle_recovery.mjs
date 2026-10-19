/** 以既有发布对象恢复跨机器落后的本机清单，不改写 Git 引用或追认历史动作。 */
import { createHash } from "node:crypto";
import { closeSync, existsSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { canonicalBytes, validateContext, verifyReviewScope } from "../../desktop-prepare-release/scripts/release_context.mjs";
import { parseDocumentBytes } from "../../desktop-prepare-release/scripts/release_notes.mjs";
import { check as checkVersion, Version } from "../../desktop-manage-version/scripts/version_gate.mjs";
import {
  HEX_OID_RE, LifecycleError, SHA256_RE, currentBranchOrNone, currentHead, localTagTarget,
  preflightCycleResources, primaryRepository, requireClean, runGit, saveState, statePath, validateState,
} from "./git_lifecycle_core.mjs";

/** 计算精确字节摘要，绑定恢复前清单和历史上下文。 */
function digest(bytes) { return createHash("sha256").update(bytes).digest("hex"); }

/** 读取历史普通文件；拒绝符号链接和不完整提交对象。 */
function blob(repository, head, relative) {
  const mode = runGit(repository.root, ["ls-tree", head, "--", relative]).stdout;
  if (!/^100(?:644|755) blob /u.test(mode)) throw new Error("not a regular tracked file");
  return runGit(repository.root, ["show", `${head}:${relative}`], { bytes: true }).stdout;
}

/** 验证祖先关系，允许默认主分支已正常吸收发布后的维护。 */
function requireAncestor(repository, ancestor, head) {
  if (runGit(repository.root, ["merge-base", "--is-ancestor", ancestor, head], { check: false }).returncode !== 0) {
    throw new Error("release ancestry differs");
  }
}

/** 核对历史 tag 的上下文、版本、日志及已审查源码身份。 */
function historicalRelease(repository, args) {
  try {
    const head = localTagTarget(repository, args.tag);
    if (head !== args.sourceCommit) throw new Error("tag differs");
    const bytes = blob(repository, head, ".harness/release-context.json");
    if (digest(bytes) !== args.releaseContextSha256) throw new Error("context digest differs");
    const context = validateContext(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)));
    if (!canonicalBytes(context).equals(bytes) || context.expectedTag !== args.tag) throw new Error("context differs");
    Version.parse(context.version);
    verifyReviewScope(repository.root, context);
    requireAncestor(repository, context.sourceHead, head);
    const changed = runGit(repository.root, ["diff", "--name-only", "--no-renames", "-z", context.sourceHead, head, "--"])
      .stdout.split("\0").filter(Boolean);
    if (changed.some((name) => !["release-notes.json", ".harness/release-context.json"].includes(name))) {
      throw new Error("unreviewed source differs");
    }
    const notes = parseDocumentBytes(blob(repository, head, "release-notes.json"));
    if (notes.releases[0].version !== `v${context.version}` || notes.releases[0].releaseDate !== context.releaseDate) {
      throw new Error("release notes differ");
    }
    const historicalState = JSON.parse(blob(repository, head, ".harness/version-state.json").toString("utf8"));
    const cargo = blob(repository, head, "Cargo.toml").toString("utf8");
    const sections = cargo.split(/(?=^\[)/mu).filter((section) => section.startsWith("[workspace.package]"));
    const versions = [...(sections[0] ?? "").matchAll(/^version\s*=\s*"([^"\r\n]+)"\s*$/gmu)];
    if (sections.length !== 1 || versions.length !== 1 || versions[0][1] !== context.version || historicalState.target_version !== context.version) {
      throw new Error("historical version differs");
    }
    return {
      tag: args.tag, head, version: context.version, date: context.releaseDate.replaceAll("-", ""),
      defaultBranch: context.defaultBranch, releaseContextSha256: args.releaseContextSha256,
      postReleaseAction: null,
    };
  } catch {
    throw new LifecycleError("recovery-release-unverifiable", "Historical tag, context, review, notes or version could not be verified.");
  }
}

/** 在私有 common-dir 保存原清单精确字节；已有同摘要备份必须为相同普通文件。 */
function backupState(repository, bytes) {
  const directory = join(dirname(statePath(repository)), "recovery-audits");
  if (existsSync(directory)) {
    const stat = lstatSync(directory);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error("unsafe audit directory");
  } else mkdirSync(directory, { mode: 0o700 });
  const backup = join(directory, `${digest(bytes)}.json`);
  if (existsSync(backup)) {
    const stat = lstatSync(backup);
    if (stat.isSymbolicLink() || !stat.isFile() || !readFileSync(backup).equals(bytes)) throw new Error("backup differs");
  } else {
    const descriptor = openSync(backup, "wx", 0o600);
    try { writeFileSync(descriptor, bytes); fsyncSync(descriptor); }
    finally { closeSync(descriptor); }
  }
  return backup;
}

/** 只在明确授权及两端事实一致时补齐落后的 lastRelease；保留所有精确资源和周期。 */
export function commandRecoverRelease(repository, args) {
  if (args.recoveryApproved !== "yes" || !HEX_OID_RE.test(args.sourceCommit ?? "") ||
      !SHA256_RE.test(args.expectedStateSha256 ?? "") || !SHA256_RE.test(args.releaseContextSha256 ?? "") ||
      !/^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d?)\.(?:0|[1-9]\d?)-\d{8}$/u.test(args.tag ?? "")) {
    throw new LifecycleError("invalid-argument", "Recovery requires explicit approval and exact tag, commit and digests.");
  }
  requireClean(repository);
  repository = primaryRepository(repository);
  requireClean(repository);
  if (!existsSync(statePath(repository))) {
    throw new LifecycleError("recovery-state-conflict", "Recovery requires an existing lifecycle inventory.");
  }
  const metadata = lstatSync(statePath(repository));
  if (metadata.isSymbolicLink() || !metadata.isFile() || metadata.size > 1_048_576) {
    throw new LifecycleError("recovery-state-conflict", "Recovery requires a bounded regular lifecycle inventory.");
  }
  // 同一份原字节同时作为校验、备份及写回对象来源，避免读取间覆盖外部资源登记。
  const bytes = readFileSync(statePath(repository));
  let state;
  try { state = validateState(repository, JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes))); }
  catch { throw new LifecycleError("recovery-state-conflict", "Recovery requires a valid schema 4 lifecycle inventory."); }
  if (state.pendingPublish !== null || state.cycle?.pendingRelease) {
    throw new LifecycleError("lifecycle-in-progress", "Finish pending Git operations before recovery.");
  }
  preflightCycleResources(repository, state);
  const checkoutHead = currentHead(repository);
  const checkoutBranch = currentBranchOrNone(repository);
  const recovered = historicalRelease(repository, args);
  try {
    if (state.defaultBranch !== recovered.defaultBranch) throw new Error("branch differs");
    const current = checkVersion(repository.root, "release");
    const versionState = JSON.parse(blob(repository, "HEAD", ".harness/version-state.json").toString("utf8"));
    if (current.current_version !== recovered.version || versionState.last_release?.version !== recovered.version ||
        versionState.last_release?.source_commit !== recovered.head || versionState.pending_changes.length !== 0) {
      throw new Error("current finalized release differs");
    }
    requireAncestor(repository, recovered.head, `refs/heads/${recovered.defaultBranch}`);
    requireAncestor(repository, recovered.head, "HEAD");
    const tags = runGit(repository.root, ["tag", "--list", `v${recovered.version}-*`]).stdout.trim().split(/\r?\n/u).filter(Boolean);
    if (tags.some((tag) => localTagTarget(repository, tag) !== recovered.head)) throw new Error("same-version tags differ");
    if (state.lastRelease?.tag === recovered.tag) {
      if (["tag", "head", "version", "date", "defaultBranch", "releaseContextSha256"]
        .some((field) => state.lastRelease[field] !== recovered[field])) throw new Error("recorded identity differs");
      return { status: "already-recovered", head: recovered.head, tag: recovered.tag };
    }
    if (state.lastRelease === null || Version.parse(recovered.version).compare(Version.parse(state.lastRelease.version)) <= 0 ||
        localTagTarget(repository, state.lastRelease.tag) !== state.lastRelease.head) throw new Error("old release differs");
    requireAncestor(repository, state.lastRelease.head, recovered.head);
  } catch {
    throw new LifecycleError("recovery-state-conflict", "Current version state, default branch or recorded release conflicts with recovery.");
  }
  if (digest(bytes) !== args.expectedStateSha256) {
    throw new LifecycleError("recovery-state-changed", "Lifecycle state changed after recovery review.");
  }
  const mainHead = runGit(repository.root, ["rev-parse", `refs/heads/${recovered.defaultBranch}`]).stdout.trim();
  let backup;
  try { backup = backupState(repository, bytes); }
  catch { throw new LifecycleError("recovery-backup-failed", "Original lifecycle bytes could not be preserved; no state was changed."); }
  // 生命周期锁保护协作写入；保存前仍复读原字节，拒绝未遵循锁的外部修改。
  requireClean(repository);
  if (!readFileSync(statePath(repository)).equals(bytes) || localTagTarget(repository, args.tag) !== recovered.head ||
      runGit(repository.root, ["rev-parse", `refs/heads/${recovered.defaultBranch}`]).stdout.trim() !== mainHead ||
      currentHead(repository) !== checkoutHead || currentBranchOrNone(repository) !== checkoutBranch) {
    throw new LifecycleError("recovery-state-changed", "Lifecycle state changed before recovery could be saved.");
  }
  state.lastRelease = recovered;
  saveState(repository, state);
  return { status: "release-recovered", head: recovered.head, tag: recovered.tag, backup, postReleaseAction: null };
}
