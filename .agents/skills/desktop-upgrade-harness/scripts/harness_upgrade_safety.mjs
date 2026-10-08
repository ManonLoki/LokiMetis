/** Harness 升级的路径、快照、来源和控制文件安全边界。 */

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { LOCK_RELATIVE, OWNERSHIP_RELATIVE, VERSION_PATTERN } from "./harness_upgrade_policy.mjs";

export class UpgradeError extends Error {}

/** 读取路径本身的 lstat；不存在时返回 null，其他错误照常抛出。 */
export function lstatOrNull(value) {
  try { return fs.lstatSync(value); } catch (error) { if (error?.code === "ENOENT") return null; throw error; }
}

export function isWithin(value, root) { return value === root || value.startsWith(`${root}${path.sep}`); }

export function canonicalDirectory(value, label) {
  const absolute = path.resolve(value);
  const stat = lstatOrNull(absolute);
  if (stat?.isSymbolicLink()) throw new UpgradeError(`${label}不得是符号链接：${value}`);
  let resolved;
  try { resolved = fs.realpathSync(absolute); } catch (error) { throw new UpgradeError(`无法解析${label} ${value}：${error.message}`); }
  if (!fs.statSync(resolved).isDirectory()) throw new UpgradeError(`${label}不是目录：${resolved}`);
  return resolved;
}

export function safeRelativePath(raw) {
  if (typeof raw !== "string" || !raw || path.isAbsolute(raw) || raw.includes("\0")) throw new UpgradeError(`相对路径不安全：${JSON.stringify(raw)}`);
  const slash = raw.replaceAll("\\", "/");
  if (slash === "." || slash.split("/").includes("..") || path.posix.normalize(slash) !== slash) throw new UpgradeError(`相对路径不是规范形式：${JSON.stringify(raw)}`);
  return slash;
}

/** 拒绝根外路径、任一祖先符号链接及中间非目录。 */
export function assertSafePath(root, candidate, label, { finalMayBeMissing }) {
  const absolute = path.resolve(candidate);
  if (!isWithin(absolute, root)) throw new UpgradeError(`${label}越出声明根目录：${absolute}`);
  const parts = path.relative(root, absolute).split(path.sep).filter(Boolean);
  let cursor = root;
  for (let index = 0; index < parts.length; index += 1) {
    cursor = path.join(cursor, parts[index]);
    const stat = lstatOrNull(cursor);
    if (!stat) { if (index !== parts.length - 1 && !finalMayBeMissing) throw new UpgradeError(`${label}的父路径缺失：${cursor}`); break; }
    if (stat.isSymbolicLink()) throw new UpgradeError(`${label}包含符号链接或目录联接：${cursor}`);
    if (index !== parts.length - 1 && !stat.isDirectory()) throw new UpgradeError(`${label}的父路径不是目录：${cursor}`);
  }
  return absolute;
}

export function loadJson(file, label) {
  const stat = lstatOrNull(file);
  if (!stat?.isFile() || stat.isSymbolicLink()) throw new UpgradeError(`${label}必须是非符号链接的普通文件：${file}`);
  let value;
  try { value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(fs.readFileSync(file))); } catch (error) { throw new UpgradeError(`无法读取${label} ${file}：${error.message}`); }
  if (!value || Array.isArray(value) || typeof value !== "object") throw new UpgradeError(`${label}必须包含 JSON 对象：${file}`);
  return value;
}

export function snapshotFd(descriptor) {
  const observed = fs.fstatSync(descriptor);
  if (!observed.isFile()) throw new UpgradeError("快照来源不是普通文件");
  const permissionMode = observed.mode & 0o7777;
  if (permissionMode & ~0o777) throw new UpgradeError(`快照来源使用了不受支持的特殊权限位：0o${permissionMode.toString(8)}`);
  const digest = crypto.createHash("sha256");
  const buffer = Buffer.allocUnsafe(1024 * 1024);
  let position = 0;
  for (;;) { const bytes = fs.readSync(descriptor, buffer, 0, buffer.length, position); if (!bytes) break; digest.update(buffer.subarray(0, bytes)); position += bytes; }
  return { sha256: digest.digest("hex"), mode: permissionMode };
}

export function snapshotFile(file) {
  let descriptor;
  try { descriptor = fs.openSync(file, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0)); }
  catch (error) { throw new UpgradeError(`无法打开普通文件 ${file}：${error.message}`); }
  try { return snapshotFd(descriptor); } finally { fs.closeSync(descriptor); }
}

export function validateSnapshot(value, label) {
  if (value === null) return null;
  if (!value || Array.isArray(value) || typeof value !== "object" || Object.keys(value).sort().join(",") !== "mode,sha256") throw new UpgradeError(`${label}的快照非法`);
  if (typeof value.sha256 !== "string" || !/^[0-9a-f]{64}$/u.test(value.sha256) || !Number.isInteger(value.mode) || value.mode < 0 || value.mode > 0o777) throw new UpgradeError(`${label}的快照非法`);
  return { sha256: value.sha256, mode: value.mode };
}

function gitResult(root, args, { allowFailure = false, encoding = "utf8" } = {}) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: null });
  if (result.error) throw new UpgradeError(`Git 命令失败（${args.join(" ")}）：${result.error.message}`);
  let completed = result;
  if (encoding !== null) {
    try {
      const decoder = new TextDecoder("utf-8", { fatal: true });
      completed = { ...result, stdout: decoder.decode(result.stdout ?? Buffer.alloc(0)), stderr: decoder.decode(result.stderr ?? Buffer.alloc(0)) };
    } catch {
      throw new UpgradeError(`Git 命令返回了非 UTF-8 输出（${args.join(" ")}）`);
    }
  }
  if ((completed.status ?? 1) !== 0 && !allowFailure) {
    const detail = encoding === null ? new TextDecoder("utf-8").decode(completed.stderr ?? Buffer.alloc(0)).trim() : (completed.stderr ?? "").trim();
    throw new UpgradeError(`Git 命令失败（${args.join(" ")}）：${detail}`);
  }
  return completed;
}

export function requireGitRoot(root, label = "目标") {
  const top = fs.realpathSync(gitResult(root, ["rev-parse", "--show-toplevel"]).stdout.trim());
  if (top !== root) throw new UpgradeError(`${label} Git 顶层目录不匹配：预期 ${root}，实际 ${top}`);
  const commonRaw = gitResult(root, ["rev-parse", "--git-common-dir"]).stdout.trim();
  const common = fs.realpathSync(path.isAbsolute(commonRaw) ? commonRaw : path.join(root, commonRaw));
  const commonStat = fs.statSync(common);
  const headResult = gitResult(root, ["rev-parse", "--verify", "HEAD"], { allowFailure: true });
  if ((headResult.status ?? 1) !== 0) throw new UpgradeError(`${label} Git 仓库必须已有 HEAD 提交`);
  const statusResult = gitResult(root, ["status", "--porcelain=v1", "--untracked-files=all", "-z"], { encoding: null });
  const statusBytes = Buffer.from(statusResult.stdout ?? Buffer.alloc(0));
  return {
    top_level: root, common_dir: common, common_device: commonStat.dev, common_inode: commonStat.ino,
    head: headResult.stdout.trim(), branch: gitResult(root, ["branch", "--show-current"]).stdout.trim(),
    dirty: statusBytes.length > 0, status_sha256: crypto.createHash("sha256").update(statusBytes).digest("hex"),
  };
}

export function requireSourceIdentity(sourceRoot, expectedVersion, expectedCommit) {
  const source = canonicalDirectory(sourceRoot, "源 Harness 根目录");
  let identity;
  try { identity = requireGitRoot(source, "源 Harness"); } catch (error) { throw error; }
  if (identity.dirty) throw new UpgradeError("源 Harness Git 工作树必须保持干净");
  if (expectedCommit !== identity.head) throw new UpgradeError(`源 commit 与源 Harness HEAD 不匹配：预期 ${expectedCommit}，实际 ${identity.head}`);
  const versionFile = assertSafePath(source, path.join(source, "Version.md"), "源 Harness Version.md", { finalMayBeMissing: false });
  const stat = lstatOrNull(versionFile);
  if (!stat?.isFile() || stat.isSymbolicLink()) throw new UpgradeError("源 Harness Version.md 必须是普通文件");
  let versionText;
  try { versionText = new TextDecoder("utf-8", { fatal: true }).decode(fs.readFileSync(versionFile)); }
  catch (error) { throw new UpgradeError(`源 Harness Version.md 不是有效 UTF-8：${error.message}`); }
  const match = VERSION_PATTERN.exec(versionText);
  if (!match) throw new UpgradeError("源 Harness Version.md 没有当前版本");
  if (expectedVersion !== match[1]) throw new UpgradeError(`源版本与源 Harness Version.md 不匹配：预期 ${expectedVersion}，实际 ${match[1]}`);
  return { source, identity };
}

export function requireControlPaths(target, ownershipPath, lockPath) {
  const expectedOwnership = path.join(target, OWNERSHIP_RELATIVE);
  let observedOwnership;
  try { observedOwnership = fs.realpathSync(ownershipPath); } catch (error) { throw new UpgradeError(`无法解析所有权 manifest ${ownershipPath}：${error.message}`); }
  if (observedOwnership !== expectedOwnership) throw new UpgradeError(`所有权 manifest 必须精确位于 ${expectedOwnership}`);
  assertSafePath(target, observedOwnership, "所有权 manifest", { finalMayBeMissing: false });
  if (!fs.statSync(observedOwnership).isFile()) throw new UpgradeError(`缺少所有权 manifest：${observedOwnership}`);
  const expectedLock = path.join(target, LOCK_RELATIVE);
  const observedLock = path.resolve(lockPath);
  if (observedLock !== expectedLock) throw new UpgradeError(`lock 必须精确位于 ${expectedLock}`);
  assertSafePath(target, observedLock, "上游 lock", { finalMayBeMissing: true });
  return { ownership: observedOwnership, lockFile: observedLock };
}
