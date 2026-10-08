#!/usr/bin/env node
/** 检查发布工作树并把 Agent 已复核的精确路径提交到本地 Git。 */

import {
  closeSync,
  copyFileSync,
  lstatSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readlinkSync,
  readSync,
  realpathSync,
  rmSync,
  statSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { dirname, isAbsolute, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HEAD_PATTERN = /^[0-9a-f]{40}$/;
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const SECRET_PATTERNS = [
  /-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----/,
  /(?:^|[^A-Za-z0-9])AKIA[0-9A-Z]{16}(?:$|[^A-Za-z0-9])/,
  /(?:^|[^A-Za-z0-9])gh[pousr]_[A-Za-z0-9]{20,}(?:$|[^A-Za-z0-9])/,
  /(?:^|[^A-Za-z0-9])sk-[A-Za-z0-9_-]{20,}(?:$|[^A-Za-z0-9_-])/,
];
const CACHED_PATCH_ARGUMENTS = [
  "diff", "--cached", "--binary", "--full-index", "--no-renames",
  "--no-ext-diff", "--no-textconv",
];
const APPROVABLE_HARNESS_PATHS = new Set([
  ".harness/release-context.json",
  ".harness/upstream-lock.json",
  ".harness/version-state.json",
]);
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true });

/** 表示发布提交的仓库边界、快照或 Git 操作不安全。 */
export class ReleaseGitError extends Error {
  /** 保存可公开错误，不把异常类型泄露为机器契约。 */
  constructor(message) {
    super(message);
    this.name = "ReleaseGitError";
  }
}

/** 在精确项目根运行 Git；从不经 shell，也不绕过 hooks。 */
export function runGit(root, args, { check = true, text = true, environment = {} } = {}) {
  const result = spawnSync("git", ["-C", root, ...args], {
    encoding: text ? "utf8" : undefined,
    env: {
      ...process.env,
      GIT_TERMINAL_PROMPT: "0",
      GCM_INTERACTIVE: "Never",
      ...environment,
    },
    input: Buffer.alloc(0),
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) throw new ReleaseGitError("git executable is unavailable");
  if (check && result.status !== 0) {
    const stderr = text ? result.stderr : result.stderr.toString("utf8");
    const stdout = text ? result.stdout : result.stdout.toString("utf8");
    const detail = stderr.trim() || stdout.trim() || "unknown git failure";
    throw new ReleaseGitError(`git ${args.join(" ")} failed: ${detail}`);
  }
  return { returncode: result.status ?? 1, stdout: result.stdout, stderr: result.stderr };
}

/** 要求独立非符号链接 Git 顶层、具名分支与 40 位 HEAD。 */
export function resolveRepository(projectRoot) {
  const supplied = resolve(projectRoot);
  try {
    if (lstatSync(supplied).isSymbolicLink()) {
      throw new ReleaseGitError("project root must not be a symbolic link");
    }
  } catch (error) {
    if (error instanceof ReleaseGitError) throw error;
    throw new ReleaseGitError("project root does not exist");
  }
  let root;
  try {
    root = realpathSync(supplied);
  } catch {
    throw new ReleaseGitError("project root does not exist");
  }
  if (!statSync(root).isDirectory()) throw new ReleaseGitError("project root is not a directory");
  const values = runGit(root, ["rev-parse", "--is-inside-work-tree", "--show-toplevel", "HEAD"])
    .stdout.trimEnd().split(/\r?\n/);
  if (values.length !== 3 || values[0] !== "true") {
    throw new ReleaseGitError("project root is not a Git work tree with HEAD");
  }
  let top;
  try { top = realpathSync(values[1]); } catch { top = ""; }
  if (top !== root) throw new ReleaseGitError("project root must equal the independent Git top level");
  const head = values[2];
  if (!HEAD_PATTERN.test(head)) {
    throw new ReleaseGitError("HEAD must resolve to a 40-character lowercase commit");
  }
  const branchResult = runGit(root, ["symbolic-ref", "--quiet", "--short", "HEAD"], { check: false });
  const branch = branchResult.stdout.trim();
  if (branchResult.returncode !== 0 || !branch) {
    throw new ReleaseGitError("HEAD must be attached to a named branch");
  }
  return { root, head, branch };
}

/** 读取包括 ignored 规则外全部未跟踪文件的完整工作树状态。 */
export function statusBytes(root) {
  return runGit(root, ["status", "--porcelain=v1", "-z", "--untracked-files=all"], { text: false }).stdout;
}

/** 严格解码 Git NUL 路径；Node 无 surrogateescape 时对非法 UTF-8 安全失败。 */
function decodeGitPath(value) {
  try {
    return UTF8_DECODER.decode(value);
  } catch {
    throw new ReleaseGitError("Git path is not valid UTF-8");
  }
}

/** 读取 Git NUL 分隔路径输出，避免空格和引号歧义。 */
export function nulPaths(root, args) {
  const output = runGit(root, args, { text: false }).stdout;
  const paths = [];
  let start = 0;
  for (let index = 0; index < output.length; index += 1) {
    if (output[index] !== 0) continue;
    if (index > start) paths.push(decodeGitPath(output.subarray(start, index)));
    start = index + 1;
  }
  if (start < output.length) throw new ReleaseGitError("Git NUL-delimited output is malformed");
  return paths;
}

/** 拒绝绝对路径、Git pathspec magic、元数据和候选产物目录。 */
export function normalizeApprovedPath(value) {
  if (typeof value !== "string" || !value || value.includes("\0") || value.includes("\\") || value.startsWith(":")) {
    throw new ReleaseGitError(`unsafe approved path: ${JSON.stringify(value)}`);
  }
  if (posix.isAbsolute(value) || value === ".") {
    throw new ReleaseGitError(`unsafe approved path: ${JSON.stringify(value)}`);
  }
  const parts = value.split("/");
  if (parts.some((part) => !part || part === "." || part === "..")) {
    throw new ReleaseGitError(`unsafe approved path: ${JSON.stringify(value)}`);
  }
  const normalized = parts.join("/");
  if (
    parts[0] === ".git" ||
    parts[0] === "release" ||
    parts[0].startsWith(".release-clean.") ||
    normalized === ".harness" ||
    (parts[0] === ".harness" && !APPROVABLE_HARNESS_PATHS.has(normalized))
  ) {
    throw new ReleaseGitError(`unapproved Harness metadata or Git internals cannot be approved: ${JSON.stringify(value)}`);
  }
  return normalized;
}

/** 判断变化路径是否属于精确批准文件或批准目录后代。 */
export function pathIsApproved(changed, approved) {
  return approved.some((item) => changed === item || changed.startsWith(`${item}/`));
}

/** 把一个普通文件以固定块大小纳入摘要。 */
function digestFile(digest, path) {
  const descriptor = openSync(path, "r");
  const block = Buffer.allocUnsafe(1024 * 1024);
  try {
    while (true) {
      const count = readSync(descriptor, block, 0, block.length, null);
      if (count === 0) break;
      digest.update(block.subarray(0, count));
    }
  } finally {
    closeSync(descriptor);
  }
}

/** 摘要分支、HEAD、状态、diff、index 与未跟踪字节，阻断切换竞态。 */
export function repositorySnapshotDigest(root, statusValue, { head, branch }) {
  const digest = createHash("sha256");
  digest.update(Buffer.from("branch\0"));
  digest.update(Buffer.from(branch, "utf8"));
  digest.update(Buffer.from("\0head\0"));
  digest.update(Buffer.from(head, "ascii"));
  digest.update(Buffer.from("\0status\0"));
  digest.update(statusValue);
  for (const [label, args] of [
    ["worktree\0", ["diff", "--binary", "--no-ext-diff", "--no-textconv"]],
    ["index\0", ["diff", "--cached", "--binary", "--no-ext-diff", "--no-textconv"]],
  ]) {
    digest.update(Buffer.from(label));
    digest.update(runGit(root, args, { text: false }).stdout);
  }
  for (const relative of nulPaths(root, ["ls-files", "--others", "--exclude-standard", "-z"])) {
    const normalized = normalizeApprovedPath(relative);
    const candidate = join(root, ...normalized.split("/"));
    const metadata = lstatSync(candidate);
    digest.update(Buffer.from("untracked\0"));
    digest.update(Buffer.from(normalized, "utf8"));
    digest.update(Buffer.from("\0"));
    if (metadata.isSymbolicLink()) {
      digest.update(Buffer.from("symlink\0"));
      digest.update(Buffer.from(readlinkSync(candidate), "utf8"));
    } else if (metadata.isFile()) {
      digest.update(Buffer.from("file\0"));
      digestFile(digest, candidate);
    } else {
      throw new ReleaseGitError(`untracked path is not a regular file or symlink: ${JSON.stringify(relative)}`);
    }
  }
  return digest.digest("hex");
}

/** 仅把 NUL 状态记录转换为 JSON 显示值，不重新解释为 pathspec。 */
export function decodeStatusRecords(value) {
  const records = [];
  let start = 0;
  for (let index = 0; index < value.length; index += 1) {
    if (value[index] !== 0) continue;
    if (index > start) records.push(decodeGitPath(value.subarray(start, index)));
    start = index + 1;
  }
  if (start < value.length) throw new ReleaseGitError("Git status output is malformed");
  return records;
}

/** 返回当前 HEAD、clean 结论与快照摘要供逐项复核。 */
export function inspectRepository(projectRoot) {
  const { root, head, branch } = resolveRepository(projectRoot);
  const status = statusBytes(root);
  return {
    status: status.length === 0 ? "clean" : "dirty",
    projectRoot: root,
    branch,
    head,
    statusSha256: repositorySnapshotDigest(root, status, { head, branch }),
    records: decodeStatusRecords(status),
  };
}

/** 只匹配高置信度私钥与令牌形态，不把匹配内容写入日志。 */
export function stagedDiffContainsSecret(root) {
  const patch = runGit(
    root,
    ["diff", "--cached", "--binary", "--no-ext-diff", "--no-textconv", "--unified=0"],
    { text: false },
  ).stdout.toString("latin1");
  return SECRET_PATTERNS.some((pattern) => pattern.test(patch));
}

/** 返回包含新增、删除、mode 与二进制字节的完整 index patch。 */
export function cachedIndexPatch(root, { environment = {} } = {}) {
  return runGit(root, CACHED_PATCH_ARGUMENTS, { text: false, environment }).stdout;
}

/** 在隔离 index 中冻结当前快照执行同一 add 后的精确 patch。 */
export function freezeExpectedIndexPatch(root, literalPathspecs) {
  const lines = runGit(root, ["rev-parse", "--path-format=absolute", "--git-path", "index"])
    .stdout.trimEnd().split(/\r?\n/);
  if (lines.length !== 1) throw new ReleaseGitError("cannot resolve the repository index");
  const indexPath = isAbsolute(lines[0]) ? lines[0] : join(root, lines[0]);
  const indexMetadata = lstatSync(indexPath);
  if (indexMetadata.isSymbolicLink() || !indexMetadata.isFile()) {
    throw new ReleaseGitError("repository index must be a regular file");
  }
  const indexParent = dirname(indexPath);
  const parentMetadata = lstatSync(indexParent);
  if (parentMetadata.isSymbolicLink() || !parentMetadata.isDirectory()) {
    throw new ReleaseGitError("repository index parent must be a regular directory");
  }
  const temporaryDirectory = mkdtempSync(join(indexParent, ".release-reviewed-index-"));
  try {
    const temporaryIndex = join(temporaryDirectory, "index");
    copyFileSync(indexPath, temporaryIndex);
    const environment = { GIT_INDEX_FILE: temporaryIndex };
    runGit(root, ["add", "-A", "--", ...literalPathspecs], { environment });
    return cachedIndexPatch(root, { environment });
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

/** 在快照未变化时提交精确路径、运行正常 hooks 并要求最终 clean。 */
export function commitApproved(projectRoot, { expectedStatusSha256, message, paths }) {
  const initial = resolveRepository(projectRoot);
  const { root, head: previousHead, branch } = initial;
  if (!SHA256_PATTERN.test(expectedStatusSha256)) {
    throw new ReleaseGitError("expected status SHA-256 must be 64 lowercase hexadecimal characters");
  }
  if (typeof message !== "string" || !message.trim() || message.includes("\0")) {
    throw new ReleaseGitError("commit message must be non-empty and contain no NUL");
  }
  const approved = [...new Set(paths.map(normalizeApprovedPath))];
  if (approved.length === 0) throw new ReleaseGitError("at least one reviewed path is required");
  const before = statusBytes(root);
  if (repositorySnapshotDigest(root, before, { head: previousHead, branch }) !== expectedStatusSha256) {
    throw new ReleaseGitError("working tree changed after review; inspect it again before committing");
  }
  if (before.length === 0) throw new ReleaseGitError("working tree is clean; refusing an empty release commit");
  const stagedBefore = nulPaths(root, ["diff", "--cached", "--name-only", "-z"]);
  const unapprovedStaged = stagedBefore.filter((path) => !pathIsApproved(path, approved));
  if (unapprovedStaged.length) {
    throw new ReleaseGitError(`index contains staged paths outside the reviewed scope: ${unapprovedStaged.join(", ")}`);
  }
  const pathspecs = approved.map((path) => `:(literal)${path}`);
  const expectedIndexPatch = freezeExpectedIndexPatch(root, pathspecs);
  const frozenStatus = statusBytes(root);
  const frozen = resolveRepository(root);
  if (
    frozen.head !== previousHead ||
    frozen.branch !== branch ||
    repositorySnapshotDigest(root, frozenStatus, { head: frozen.head, branch: frozen.branch }) !== expectedStatusSha256
  ) {
    throw new ReleaseGitError("working tree, branch, HEAD, or index changed while freezing the reviewed content");
  }
  runGit(root, ["add", "-A", "--", ...pathspecs]);
  const reviewedIndexPatch = cachedIndexPatch(root);
  if (!reviewedIndexPatch.equals(expectedIndexPatch)) {
    throw new ReleaseGitError("staged content changed after review; inspect it again before committing");
  }
  const staged = nulPaths(root, ["diff", "--cached", "--name-only", "-z"]);
  if (staged.length === 0) throw new ReleaseGitError("reviewed paths produced no staged changes");
  const unapproved = staged.filter((path) => !pathIsApproved(path, approved));
  if (unapproved.length) {
    throw new ReleaseGitError(`staged paths escaped the reviewed scope: ${unapproved.join(", ")}`);
  }
  if (stagedDiffContainsSecret(root)) {
    throw new ReleaseGitError("potential secret detected in reviewed staged bytes; remove it and inspect again");
  }
  const unstaged = runGit(root, ["diff", "--quiet"], { check: false });
  if (![0, 1].includes(unstaged.returncode)) throw new ReleaseGitError("cannot verify unstaged tracked changes");
  const untracked = nulPaths(root, ["ls-files", "--others", "--exclude-standard", "-z"]);
  if (unstaged.returncode === 1 || untracked.length) {
    throw new ReleaseGitError("reviewed paths do not cover the complete working tree; refusing a partial release commit");
  }
  const beforeCommit = resolveRepository(root);
  if (beforeCommit.head !== previousHead || beforeCommit.branch !== branch) {
    throw new ReleaseGitError("branch or HEAD changed before the reviewed commit");
  }
  // 不传 --no-verify；pre-commit/commit-msg hook、签名或提交失败都会阻断。
  if (runGit(root, ["commit", "-m", message], { check: false }).returncode !== 0) {
    throw new ReleaseGitError("git commit failed; hooks were not bypassed");
  }
  if (statusBytes(root).length) {
    throw new ReleaseGitError("commit succeeded but working tree is not clean; release must stop");
  }
  const final = resolveRepository(root);
  if (final.head === previousHead) throw new ReleaseGitError("git commit did not advance HEAD");
  if (final.branch !== branch) throw new ReleaseGitError("git commit changed the reviewed branch unexpectedly");
  const parents = runGit(root, ["rev-list", "--parents", "-n", "1", final.head]).stdout.trim().split(/\s+/);
  if (JSON.stringify(parents) !== JSON.stringify([final.head, previousHead])) {
    throw new ReleaseGitError("reviewed commit is not the direct non-merge child of the reviewed HEAD");
  }
  const committedPatch = runGit(
    root,
    ["diff", "--binary", "--full-index", "--no-renames", "--no-ext-diff", "--no-textconv", previousHead, final.head],
    { text: false },
  ).stdout;
  if (!committedPatch.equals(reviewedIndexPatch)) {
    throw new ReleaseGitError("a commit hook changed the reviewed staged content; release must stop");
  }
  return {
    status: "committed",
    projectRoot: root,
    previousHead,
    head: final.head,
    paths: approved,
    clean: true,
  };
}

/** 把 kebab-case CLI 选项转换为内部 camelCase 字段。 */
function optionName(token) {
  return token.slice(2).replace(/-([a-z])/g, (_match, letter) => letter.toUpperCase());
}

/** 解析 inspect 与 commit 的固定参数，结构错误保持退出码 2。 */
function parseArguments(argv) {
  if (argv.length === 0 || !["inspect", "commit"].includes(argv[0])) {
    throw Object.assign(new Error("command is required"), { cliExit: 2 });
  }
  const args = { command: argv[0], path: [] };
  const allowed = args.command === "inspect"
    ? new Set(["projectRoot"])
    : new Set(["projectRoot", "expectedStatusSha256", "message", "path"]);
  for (let index = 1; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--") || index + 1 >= argv.length || argv[index + 1].startsWith("--")) {
      throw Object.assign(new Error(`invalid argument: ${token}`), { cliExit: 2 });
    }
    const name = optionName(token);
    if (!allowed.has(name)) throw Object.assign(new Error(`invalid argument: ${token}`), { cliExit: 2 });
    if (name === "path") args.path.push(argv[index + 1]);
    else args[name] = argv[index + 1];
    index += 1;
  }
  if (args.projectRoot === undefined) throw Object.assign(new Error("--project-root is required"), { cliExit: 2 });
  if (args.command === "commit" && (
    args.expectedStatusSha256 === undefined || args.message === undefined || args.path.length === 0
  )) {
    throw Object.assign(new Error("commit arguments are incomplete"), { cliExit: 2 });
  }
  return args;
}

/** 递归按键排序以保持原 helper 的 JSON 输出稳定性。 */
function sortedJsonValue(value) {
  if (Array.isArray(value)) return value.map(sortedJsonValue);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortedJsonValue(value[key])]));
  }
  return value;
}

/** 输出稳定 JSON；所有歧义或 Git 失败均以非零状态停止。 */
export function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseArguments(argv);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return error.cliExit ?? 2;
  }
  try {
    const result = args.command === "inspect"
      ? inspectRepository(args.projectRoot)
      : commitApproved(args.projectRoot, {
        expectedStatusSha256: args.expectedStatusSha256,
        message: args.message,
        paths: args.path,
      });
    process.stdout.write(`${JSON.stringify(sortedJsonValue(result))}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ status: "error", error: error.message })}\n`);
    return 1;
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  process.exitCode = main();
}
