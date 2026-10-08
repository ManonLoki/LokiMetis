/** Git 生命周期的仓库、状态、锁和登记资源基础设施。 */

import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmdirSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";

export const SCHEMA_VERSION = 4;
export const SUMMARY_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
export const REMOTE_RE = /^[A-Za-z0-9][A-Za-z0-9._/-]*$/;
export const HEX_OID_RE = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
export const SHA256_RE = /^[0-9a-f]{64}$/;
export const STATE_DIRECTORY = "agent-first-harness";
export const STATE_FILENAME = "git-lifecycle.json";
const LOCK_DIRECTORY = ".git-lifecycle.lock";
const LOCK_TIMEOUT_MILLISECONDS = 30_000;
const LOCK_POLL_MILLISECONDS = 50;
const RELEASE_CONTEXT_PATH = ".harness/release-context.json";

/** 携带稳定错误码和可公开消息，避免泄露 Git 子进程细节。 */
export class LifecycleError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "LifecycleError";
    this.code = code;
    this.publicMessage = message;
  }
}

/** 构造禁止终端或凭据管理器交互的 Git 子进程环境。 */
function gitEnvironment() {
  return {
    ...process.env,
    GIT_TERMINAL_PROMPT: "0",
    GCM_INTERACTIVE: "Never",
    SSH_ASKPASS_REQUIRE: "never",
  };
}

/** 非交互执行 Git，只用调用方提供的脱敏描述失败。 */
export function runGit(
  cwd,
  args,
  { check = true, code = "git-error", message = "Git operation failed.", bytes = false } = {},
) {
  const result = spawnSync("git", ["-C", cwd, ...args], {
    env: gitEnvironment(),
    encoding: bytes ? undefined : "utf8",
    input: Buffer.alloc(0),
    timeout: 60_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error || (check && result.status !== 0)) {
    throw new LifecycleError(code, message);
  }
  return {
    returncode: result.status ?? 1,
    stdout: result.stdout,
    stderr: result.stderr,
  };
}

/** 解析独立 Git 工作树及共享 common-dir。 */
export function resolveRepository(projectRoot) {
  let requested;
  try {
    requested = realpathSync(resolve(projectRoot));
  } catch {
    throw new LifecycleError("invalid-project-root", "Project root is unavailable.");
  }
  const top = runGit(requested, ["rev-parse", "--show-toplevel"], {
    code: "not-a-repository",
    message: "Project root is not a Git worktree.",
  }).stdout.trim();
  const common = runGit(requested, ["rev-parse", "--path-format=absolute", "--git-common-dir"], {
    code: "not-a-repository",
    message: "Git common directory is unavailable.",
  }).stdout.trim();
  try {
    const root = realpathSync(top);
    const commonDir = realpathSync(isAbsolute(common) ? common : join(root, common));
    return { root, commonDir };
  } catch {
    throw new LifecycleError("not-a-repository", "Git repository paths are unavailable.");
  }
}

/** 返回 common-dir 内唯一生命周期状态路径。 */
export function statePath(repository) {
  return join(repository.commonDir, STATE_DIRECTORY, STATE_FILENAME);
}

/** 构造尚未开始发布周期的规范状态。 */
export function newState() {
  return {
    schemaVersion: SCHEMA_VERSION,
    remote: null,
    defaultBranch: null,
    cycle: null,
    pendingPublish: null,
    lastRelease: null,
    releasedResources: [],
  };
}

/** 本进程内已由 Git 判定过的分支名结果；状态每次加载/保存都会复核全部历史分支，避免重复子进程。 */
const branchFormatCache = new Map();

/** 使用 Git 自身规则验证具名分支并拒绝伪引用。 */
export function validBranch(repository, branch) {
  if (
    typeof branch !== "string" || branch.length === 0 || branch.startsWith("-") ||
    branch === "@" || branch === "HEAD" || [...branch].some((character) => character.codePointAt(0) < 32)
  ) return false;
  const key = `${repository.root}\0${branch}`;
  if (!branchFormatCache.has(key)) {
    branchFormatCache.set(key, runGit(repository.root, ["check-ref-format", "--branch", branch], { check: false }).returncode === 0);
  }
  return branchFormatCache.get(key);
}

/** 验证远端名可安全作为独立子进程参数使用。 */
export function validRemote(remote) {
  return typeof remote === "string" && REMOTE_RE.test(remote) && !remote.includes("..") && !remote.endsWith("/");
}

/** 校验冻结的本地发布身份；推送位置不属于发布记录。 */
function validateReleaseRecord(record, label, { pending }) {
  const keys = pending
    ? ["tag", "head", "date", "version", "releaseContextSha256", "postReleaseAction"]
    : ["tag", "head", "date", "version", "releaseContextSha256", "defaultBranch", "postReleaseAction"];
  if (!record || typeof record !== "object" || Array.isArray(record) ||
      Object.keys(record).sort().join("|") !== keys.sort().join("|")) {
    throw new LifecycleError("state-invalid", `${label} state is invalid.`);
  }
  for (const key of ["tag", "date", "version", "releaseContextSha256"]) {
    if (typeof record[key] !== "string") throw new LifecycleError("state-invalid", `${label} state is invalid.`);
  }
  if (!/^\d{8}$/.test(record.date) || record.version.length === 0 ||
      record.tag !== `v${record.version}-${record.date}`) {
    throw new LifecycleError("state-invalid", `${label} identity is invalid.`);
  }
  if (!SHA256_RE.test(record.releaseContextSha256)) {
    throw new LifecycleError("state-invalid", `${label} state is invalid.`);
  }
  if (record.postReleaseAction !== null &&
      !["local_package", "push_release_branch"].includes(record.postReleaseAction)) {
    throw new LifecycleError("state-invalid", `${label} post-release action is invalid.`);
  }
  if (record.head === null) {
    if (!pending) throw new LifecycleError("state-invalid", `${label} state is invalid.`);
  } else if (typeof record.head !== "string" || !HEX_OID_RE.test(record.head)) {
    throw new LifecycleError("state-invalid", `${label} state is invalid.`);
  }
  if (!pending && (typeof record.defaultBranch !== "string" || record.defaultBranch.length === 0)) {
    throw new LifecycleError("state-invalid", `${label} state is invalid.`);
  }
}

/** 校验精确登记的开发分支与 Worktree，不推断名称前缀。 */
function validateResources(repository, branches, worktrees, label) {
  if (!Array.isArray(branches) || !Array.isArray(worktrees)) {
    throw new LifecycleError("state-invalid", `${label} resources are invalid.`);
  }
  const branchNames = new Set();
  for (const branch of branches) {
    if (!branch || typeof branch !== "object" || Array.isArray(branch) ||
        Object.keys(branch).sort().join("|") !== "createdAt|name|summary" ||
        !validBranch(repository, branch.name) || branchNames.has(branch.name) ||
        (branch.summary !== null && (typeof branch.summary !== "string" || !SUMMARY_RE.test(branch.summary))) ||
        typeof branch.createdAt !== "string") {
      throw new LifecycleError("state-invalid", `${label} branch state is invalid.`);
    }
    branchNames.add(branch.name);
  }
  const worktreePaths = new Set();
  for (const worktree of worktrees) {
    if (!worktree || typeof worktree !== "object" || Array.isArray(worktree) ||
        Object.keys(worktree).sort().join("|") !== "branch|path" ||
        typeof worktree.path !== "string" || !isAbsolute(worktree.path) || worktreePaths.has(worktree.path) ||
        typeof worktree.branch !== "string" || !branchNames.has(worktree.branch)) {
      throw new LifecycleError("state-invalid", `${label} worktree state is invalid.`);
    }
    worktreePaths.add(worktree.path);
  }
}

/** 校验一次 publish 冻结的 HEAD、目标顺序和进度。 */
function validatePendingPublish(repository, state) {
  const pending = state.pendingPublish;
  if (pending === null) return;
  if (!pending || typeof pending !== "object" || Array.isArray(pending) ||
      Object.keys(pending).sort().join("|") !== "head|targets") {
    throw new LifecycleError("state-invalid", "Pending publication state is invalid.");
  }
  if (typeof pending.head !== "string" || !HEX_OID_RE.test(pending.head) ||
      !Array.isArray(pending.targets) || pending.targets.length === 0) {
    throw new LifecycleError("state-invalid", "Pending publication targets are invalid.");
  }
  const seen = new Set();
  let reachedUnconfirmed = false;
  for (const target of pending.targets) {
    if (!target || typeof target !== "object" || Array.isArray(target) ||
        Object.keys(target).sort().join("|") !== "branch|confirmed|remote" ||
        !validRemote(target.remote) || seen.has(target.remote) ||
        !validBranch(repository, target.branch) || typeof target.confirmed !== "boolean") {
      throw new LifecycleError("state-invalid", "Pending publication target is invalid.");
    }
    if (reachedUnconfirmed && target.confirmed) {
      throw new LifecycleError("state-invalid", "Pending publication progress is invalid.");
    }
    reachedUnconfirmed ||= !target.confirmed;
    seen.add(target.remote);
  }
  const primary = pending.targets[0];
  if (state.remote !== primary.remote || state.defaultBranch !== primary.branch) {
    throw new LifecycleError("state-invalid", "Pending publication primary target is invalid.");
  }
}

/** 严格校验当前周期与已发布资源的精确清单。 */
export function validateState(repository, state) {
  const expected = ["schemaVersion", "remote", "defaultBranch", "cycle", "pendingPublish", "lastRelease", "releasedResources"];
  if (!state || typeof state !== "object" || Array.isArray(state) ||
      Object.keys(state).sort().join("|") !== expected.sort().join("|") ||
      state.schemaVersion !== SCHEMA_VERSION) {
    throw new LifecycleError("state-invalid", "Lifecycle state schema is invalid.");
  }
  if (state.remote !== null && !validRemote(state.remote)) {
    throw new LifecycleError("state-invalid", "Lifecycle remote is invalid.");
  }
  if (state.defaultBranch !== null && !validBranch(repository, state.defaultBranch)) {
    throw new LifecycleError("state-invalid", "Lifecycle default branch is invalid.");
  }
  if (state.lastRelease !== null) {
    validateReleaseRecord(state.lastRelease, "Last release", { pending: false });
    if (!validBranch(repository, state.lastRelease.defaultBranch)) {
      throw new LifecycleError("state-invalid", "Last release default branch is invalid.");
    }
  }
  validatePendingPublish(repository, state);
  if (!Array.isArray(state.releasedResources)) {
    throw new LifecycleError("state-invalid", "Released resource inventory is invalid.");
  }
  const releasedTags = new Set();
  for (const released of state.releasedResources) {
    if (!released || typeof released !== "object" || Array.isArray(released) ||
        Object.keys(released).sort().join("|") !== "branches|head|tag|worktrees" ||
        typeof released.tag !== "string" || releasedTags.has(released.tag) ||
        typeof released.head !== "string" || !HEX_OID_RE.test(released.head)) {
      throw new LifecycleError("state-invalid", "Released resource entry is invalid.");
    }
    releasedTags.add(released.tag);
    validateResources(repository, released.branches, released.worktrees, "Released");
  }
  const cycle = state.cycle;
  if (cycle === null) return state;
  if (!cycle || typeof cycle !== "object" || Array.isArray(cycle) ||
      Object.keys(cycle).sort().join("|") !== "branches|pendingRelease|worktrees" ||
      !Array.isArray(cycle.branches) || !Array.isArray(cycle.worktrees)) {
    throw new LifecycleError("state-invalid", "Lifecycle cycle is invalid.");
  }
  validateResources(repository, cycle.branches, cycle.worktrees, "Registered");
  const pending = cycle.pendingRelease;
  if (pending !== null) {
    if (state.pendingPublish !== null) {
      throw new LifecycleError("state-invalid", "Publication and release cannot both be pending.");
    }
    validateReleaseRecord(pending, "Pending release", { pending: true });
    if (state.defaultBranch === null) {
      throw new LifecycleError("state-invalid", "Pending release default branch is invalid.");
    }
  }
  return state;
}

/** 只迁移没有进行中副作用的 v2；已完成发布仍由原标签实证。 */
function migrateCompletedV2(repository, legacy) {
  const expected = ["schemaVersion", "remote", "defaultBranch", "cycle", "pendingPublish", "lastRelease"];
  if (!legacy || typeof legacy !== "object" || Array.isArray(legacy) ||
      Object.keys(legacy).sort().join("|") !== expected.sort().join("|") || legacy.schemaVersion !== 2) {
    throw new LifecycleError("state-invalid", "Lifecycle state schema is invalid.");
  }
  if (legacy.pendingPublish !== null || legacy.cycle?.pendingRelease != null) {
    throw new LifecycleError("legacy-inflight-unsupported", "Finish or repair the existing v2 Git operation before using the new lifecycle.");
  }
  const state = newState();
  state.remote = legacy.remote;
  state.defaultBranch = legacy.defaultBranch;
  if (legacy.cycle !== null) {
    if (!legacy.cycle || typeof legacy.cycle !== "object" || Array.isArray(legacy.cycle) ||
        Object.keys(legacy.cycle).sort().join("|") !== "branches|pendingRelease|worktrees" ||
        !Array.isArray(legacy.cycle.branches) || !Array.isArray(legacy.cycle.worktrees)) {
      throw new LifecycleError("state-invalid", "Legacy cycle state is invalid.");
    }
    const branches = legacy.cycle.branches.map((branch) => {
      if (!branch || typeof branch !== "object" || Array.isArray(branch) ||
          Object.keys(branch).sort().join("|") !== "createdAt|localDeleted|name|remoteDeleted|summary" ||
          branch.localDeleted !== false || branch.remoteDeleted !== false) {
        throw new LifecycleError("state-invalid", "Legacy branch state is invalid.");
      }
      return { name: branch.name, summary: branch.summary, createdAt: branch.createdAt };
    });
    state.cycle = { branches, worktrees: legacy.cycle.worktrees, pendingRelease: null };
  }
  if (legacy.lastRelease !== null) {
    const last = legacy.lastRelease;
    const keys = ["tag", "head", "date", "version", "gitPublication", "remote", "releaseContextSha256"];
    if (!last || typeof last !== "object" || Array.isArray(last) ||
        Object.keys(last).sort().join("|") !== keys.sort().join("|") ||
        !["local", "remote"].includes(last.gitPublication) ||
        (last.gitPublication === "local" ? last.remote !== null : !validRemote(last.remote))) {
      throw new LifecycleError("state-invalid", "Legacy release state is invalid.");
    }
    if (legacy.defaultBranch === null || !branchExists(repository, legacy.defaultBranch) ||
        localTagTarget(repository, last.tag) !== last.head ||
        runGit(repository.root, ["merge-base", "--is-ancestor", last.head, `refs/heads/${legacy.defaultBranch}`],
          { check: false }).returncode !== 0) {
      throw new LifecycleError("legacy-release-unverifiable", "Completed v2 release tag and default branch could not be verified.");
    }
    const contextBlob = runGit(repository.root, ["show", `${last.tag}:${RELEASE_CONTEXT_PATH}`], { check: false, bytes: true });
    if (contextBlob.returncode !== 0 ||
        createHash("sha256").update(contextBlob.stdout).digest("hex") !== last.releaseContextSha256) {
      throw new LifecycleError("legacy-release-unverifiable", "Completed v2 release context digest could not be verified from its tag.");
    }
    state.lastRelease = {
      tag: last.tag, head: last.head, date: last.date, version: last.version,
      releaseContextSha256: last.releaseContextSha256, defaultBranch: legacy.defaultBranch,
      postReleaseAction: null,
    };
  }
  return validateState(repository, state);
}

/** 静止的 v3 发布没有动作快照：标记为未绑定，不改释进行中的旧操作。 */
function migrateV3(repository, legacy) {
  if ((legacy.pendingPublish !== undefined && legacy.pendingPublish !== null) ||
      (legacy.cycle?.pendingRelease !== undefined && legacy.cycle?.pendingRelease !== null)) {
    throw new LifecycleError("legacy-inflight-unsupported", "Finish or repair the existing v3 Git operation before using the new lifecycle.");
  }
  const actionless = (record) => {
    if (record === null) return null;
    if (!record || typeof record !== "object" || Array.isArray(record) ||
        Object.hasOwn(record, "postReleaseAction")) {
      throw new LifecycleError("state-invalid", "Legacy release state is invalid.");
    }
    return { ...record, postReleaseAction: null };
  };
  const migrated = {
    ...legacy,
    schemaVersion: SCHEMA_VERSION,
    lastRelease: actionless(legacy.lastRelease),
    cycle: legacy.cycle === null ? null : {
      ...legacy.cycle,
      pendingRelease: actionless(legacy.cycle?.pendingRelease),
    },
  };
  return validateState(repository, migrated);
}

/** 加载 common-dir 状态；缺失时只返回内存默认值。 */
export function loadState(repository) {
  const path = statePath(repository);
  if (!existsSync(path)) return newState();
  let metadata;
  try {
    metadata = lstatSync(path);
    if (metadata.isSymbolicLink() || !metadata.isFile()) throw new Error("unsafe");
    if (metadata.size > 1_048_576) throw new LifecycleError("state-invalid", "Lifecycle state is too large.");
    const parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(readFileSync(path)));
    const legacy = ["schemaVersion", "remote", "defaultBranch", "cycle", "lastRelease"];
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed) &&
        Object.keys(parsed).sort().join("|") === legacy.sort().join("|") && parsed.schemaVersion === 2) {
      parsed.pendingPublish = null;
    }
    if (parsed.schemaVersion === 2) return migrateCompletedV2(repository, parsed);
    if (parsed.schemaVersion === 3) return migrateV3(repository, parsed);
    return validateState(repository, parsed);
  } catch (error) {
    if (error instanceof LifecycleError) throw error;
    throw new LifecycleError("state-invalid", "Lifecycle state cannot be read.");
  }
}

/** 在同目录排他临时文件中写入并原子替换状态。 */
export function saveState(repository, state) {
  validateState(repository, state);
  const path = statePath(repository);
  const directory = dirname(path);
  try {
    if (existsSync(directory)) {
      const metadata = lstatSync(directory);
      if (metadata.isSymbolicLink() || !metadata.isDirectory()) throw new Error("unsafe");
    }
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    let temporary;
    let descriptor;
    try {
      for (let attempt = 0; attempt < 100; attempt += 1) {
        temporary = join(directory, `.git-lifecycle-${process.pid}-${randomBytes(8).toString("hex")}`);
        try {
          descriptor = openSync(temporary, "wx", 0o600);
          break;
        } catch (error) {
          if (error.code !== "EEXIST") throw error;
        }
      }
      if (descriptor === undefined) throw new Error("collision");
      writeFileSync(descriptor, `${JSON.stringify(state, null, 2)}\n`, "utf8");
      fsyncSync(descriptor);
      closeSync(descriptor);
      descriptor = undefined;
      renameSync(temporary, path);
      temporary = undefined;
    } finally {
      if (descriptor !== undefined) closeSync(descriptor);
      if (temporary && existsSync(temporary)) unlinkSync(temporary);
    }
  } catch (error) {
    if (error instanceof LifecycleError) throw error;
    throw new LifecycleError("state-write-failed", "Lifecycle state cannot be saved.");
  }
}

/** 串行化同一 common-dir 的生命周期写入。 */
export async function withLifecycleStateLock(repository, callback) {
  const directory = dirname(statePath(repository));
  try {
    if (existsSync(directory)) {
      const metadata = lstatSync(directory);
      if (metadata.isSymbolicLink() || !metadata.isDirectory()) throw new Error("unsafe");
    }
    mkdirSync(directory, { recursive: true, mode: 0o700 });
  } catch {
    throw new LifecycleError("state-lock-failed", "Lifecycle state lock cannot be created.");
  }
  const lock = join(directory, LOCK_DIRECTORY);
  const deadline = Date.now() + LOCK_TIMEOUT_MILLISECONDS;
  while (true) {
    try {
      mkdirSync(lock, { mode: 0o700 });
      break;
    } catch (error) {
      if (error.code !== "EEXIST") {
        throw new LifecycleError("state-lock-failed", "Lifecycle state lock cannot be created.");
      }
      const metadata = lstatSync(lock);
      if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
        throw new LifecycleError("state-lock-failed", "Lifecycle state lock is unsafe.");
      }
      if (Date.now() >= deadline) {
        throw new LifecycleError("lifecycle-locked", "Another Git lifecycle operation is still running or left a lock to inspect.");
      }
      await new Promise((accept) => setTimeout(accept, LOCK_POLL_MILLISECONDS));
    }
  }
  let failed = false;
  try {
    return await callback();
  } catch (error) {
    failed = true;
    throw error;
  } finally {
    try {
      rmdirSync(lock);
    } catch {
      if (!failed) {
        throw new LifecycleError("state-lock-release-failed", "Lifecycle operation finished but its state lock could not be removed.");
      }
    }
  }
}

/** 读取具名当前分支；分离 HEAD 返回 null。 */
export function currentBranchOrNone(repository, cwd = repository.root) {
  const result = runGit(cwd, ["symbolic-ref", "--quiet", "--short", "HEAD"], { check: false });
  const branch = result.stdout.trim();
  if (result.returncode !== 0) return null;
  if (!validBranch(repository, branch)) throw new LifecycleError("git-error", "Current Git branch is invalid.");
  return branch;
}

/** 读取必须具名的当前分支。 */
export function currentBranch(repository, cwd = repository.root) {
  const branch = currentBranchOrNone(repository, cwd);
  if (branch === null) throw new LifecycleError("detached-head", "A named current branch is required.");
  return branch;
}

/** 读取当前提交 OID。 */
export function currentHead(repository, cwd = repository.root) {
  const head = runGit(cwd, ["rev-parse", "--verify", "HEAD^{commit}"]).stdout.trim();
  if (!HEX_OID_RE.test(head)) throw new LifecycleError("git-error", "Current Git HEAD is invalid.");
  return head;
}

/** 检查全部已跟踪、暂存与未跟踪变化。 */
export function isClean(repository, cwd = repository.root) {
  return runGit(cwd, ["status", "--porcelain=v1", "--untracked-files=all"]).stdout === "";
}

/** 拒绝会受切分支、合并或清理影响的脏数据。 */
export function requireClean(repository, cwd = repository.root) {
  if (!isClean(repository, cwd)) throw new LifecycleError("dirty-worktree", "Git worktree has uncommitted changes.");
}

/** 复读 push 后的当前分支、HEAD 和干净状态。 */
export function verifyLocalPosition(repository, branch, head) {
  if (currentBranch(repository) !== branch || currentHead(repository) !== head || !isClean(repository)) {
    throw new LifecycleError("local-state-changed", "Local Git state changed during the push operation.");
  }
}

/** 精确检查本地分支。 */
export function branchExists(repository, branch) {
  return runGit(repository.root, ["show-ref", "--verify", "--quiet", `refs/heads/${branch}`], { check: false }).returncode === 0;
}

/** 读取配置中的远端名。 */
export function configuredRemotes(repository) {
  return runGit(repository.root, ["remote"]).stdout.split(/\r?\n/).filter(Boolean);
}

/** 优先使用显式或已登记远端。 */
export function selectRemote(repository, state, explicit, { required }) {
  const remotes = configuredRemotes(repository);
  const stored = state.remote;
  if (explicit !== undefined && explicit !== null) {
    if (!validRemote(explicit)) throw new LifecycleError("invalid-argument", "Remote name is invalid.");
    if (stored !== null && stored !== explicit) {
      throw new LifecycleError("remote-conflict", "Requested Git remote differs from lifecycle state.");
    }
    if (!remotes.includes(explicit)) throw new LifecycleError("remote-not-found", "Requested Git remote is not configured.");
    return explicit;
  }
  if (stored !== null) {
    if (!remotes.includes(stored) && required) throw new LifecycleError("remote-not-found", "Lifecycle Git remote is not configured.");
    return stored;
  }
  if (remotes.includes("origin")) return "origin";
  if (remotes.length === 1) return remotes[0];
  if (required) {
    throw new LifecycleError(remotes.length === 0 ? "remote-required" : "remote-ambiguous", "A unique Git remote is required for this operation.");
  }
  return null;
}

/** 只读已存在的远端跟踪 HEAD。 */
export function localRemoteDefault(repository, remote) {
  const result = runGit(repository.root, ["symbolic-ref", "--quiet", "--short", `refs/remotes/${remote}/HEAD`], { check: false });
  const prefix = `${remote}/`;
  const value = result.stdout.trim();
  if (result.returncode === 0 && value.startsWith(prefix)) {
    const branch = value.slice(prefix.length);
    if (validBranch(repository, branch)) return branch;
  }
  return null;
}

/** 从远端 HEAD 符号引用解析默认分支。 */
export function remoteDefaultBranch(repository, remote) {
  const result = runGit(repository.root, ["ls-remote", "--symref", remote, "HEAD"], { check: false });
  if (result.returncode !== 0) throw new LifecycleError("remote-read-failed", "Git remote default branch cannot be read.");
  for (const line of result.stdout.split(/\r?\n/)) {
    if (line.startsWith("ref: refs/heads/") && line.endsWith("\tHEAD")) {
      const branch = line.slice("ref: refs/heads/".length, -"\tHEAD".length);
      if (validBranch(repository, branch)) return branch;
    }
  }
  throw new LifecycleError("remote-default-unavailable", "Git remote does not advertise a default branch.");
}

/** 校验显式补充远端并预读默认分支。 */
export function resolveAdditionalRemoteTargets(repository, primaryRemote, additionalRemotes) {
  const configured = new Set(configuredRemotes(repository));
  const seen = new Set([primaryRemote]);
  return additionalRemotes.map((remote) => {
    if (!validRemote(remote)) throw new LifecycleError("invalid-argument", "Additional Git remote name is invalid.");
    if (seen.has(remote)) {
      throw new LifecycleError("invalid-argument", "Additional Git remotes must be distinct from the primary remote and each other.");
    }
    if (!configured.has(remote)) throw new LifecycleError("remote-not-found", "Additional Git remote is not configured.");
    seen.add(remote);
    return { remote, branch: remoteDefaultBranch(repository, remote) };
  });
}

/** 精确复读一个远端分支 OID。 */
export function remoteBranchOid(repository, remote, branch) {
  const result = runGit(repository.root, ["ls-remote", "--heads", remote, `refs/heads/${branch}`], { check: false });
  if (result.returncode !== 0) throw new LifecycleError("remote-read-failed", "Git remote branch cannot be read.");
  const lines = result.stdout.split(/\r?\n/).filter(Boolean);
  if (lines.length === 0) return null;
  const oid = lines[0].split("\t", 1)[0];
  if (lines.length !== 1 || !HEX_OID_RE.test(oid)) throw new LifecycleError("remote-read-failed", "Git remote branch response is invalid.");
  return oid;
}

/** 精确读取远端标签最终指向的提交。 */
export function remoteTagTarget(repository, remote, tag) {
  const result = runGit(repository.root, ["ls-remote", "--tags", remote, `refs/tags/${tag}`, `refs/tags/${tag}^{}`], { check: false });
  if (result.returncode !== 0) throw new LifecycleError("remote-read-failed", "Git remote tag cannot be read.");
  let direct = null;
  let peeled = null;
  for (const line of result.stdout.split(/\r?\n/)) {
    if (!line.includes("\t")) continue;
    const [oid, reference] = line.split("\t", 2);
    if (!HEX_OID_RE.test(oid)) throw new LifecycleError("remote-read-failed", "Git remote tag response is invalid.");
    if (reference === `refs/tags/${tag}`) direct = oid;
    if (reference === `refs/tags/${tag}^{}`) peeled = oid;
  }
  return peeled ?? direct;
}

/** 读取本地同名标签最终提交。 */
export function localTagTarget(repository, tag) {
  if (runGit(repository.root, ["show-ref", "--verify", "--quiet", `refs/tags/${tag}`], { check: false }).returncode !== 0) return null;
  const oid = runGit(repository.root, ["rev-parse", "--verify", `refs/tags/${tag}^{commit}`]).stdout.trim();
  if (!HEX_OID_RE.test(oid)) throw new LifecycleError("tag-conflict", "Existing local tag target is invalid.");
  return oid;
}

/** 以 NUL 分隔格式读取共享仓库 Worktree。 */
export function worktreeRecords(repository) {
  const raw = runGit(repository.root, ["worktree", "list", "--porcelain", "-z"]).stdout;
  const records = [];
  let current = {};
  for (const field of raw.split("\0")) {
    if (field === "") {
      if (Object.keys(current).length > 0) records.push(current);
      current = {};
    } else if (field.startsWith("worktree ")) {
      if (Object.keys(current).length > 0) records.push(current);
      current = { path: field.slice("worktree ".length) };
    } else if (field.startsWith("branch refs/heads/")) {
      current.branch = field.slice("branch refs/heads/".length);
    } else if (field === "detached") current.detached = "true";
  }
  if (Object.keys(current).length > 0) records.push(current);
  if (records.length === 0 || !records[0].path) throw new LifecycleError("git-error", "Git worktree inventory is unavailable.");
  return records;
}

/** 按 strict 选择规范化路径。 */
export function canonicalPath(value, { strict }) {
  try {
    const absolute = resolve(value);
    if (strict || existsSync(absolute)) return realpathSync(absolute);
    const suffix = [];
    let parent = absolute;
    while (!existsSync(parent)) {
      const next = dirname(parent);
      if (next === parent) throw new Error("no existing parent");
      suffix.push(basename(parent));
      parent = next;
    }
    return join(realpathSync(parent), ...suffix.reverse());
  } catch {
    throw new LifecycleError("not-a-worktree", "Requested worktree path is unavailable.");
  }
}

/** 把主分支写入稳定路由到 primary Worktree。 */
export function primaryRepository(repository) {
  const primary = canonicalPath(worktreeRecords(repository)[0].path, { strict: true });
  if (primary === repository.root) return repository;
  const resolved = resolveRepository(primary);
  if (resolved.commonDir !== repository.commonDir) throw new LifecycleError("repository-mismatch", "Primary worktree belongs to another repository.");
  return resolved;
}

/** 在任何主分支写入前确认登记资源仍安全。 */
export function preflightCycleResources(repository, state) {
  const cycle = state.cycle;
  if (cycle === null) return;
  const registeredBranches = new Set(cycle.branches.map((entry) => entry.name));
  for (const entry of cycle.branches) {
    if (!branchExists(repository, entry.name)) {
      throw new LifecycleError("registered-branch-missing", "A registered development branch is missing before publication.");
    }
  }
  const records = worktreeRecords(repository);
  const primary = canonicalPath(records[0].path, { strict: true });
  const inventory = new Map(records.map((record) => [canonicalPath(record.path, { strict: false }), record]));
  const registeredPaths = new Map(cycle.worktrees.map((entry) => [entry.path, entry.branch]));
  for (const [path, branch] of registeredPaths) {
    const registered = canonicalPath(path, { strict: false });
    if (registered === primary) throw new LifecycleError("cleanup-safety", "Registered cleanup path is the primary Git worktree.");
    const record = inventory.get(registered);
    if (!record) continue;
    if (record.detached === "true" || record.branch !== branch) throw new LifecycleError("ownership-conflict", "Registered worktree ownership changed.");
    if (branch === state.defaultBranch) throw new LifecycleError("cleanup-safety", "Default branch worktree cannot be removed.");
    if (existsSync(registered)) requireClean(repository, registered);
  }
  for (const [path, record] of inventory) {
    if (path === primary || !registeredBranches.has(record.branch)) continue;
    if (registeredPaths.get(path) !== record.branch) {
      throw new LifecycleError("unregistered-worktree", "A registered development branch is checked out in an unregistered worktree.");
    }
  }
}

const SHANGHAI_CLOCK = new Intl.DateTimeFormat("en-CA", {
  timeZone: "Asia/Shanghai", year: "numeric", month: "2-digit", day: "2-digit",
  hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23",
});

/** 返回当前时刻在固定上海时区的各日期时间字段。 */
function shanghaiParts() {
  return Object.fromEntries(SHANGHAI_CLOCK.formatToParts(new Date()).map(({ type, value }) => [type, value]));
}

/** 返回固定上海时区的 YYYYMMDD。 */
export function shanghaiDate() {
  const value = shanghaiParts();
  return `${value.year}${value.month}${value.day}`;
}

/** 返回带 +08:00 的秒级时间戳。 */
function shanghaiTimestamp() {
  const value = shanghaiParts();
  return `${value.year}-${value.month}-${value.day}T${value.hour}:${value.minute}:${value.second}+08:00`;
}

/** 自动登记当前非主 Worktree。 */
function ensureCurrentWorktreeTracked(repository, cycle, branch) {
  const records = worktreeRecords(repository);
  const primary = canonicalPath(records[0].path, { strict: true });
  const current = canonicalPath(repository.root, { strict: true });
  if (current === primary) return [false, false];
  const inventory = new Map(records.map((record) => [canonicalPath(record.path, { strict: true }), record]));
  const currentRecord = inventory.get(current);
  if (!currentRecord || currentRecord.detached === "true" || currentRecord.branch !== branch) {
    throw new LifecycleError("ownership-conflict", "Current Git worktree ownership could not be confirmed.");
  }
  for (const existing of cycle.worktrees) {
    if (existing.path === current) {
      if (existing.branch !== branch) throw new LifecycleError("ownership-conflict", "Current Git worktree conflicts with an existing lifecycle registration.");
      return [true, false];
    }
    if (existing.branch === branch) throw new LifecycleError("ownership-conflict", "Current Git worktree conflicts with an existing lifecycle registration.");
  }
  cycle.worktrees.push({ path: current, branch });
  return [true, true];
}

/** 只读报告当前 Git 与生命周期状态。 */
export function commandInspect(repository, args) {
  const state = loadState(repository);
  const remote = selectRemote(repository, state, args.remote, { required: false });
  const branch = currentBranchOrNone(repository);
  let defaultBranch = state.defaultBranch;
  if (defaultBranch === null && remote !== null) defaultBranch = localRemoteDefault(repository, remote);
  if (defaultBranch === null && state.cycle === null && branch !== null) defaultBranch = branch;
  return {
    status: "inspected", branch, head: currentHead(repository), clean: isClean(repository),
    remote, defaultBranch, statePath: statePath(repository), state,
  };
}

/** 核对最近一次本地发布冻结的后续动作及其主分支和标签身份。 */
export function commandCheckPostRelease(repository, args) {
  if (!["local_package", "push_release_branch"].includes(args.action)) {
    throw new LifecycleError("invalid-argument", "Post-release action is invalid.");
  }
  const state = loadState(repository);
  if ((state.cycle !== null && state.cycle.pendingRelease !== null) || state.pendingPublish !== null) {
    throw new LifecycleError("lifecycle-in-progress", "Another Git lifecycle operation must finish first.");
  }
  const last = state.lastRelease;
  if (last === null) throw new LifecycleError("no-release", "No completed local release is recorded.");
  if (last.postReleaseAction === null) {
    throw new LifecycleError("post-release-action-unbound", "This release predates a frozen post-release action.");
  }
  if (last.postReleaseAction !== args.action) {
    throw new LifecycleError("post-release-action-mismatch", "The frozen release action differs from the requested action.");
  }
  if (!branchExists(repository, last.defaultBranch) ||
      runGit(repository.root, ["rev-parse", "--verify", `refs/heads/${last.defaultBranch}^{commit}`]).stdout.trim() !== last.head) {
    throw new LifecycleError("local-state-changed", "Released default branch no longer matches its frozen HEAD.");
  }
  if (localTagTarget(repository, last.tag) !== last.head) {
    throw new LifecycleError("tag-conflict", "Released tag no longer matches its frozen HEAD.");
  }
  return {
    status: "post-release-action-confirmed", action: last.postReleaseAction,
    branch: last.defaultBranch, head: last.head, tag: last.tag,
  };
}

/** 从当前 HEAD 建立唯一开发分支并登记。 */
export function commandStart(repository, args) {
  const summary = args.summary;
  if (!SUMMARY_RE.test(summary)) throw new LifecycleError("invalid-summary", "Summary must be lowercase ASCII kebab-case.");
  const state = loadState(repository);
  if (state.pendingPublish !== null) throw new LifecycleError("publish-in-progress", "A publication must finish before new development.");
  const branch = currentBranchOrNone(repository);
  let cycle = state.cycle;
  if (cycle !== null) {
    if (cycle.pendingRelease !== null) throw new LifecycleError("release-in-progress", "A release cleanup must finish before new development.");
    if (branch !== null) {
      const record = cycle.branches.find((item) => item.name === branch && item.summary === summary);
      if (record && branchExists(repository, branch)) {
        const [worktreeTracked, changed] = ensureCurrentWorktreeTracked(repository, cycle, branch);
        if (changed) saveState(repository, state);
        return { status: "already-started", branch, head: currentHead(repository), remote: state.remote,
          defaultBranch: state.defaultBranch, worktreeTracked };
      }
    }
  }
  requireClean(repository);
  const remote = selectRemote(repository, state, args.remote, { required: false });
  const originalHead = currentHead(repository);
  let startHead = originalHead;
  let baseReference = null;
  if (cycle === null && state.lastRelease !== null) {
    const released = state.lastRelease;
    if (state.defaultBranch !== released.defaultBranch || !branchExists(repository, released.defaultBranch) ||
        localTagTarget(repository, released.tag) !== released.head) {
      throw new LifecycleError("release-baseline-changed", "The last released default branch and tag must be reconciled before new development.");
    }
    baseReference = `refs/heads/${released.defaultBranch}`;
    startHead = runGit(repository.root, ["rev-parse", "--verify", `${baseReference}^{commit}`]).stdout.trim();
    if (startHead !== released.head) {
      throw new LifecycleError("release-baseline-changed", "The last released default branch changed before new development.");
    }
  }
  const base = `feature-${summary}-${shanghaiDate()}`;
  let candidate = base;
  for (let suffix = 2; branchExists(repository, candidate); suffix += 1) candidate = `${base}-${suffix}`;
  runGit(repository.root, ["switch", "-c", candidate, ...(baseReference === null ? [] : [baseReference])], {
    code: "branch-create-failed", message: "Development branch could not be created.",
  });
  try {
    if (cycle === null) {
      cycle = { branches: [], worktrees: [], pendingRelease: null };
      state.cycle = cycle;
    }
    if (state.defaultBranch === null) state.defaultBranch = (remote ? localRemoteDefault(repository, remote) : null) ?? branch;
    if (remote !== null) state.remote = remote;
    cycle.branches.push({ name: candidate, summary, createdAt: shanghaiTimestamp() });
    const [worktreeTracked] = ensureCurrentWorktreeTracked(repository, cycle, candidate);
    saveState(repository, state);
    return { status: "started", branch: candidate, head: startHead, remote, defaultBranch: state.defaultBranch, worktreeTracked };
  } catch (error) {
    runGit(repository.root, branch === null ? ["switch", "--detach", originalHead] : ["switch", branch], { check: false });
    runGit(repository.root, ["branch", "-D", "--", candidate], { check: false });
    throw error;
  }
}

/** 验证并登记同一 common-dir 的非主 Worktree。 */
export function commandTrackWorktree(repository, args) {
  if (!isAbsolute(args.worktree)) throw new LifecycleError("invalid-argument", "Worktree path must be absolute.");
  const state = loadState(repository);
  if (state.pendingPublish !== null) throw new LifecycleError("publish-in-progress", "A publication must finish before tracking worktrees.");
  const cycle = state.cycle;
  if (cycle === null) throw new LifecycleError("no-active-cycle", "Start a development cycle before tracking a worktree.");
  if (cycle.pendingRelease !== null) throw new LifecycleError("release-in-progress", "A release must finish before tracking worktrees.");
  const remote = selectRemote(repository, state, args.remote, { required: false });
  const target = canonicalPath(args.worktree, { strict: true });
  const records = worktreeRecords(repository);
  const normalized = new Map(records.map((record) => [canonicalPath(record.path, { strict: true }), record]));
  if (!normalized.has(target)) throw new LifecycleError("not-a-worktree", "Requested path is not a registered Git worktree.");
  const primary = canonicalPath(records[0].path, { strict: true });
  if (target === primary) throw new LifecycleError("primary-worktree", "Primary Git worktree cannot be tracked as a cycle resource.");
  if (resolveRepository(target).commonDir !== repository.commonDir) throw new LifecycleError("repository-mismatch", "Worktree belongs to a different Git repository.");
  const record = normalized.get(target);
  if (record.detached === "true" || !record.branch) throw new LifecycleError("detached-head", "Tracked worktree must have a named branch.");
  const branch = record.branch;
  if (branch === state.defaultBranch) throw new LifecycleError("default-branch", "Default branch worktree cannot be tracked for cleanup.");
  if (!validBranch(repository, branch)) throw new LifecycleError("detached-head", "Tracked worktree must have a valid named branch.");
  let changed = false;
  for (const existing of cycle.worktrees) {
    if (existing.path === target && existing.branch !== branch) throw new LifecycleError("ownership-conflict", "Worktree path is already registered to another branch.");
    if (existing.branch === branch && existing.path !== target) throw new LifecycleError("ownership-conflict", "Worktree branch is already registered at another path.");
  }
  const branchRecord = cycle.branches.find((item) => item.name === branch);
  if (!branchRecord) {
    cycle.branches.push({ name: branch, summary: null, createdAt: shanghaiTimestamp() });
    changed = true;
  }
  if (!cycle.worktrees.some((item) => item.path === target)) {
    cycle.worktrees.push({ path: target, branch });
    changed = true;
  }
  if (remote !== null && state.remote !== remote) {
    state.remote = remote;
    changed = true;
  }
  if (changed) saveState(repository, state);
  return { status: changed ? "worktree-tracked" : "worktree-already-tracked", branch, worktree: target, remote: state.remote };
}

/** 切换到已获取的远端默认分支。 */
export function switchToDefault(repository, remote, defaultBranch) {
  if (currentBranchOrNone(repository) === defaultBranch) return;
  if (branchExists(repository, defaultBranch)) {
    runGit(repository.root, ["switch", defaultBranch], { code: "switch-failed", message: "Git default branch could not be checked out." });
  } else {
    runGit(repository.root, ["switch", "-c", defaultBranch, "--track", `${remote}/${defaultBranch}`], {
      code: "switch-failed", message: "Git default branch could not be checked out.",
    });
  }
}

/** 把仍存在的登记分支普通合并到默认分支。 */
export function mergeRegisteredBranches(repository, state, defaultBranch) {
  const merged = [];
  const alreadyMerged = [];
  if (state.cycle === null) return [merged, alreadyMerged];
  for (const record of state.cycle.branches) {
    const branch = record.name;
    if (branch === defaultBranch) {
      alreadyMerged.push(branch);
      continue;
    }
    const before = currentHead(repository);
    const result = runGit(repository.root, ["merge", "--no-edit", `refs/heads/${branch}`], { check: false });
    if (result.returncode !== 0) throw new LifecycleError("merge-failed", "Git merge did not complete; inspect the worktree state.");
    (currentHead(repository) === before ? alreadyMerged : merged).push(branch);
  }
  return [merged, alreadyMerged];
}
