#!/usr/bin/env node
/** Write and verify the tracked context for one Git release. */

import {
  chmodSync,
  closeSync,
  existsSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  realpathSync,
  renameSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { createHash, randomBytes } from "node:crypto";
import { spawnSync } from "node:child_process";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  isHarnessSource,
  latestHarnessTagVersion,
  readHarnessVersion,
  readHarnessVersionStamp,
  validHarnessTimestamp,
} from "./harness_version_clock.mjs";
import {
  loadState as loadLifecycleState,
  resolveRepository as resolveLifecycleRepository,
} from "../../desktop-manage-git-lifecycle/scripts/git_lifecycle_core.mjs";

export const OID_PATTERN = /^[0-9a-f]{40}$/;
export const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const VERSION_PATTERN = /^(?:[0-9]+\.[0-9]+\.[0-9]+|[0-9]{12})$/;
export const CONTEXT_RELATIVE_PATH = ".harness/release-context.json";
export const REVIEW_CHECKS = [
  "behavior-correctness",
  "core-adapter-boundary",
  "external-contracts",
  "responsibility-and-size",
  "temporary-markers",
];
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true });

/** 表示发布上下文或其 Git 发布绑定不满足固定契约。 */
export class ReleaseContextError extends Error {
  /** 保存可公开错误，避免调用方依赖运行时异常类型。 */
  constructor(message) {
    super(message);
    this.name = "ReleaseContextError";
  }
}

/** 以非交互环境执行 Git，并按调用方选择返回文本或原始字节。 */
export function runGit(root, args, { check = true, text = true } = {}) {
  const result = spawnSync("git", ["-C", root, ...args], {
    encoding: text ? "utf8" : undefined,
    env: {
      ...process.env,
      GIT_TERMINAL_PROMPT: "0",
      GCM_INTERACTIVE: "Never",
    },
    input: Buffer.alloc(0),
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) {
    throw new ReleaseContextError("git executable is unavailable");
  }
  if (check && result.status !== 0) {
    const stderr = text ? result.stderr : result.stderr.toString("utf8");
    const stdout = text ? result.stdout : result.stdout.toString("utf8");
    const detail = stderr.trim() || stdout.trim() || "unknown Git failure";
    throw new ReleaseContextError(`git ${args.join(" ")} failed: ${detail}`);
  }
  return {
    returncode: result.status ?? 1,
    stdout: result.stdout,
    stderr: result.stderr,
  };
}

/** 要求调用路径恰好是非符号链接的独立 Git 顶层。 */
export function resolveRoot(projectRoot) {
  const supplied = resolve(projectRoot);
  try {
    if (lstatSync(supplied).isSymbolicLink()) {
      throw new ReleaseContextError("project root must not be a symbolic link");
    }
  } catch (error) {
    if (error instanceof ReleaseContextError) throw error;
    throw new ReleaseContextError("project root does not exist");
  }
  let root;
  try {
    root = realpathSync(supplied);
  } catch {
    throw new ReleaseContextError("project root does not exist");
  }
  if (!statSync(root).isDirectory()) {
    throw new ReleaseContextError("project root is not a directory");
  }
  const result = runGit(root, ["rev-parse", "--is-inside-work-tree", "--show-toplevel"]);
  const lines = result.stdout.trimEnd().split(/\r?\n/);
  let top;
  try {
    top = realpathSync(lines[1]);
  } catch {
    top = "";
  }
  if (lines.length !== 2 || lines[0] !== "true" || top !== root) {
    throw new ReleaseContextError("project root must equal the independent Git top level");
  }
  return root;
}

/** 校验固定长度的小写 Git OID。 */
export function validateOid(value, field) {
  if (typeof value !== "string" || !OID_PATTERN.test(value)) {
    throw new ReleaseContextError(`${field} must be a 40-character lowercase Git OID`);
  }
  return value;
}

/** 校验可公开写入证据的短文本。 */
export function publicText(value, field) {
  if (
    typeof value !== "string" ||
    value !== value.trim() ||
    value.length === 0 ||
    value.length > 500 ||
    [...value].some((character) => {
      const code = character.codePointAt(0);
      return code < 32 || code === 127;
    })
  ) {
    throw new ReleaseContextError(`${field} must be 1-500 trimmed printable characters`);
  }
  return value;
}

/** 拒绝 revision 语法与无法唯一表示分支的伪引用。 */
export function validBranchName(value) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.startsWith("-") ||
    value === "@" ||
    value === "HEAD" ||
    value.endsWith("/") ||
    value.endsWith(".") ||
    value.includes("..") ||
    value.includes("@{") ||
    value.includes("//")
  ) {
    return false;
  }
  const forbidden = new Set([" ", "~", "^", ":", "?", "*", "[", "\\"]);
  if ([...value].some((character) => {
    const code = character.codePointAt(0);
    return code < 32 || code === 127 || forbidden.has(character);
  })) {
    return false;
  }
  return value.split("/").every(
    (component) => component && !component.startsWith(".") && !component.endsWith(".lock"),
  );
}

/** 确认对象只包含固定字段。 */
function hasExactKeys(value, keys) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length && actual.every((key, index) => key === expected[index]);
}

/** 校验单次发布的语义审查选择，不把它扩张为分支历史门禁。 */
export function validateReleaseReview(value, sourceHead) {
  const required = [
    "selection", "status", "scopeBase", "sourceHead", "scopeDiffSha256",
    "reviewedSourceCommit", "checks", "evidenceSummary", "reason", "remainingRisk",
  ];
  if (!hasExactKeys(value, required)) {
    throw new ReleaseContextError("releaseReview fields are invalid");
  }
  if (value.sourceHead !== sourceHead) {
    throw new ReleaseContextError("releaseReview.sourceHead must equal sourceHead");
  }
  const scopeBase = validateOid(value.scopeBase, "releaseReview.scopeBase");
  if (typeof value.scopeDiffSha256 !== "string" || !SHA256_PATTERN.test(value.scopeDiffSha256)) {
    throw new ReleaseContextError("releaseReview.scopeDiffSha256 must be lowercase SHA-256");
  }
  let evidence;
  let reviewed;
  let checks;
  let reason;
  let remainingRisk;
  if (value.selection === "enabled") {
    if (
      value.status !== "passed" ||
      value.reviewedSourceCommit !== sourceHead ||
      JSON.stringify(value.checks) !== JSON.stringify(REVIEW_CHECKS) ||
      value.reason !== null ||
      value.remainingRisk !== null
    ) {
      throw new ReleaseContextError("enabled releaseReview is inconsistent");
    }
    evidence = publicText(value.evidenceSummary, "releaseReview.evidenceSummary");
    reviewed = sourceHead;
    checks = [...REVIEW_CHECKS];
    reason = null;
    remainingRisk = null;
  } else if (value.selection === "disabled") {
    if (
      value.status !== "Not run" ||
      value.reviewedSourceCommit !== null ||
      !Array.isArray(value.checks) ||
      value.checks.length !== 0 ||
      value.evidenceSummary !== null
    ) {
      throw new ReleaseContextError("disabled releaseReview is inconsistent");
    }
    evidence = null;
    reviewed = null;
    checks = [];
    reason = publicText(value.reason, "releaseReview.reason");
    remainingRisk = publicText(value.remainingRisk, "releaseReview.remainingRisk");
  } else {
    throw new ReleaseContextError("releaseReview.selection is invalid");
  }
  return {
    selection: value.selection,
    status: value.status,
    scopeBase,
    sourceHead,
    scopeDiffSha256: value.scopeDiffSha256,
    reviewedSourceCommit: reviewed,
    checks,
    evidenceSummary: evidence,
    reason,
    remainingRisk,
  };
}

/** 规范化完整发布上下文并拒绝未知或缺失字段。 */
export function validateContext(value) {
  const required = [
    "schemaVersion", "sourceHead", "version", "releaseDate",
    "expectedTag", "defaultBranch", "releaseReview",
  ];
  if (!hasExactKeys(value, required) || value.schemaVersion !== 3) {
    throw new ReleaseContextError("release context fields or schemaVersion are invalid");
  }
  const sourceHead = validateOid(value.sourceHead, "sourceHead");
  if (typeof value.version !== "string" || !VERSION_PATTERN.test(value.version) || value.version.slice(0, 1).toLowerCase() === "v") {
    throw new ReleaseContextError("version must be non-empty, safe, and omit the v prefix");
  }
  if (typeof value.releaseDate !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(value.releaseDate)) {
    throw new ReleaseContextError("releaseDate must use YYYY-MM-DD");
  }
  const parsed = new Date(`${value.releaseDate}T00:00:00Z`);
  if (Number.isNaN(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== value.releaseDate) {
    throw new ReleaseContextError("releaseDate must be a valid YYYY-MM-DD date");
  }
  const compactDate = value.releaseDate.replaceAll("-", "");
  const expectedTag = `v${value.version}-${compactDate}`;
  if (value.expectedTag !== expectedTag) {
    throw new ReleaseContextError("expectedTag does not match v{version}-{YYYYMMDD}");
  }
  if (!validBranchName(value.defaultBranch)) {
    throw new ReleaseContextError("defaultBranch is invalid");
  }
  return {
    schemaVersion: 3,
    sourceHead,
    version: value.version,
    releaseDate: value.releaseDate,
    expectedTag,
    defaultBranch: value.defaultBranch,
    releaseReview: validateReleaseReview(value.releaseReview, sourceHead),
  };
}

/** 复算已启用语义审查的源码范围，摘要取固定 Git diff 的原始字节。 */
export function verifyReviewScope(root, context) {
  const review = context.releaseReview;
  if (review.selection !== "enabled") return;
  validateOid(context.sourceHead, "sourceHead");
  validateOid(review.scopeBase, "releaseReview.scopeBase");
  const ancestry = runGit(root, [
    "merge-base", "--is-ancestor", review.scopeBase, context.sourceHead,
  ], { check: false });
  if (ancestry.returncode !== 0) {
    throw new ReleaseContextError("enabled releaseReview.scopeBase must be an ancestor of sourceHead");
  }
  const diff = runGit(root, [
    "-c", "core.quotePath=true",
    "diff", "--binary", "--full-index", "--no-renames", "--no-ext-diff", "--no-textconv",
    "--no-color", "--no-relative", "--src-prefix=a/", "--dst-prefix=b/",
    "--diff-algorithm=myers", "--no-indent-heuristic", "--unified=3",
    "--inter-hunk-context=0", "--submodule=short", "--ignore-submodules=none",
    review.scopeBase, context.sourceHead, "--",
  ], { text: false }).stdout;
  const actual = createHash("sha256").update(diff).digest("hex");
  if (actual !== review.scopeDiffSha256) {
    throw new ReleaseContextError("enabled releaseReview.scopeDiffSha256 does not match the Git diff");
  }
}

/** Harness 的发布版本只能取当前 Version.md 的上海时间版本。 */
function assertHarnessVersionBinding(root, version, { requireStamp = false } = {}) {
  const initializationSkill = join(root, ".agents", "skills", "desktop-instantiate-project", "SKILL.md");
  if (!isHarnessSource(root)) {
    if (existsSync(initializationSkill) || (typeof version === "string" && /^\d{12}$/.test(version))) {
      throw new ReleaseContextError("Harness timestamp release requires Version.md and the active instantiate skill");
    }
    return;
  }
  let declared;
  try {
    declared = readHarnessVersion(root);
  } catch (error) {
    throw new ReleaseContextError(error.message);
  }
  if (!validHarnessTimestamp(version) || version !== declared) {
    throw new ReleaseContextError("Harness release context version must equal Version.md current YYYYMMDDHHMM version");
  }
  if (requireStamp) {
    try { readHarnessVersionStamp(root, version); }
    catch (error) { throw new ReleaseContextError(error.message); }
  }
}

/** 生成跨平台固定 LF 与两个空格缩进的规范 JSON 字节。 */
export function canonicalBytes(value) {
  return Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
}

/** 读取并规范校验发布上下文，返回规范字节与摘要。 */
export function loadContext(root) {
  const path = join(root, CONTEXT_RELATIVE_PATH);
  let metadata;
  try {
    metadata = lstatSync(path);
  } catch {
    throw new ReleaseContextError("release context must be a non-symbolic regular file");
  }
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    throw new ReleaseContextError("release context must be a non-symbolic regular file");
  }
  const raw = readFileSync(path);
  let value;
  try {
    value = JSON.parse(UTF8_DECODER.decode(raw));
  } catch {
    throw new ReleaseContextError("release context must be valid UTF-8 JSON");
  }
  const normalized = validateContext(value);
  const canonical = canonicalBytes(normalized);
  const normalizedRaw = Buffer.from(
    UTF8_DECODER.decode(raw).replaceAll("\r\n", "\n"),
    "utf8",
  );
  if (!normalizedRaw.equals(canonical)) {
    throw new ReleaseContextError("release context must use canonical formatting");
  }
  return {
    value: normalized,
    canonical,
    digest: createHash("sha256").update(canonical).digest("hex"),
  };
}

/** 读取一个精确本地引用最终指向的提交。 */
export function localRefOid(root, reference) {
  const result = runGit(root, ["rev-parse", "--verify", `${reference}^{commit}`], { check: false });
  const oid = result.stdout.trim();
  if (result.returncode !== 0 || !OID_PATTERN.test(oid)) {
    throw new ReleaseContextError(`local ref is missing or invalid: ${reference}`);
  }
  return oid;
}

/** 以提交祖先关系证明已审查源码及仍登记的分支均进入最终标签。 */
function requireAncestor(root, ancestor, head, label) {
  const result = runGit(root, ["merge-base", "--is-ancestor", ancestor, head], { check: false });
  if (result.returncode !== 0) {
    throw new ReleaseContextError(`${label} is not included in the release tag commit`);
  }
}

/** common-dir 登记若存在，必须与真实 Git 发布一致且没有进行中操作。 */
function verifyLifecycleRelease(root, context, digest, head) {
  let state;
  try {
    state = loadLifecycleState(resolveLifecycleRepository(root));
  } catch (error) {
    throw new ReleaseContextError(`Git lifecycle state is invalid: ${error.message}`);
  }
  if (state.pendingPublish !== null || (state.cycle !== null && state.cycle.pendingRelease !== null)) {
    throw new ReleaseContextError("Git lifecycle has an unfinished publication or release journal");
  }
  const last = state.lastRelease;
  if (last?.tag === context.expectedTag) {
    if (
      last.head !== head || last.defaultBranch !== context.defaultBranch ||
      last.releaseContextSha256 !== digest || last.version !== context.version ||
      last.date !== context.releaseDate.replaceAll("-", "")
    ) {
      throw new ReleaseContextError("Git lifecycle completed release differs from the local tag or context");
    }
    // 下一开发周期不会改变已完成发布的 Git 身份。
    return;
  }
  for (const branch of state.cycle?.branches ?? []) {
    const branchHead = localRefOid(root, `refs/heads/${branch.name}`);
    requireAncestor(root, branchHead, head, `registered branch ${branch.name}`);
  }
}

/** 根据 CLI 选择构造固定发布审查对象。 */
function buildReview(args) {
  const enabled = args.reviewSelection === "enabled";
  return {
    selection: args.reviewSelection,
    status: enabled ? "passed" : "Not run",
    scopeBase: args.scopeBase,
    sourceHead: args.sourceHead,
    scopeDiffSha256: args.scopeDiffSha256,
    reviewedSourceCommit: enabled ? args.sourceHead : null,
    checks: enabled ? [...REVIEW_CHECKS] : [],
    evidenceSummary: enabled ? (args.reviewEvidenceSummary ?? null) : null,
    reason: enabled ? null : (args.reviewReason ?? null),
    remainingRisk: enabled ? null : (args.reviewRemainingRisk ?? null),
  };
}

/** 在同目录建立排他临时文件并原子替换目标。 */
function atomicWrite(path, payload) {
  const directory = dirname(path);
  let temporary;
  let descriptor;
  try {
    for (let attempt = 0; attempt < 100; attempt += 1) {
      temporary = join(directory, `.release-context.${process.pid}.${randomBytes(8).toString("hex")}.tmp`);
      try {
        descriptor = openSync(temporary, "wx", 0o600);
        break;
      } catch (error) {
        if (error.code !== "EEXIST") throw error;
      }
    }
    if (descriptor === undefined) throw new Error("temporary file collision");
    writeFileSync(descriptor, payload);
    fsyncSync(descriptor);
    closeSync(descriptor);
    descriptor = undefined;
    renameSync(temporary, path);
    temporary = undefined;
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
    if (temporary && existsSync(temporary)) unlinkSync(temporary);
  }
}

/** 写入本次发布唯一规范上下文。 */
export function writeContext(args) {
  const root = resolveRoot(args.projectRoot);
  assertHarnessVersionBinding(root, args.version, { requireStamp: true });
  if (isHarnessSource(root)) {
    const previousTaggedVersion = latestHarnessTagVersion(root);
    if (previousTaggedVersion !== null && args.version <= previousTaggedVersion) {
      throw new ReleaseContextError(`Harness release version must be newer than tagged version ${previousTaggedVersion}`);
    }
  }
  const head = runGit(root, ["rev-parse", "--verify", "HEAD^{commit}"]).stdout.trim();
  if (head !== args.sourceHead) {
    throw new ReleaseContextError("sourceHead must equal the current HEAD before metadata commit");
  }
  if (!validBranchName(args.defaultBranch) || runGit(root, ["check-ref-format", "--branch", args.defaultBranch], { check: false }).returncode !== 0) {
    throw new ReleaseContextError("--default-branch must name one safe Git branch");
  }
  const defaultBranch = args.defaultBranch;
  localRefOid(root, `refs/heads/${defaultBranch}`);
  const value = validateContext({
    schemaVersion: 3,
    sourceHead: args.sourceHead,
    version: args.version,
    releaseDate: args.releaseDate,
    expectedTag: `v${args.version}-${args.releaseDate.replaceAll("-", "")}`,
    defaultBranch,
    releaseReview: buildReview(args),
  });
  verifyReviewScope(root, value);
  const destination = join(root, CONTEXT_RELATIVE_PATH);
  const directory = dirname(destination);
  if (existsSync(directory) && lstatSync(directory).isSymbolicLink()) {
    throw new ReleaseContextError(".harness must not be a symbolic link");
  }
  mkdirSync(directory, { recursive: true, mode: 0o755 });
  if (!statSync(directory).isDirectory()) {
    throw new ReleaseContextError(".harness must be a directory");
  }
  const payload = canonicalBytes(value);
  atomicWrite(destination, payload);
  return {
    status: "written",
    path: CONTEXT_RELATIVE_PATH,
    sourceHead: value.sourceHead,
    expectedTag: value.expectedTag,
    defaultBranch: value.defaultBranch,
    releaseContextSha256: createHash("sha256").update(payload).digest("hex"),
  };
}

/** 校验上下文；发布后模式另验证 HEAD、clean、默认主分支和本地 tag。 */
export function checkContext(args, { published }) {
  const root = resolveRoot(args.projectRoot);
  const { value, canonical, digest } = loadContext(root);
  assertHarnessVersionBinding(root, value.version);
  verifyReviewScope(root, value);
  if (args.expectedSha256 !== undefined && digest !== args.expectedSha256) {
    throw new ReleaseContextError("release context SHA-256 does not match the expected value");
  }
  if (args.expectedVersion !== undefined && value.version !== args.expectedVersion) {
    throw new ReleaseContextError("release context version does not match the expected value");
  }
  const result = {
    status: published ? "released" : "valid",
    path: CONTEXT_RELATIVE_PATH,
    releaseContextSha256: digest,
    ...value,
  };
  if (!published) return result;
  const head = runGit(root, ["rev-parse", "--verify", "HEAD^{commit}"]).stdout.trim();
  validateOid(head, "HEAD");
  if (args.expectedHead !== undefined && head !== args.expectedHead) {
    throw new ReleaseContextError("HEAD does not match the expected source commit");
  }
  const status = runGit(root, ["status", "--porcelain=v1", "--untracked-files=all"], { text: false }).stdout;
  if (status.length !== 0) throw new ReleaseContextError("released worktree must be clean");
  const branchResult = runGit(root, ["symbolic-ref", "--quiet", "--short", "HEAD"], { check: false });
  const branch = branchResult.stdout.trim();
  if (branchResult.returncode !== 0 || branch !== value.defaultBranch) {
    throw new ReleaseContextError("current branch is not the release default branch");
  }
  const committed = runGit(root, ["show", `HEAD:${CONTEXT_RELATIVE_PATH}`], { text: false }).stdout;
  if (!committed.equals(canonical)) {
    throw new ReleaseContextError("release context bytes are not tracked by HEAD");
  }
  if (localRefOid(root, `refs/tags/${value.expectedTag}`) !== head) {
    throw new ReleaseContextError("local release tag does not point to HEAD");
  }
  requireAncestor(root, value.sourceHead, head, "reviewed sourceHead");
  verifyLifecycleRelease(root, value, digest, head);
  return { ...result, sourceCommit: head, branch };
}

/** 把 kebab-case CLI 选项转换为内部 camelCase 字段。 */
function optionName(value) {
  return value.slice(2).replace(/-([a-z])/g, (_match, letter) => letter.toUpperCase());
}

/** 解析固定 CLI，参数结构错误以退出码 2 返回。 */
export function parseArguments(argv) {
  if (argv.length === 0 || !["write", "check", "verify"].includes(argv[0])) {
    throw Object.assign(new Error("the following arguments are required: command"), { cliExit: 2 });
  }
  const command = argv[0];
  const commonCheck = new Set(["projectRoot", "expectedSha256", "expectedVersion", "expectedHead"]);
  const allowed = command === "write" ? new Set([
    "projectRoot", "sourceHead", "version", "releaseDate",
    "defaultBranch", "reviewSelection", "scopeBase", "scopeDiffSha256",
    "reviewEvidenceSummary", "reviewReason", "reviewRemainingRisk",
  ]) : commonCheck;
  const args = { command };
  for (let index = 1; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) throw Object.assign(new Error("unrecognized arguments"), { cliExit: 2 });
    const name = optionName(token);
    if (!allowed.has(name)) throw Object.assign(new Error(`unrecognized arguments: ${token}`), { cliExit: 2 });
    if (index + 1 >= argv.length || argv[index + 1].startsWith("--")) {
      throw Object.assign(new Error(`argument ${token}: expected one argument`), { cliExit: 2 });
    }
    args[name] = argv[index + 1];
    index += 1;
  }
  if (command === "write") {
    const required = [
      "projectRoot", "sourceHead", "version", "releaseDate", "defaultBranch",
      "reviewSelection", "scopeBase", "scopeDiffSha256",
    ];
    const missing = required.filter((field) => args[field] === undefined);
    if (missing.length > 0) throw Object.assign(new Error("the following arguments are required"), { cliExit: 2 });
    if (!["enabled", "disabled"].includes(args.reviewSelection)) {
      throw Object.assign(new Error("invalid choice for --review-selection"), { cliExit: 2 });
    }
  } else if (args.projectRoot === undefined) {
    throw Object.assign(new Error("the following arguments are required: --project-root"), { cliExit: 2 });
  }
  return args;
}

/** 执行 CLI，成功写 stdout，验证失败写单行 JSON stderr。 */
export function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseArguments(argv);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return error.cliExit ?? 2;
  }
  try {
    let result;
    if (args.command === "write") {
      result = writeContext(args);
    } else {
      if (args.expectedSha256 !== undefined && !SHA256_PATTERN.test(args.expectedSha256)) {
        throw new ReleaseContextError("expected SHA-256 must be 64 lowercase hexadecimal characters");
      }
      if (args.expectedHead !== undefined) validateOid(args.expectedHead, "expected HEAD");
      result = checkContext(args, { published: args.command === "verify" });
    }
    const sortValue = (value) => {
      if (Array.isArray(value)) return value.map(sortValue);
      if (value && typeof value === "object") {
        return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortValue(value[key])]));
      }
      return value;
    };
    process.stdout.write(`${JSON.stringify(sortValue(result))}\n`);
    return 0;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`${JSON.stringify({ status: "error", error: message })}\n`);
    return 1;
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  process.exitCode = main();
}
