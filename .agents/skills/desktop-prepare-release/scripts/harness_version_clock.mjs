#!/usr/bin/env node
/** 在 Harness 正式发布时选择上海当前分钟的唯一时间版本。 */

import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import {
  closeSync, constants, existsSync, fchmodSync, fstatSync, fsyncSync, lstatSync,
  mkdirSync, openSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const TIMESTAMP_PATTERN = /^\d{12}$/u;
const TAG_PATTERN = /^v(\d{12})-(\d{8})$/u;
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const CURRENT_VERSION_LINE = /^(- 当前版本：`)([^`\r\n]+)(`\r?)$/gmu;
const STAMP_FILE = "harness-version-stamp.json";

/** Tag 后缀是执行发布的日期，独立于版本取号日期。 */
function validCompactDate(value) {
  if (!/^\d{8}$/u.test(value)) return false;
  const year = Number(value.slice(0, 4));
  const month = Number(value.slice(4, 6));
  const day = Number(value.slice(6, 8));
  const date = new Date(Date.UTC(year, month - 1, day));
  return date.getUTCFullYear() === year
    && date.getUTCMonth() === month - 1
    && date.getUTCDate() === day;
}

/** 检查时间版本的日历字段，避免规范形状掩盖不存在的日期。 */
export function validHarnessTimestamp(version) {
  if (typeof version !== "string" || !TIMESTAMP_PATTERN.test(version)) return false;
  const year = Number(version.slice(0, 4));
  const month = Number(version.slice(4, 6));
  const day = Number(version.slice(6, 8));
  const hour = Number(version.slice(8, 10));
  const minute = Number(version.slice(10, 12));
  const date = new Date(Date.UTC(year, month - 1, day, hour, minute));
  return date.getUTCFullYear() === year
    && date.getUTCMonth() === month - 1
    && date.getUTCDate() === day
    && date.getUTCHours() === hour
    && date.getUTCMinutes() === minute;
}

/** 使用明确时区取当前年月日时分，不依赖宿主的本地时区。 */
export function shanghaiTimestamp(now = new Date()) {
  if (!(now instanceof Date) || Number.isNaN(now.getTime())) {
    throw new Error("current time is invalid");
  }
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: "Asia/Shanghai",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(now);
  const fields = Object.fromEntries(parts.map(({ type, value }) => [type, value]));
  return `${fields.year}${fields.month}${fields.day}${fields.hour}${fields.minute}`;
}

/** 验证调用目录是 Git 顶层，防止借用其他仓库的标签历史。 */
function gitRoot(projectRoot) {
  const supplied = resolve(projectRoot);
  if (lstatSync(supplied).isSymbolicLink()) throw new Error("project root must not be a symbolic link");
  const root = realpathSync(supplied);
  const result = spawnSync("git", ["-C", root, "rev-parse", "--show-toplevel"], {
    encoding: "utf8", input: "", env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  if (result.error || result.status !== 0 || realpathSync(result.stdout.trim()) !== root) {
    throw new Error("project root must equal the independent Git top level");
  }
  return root;
}

/** 仅在 Harness 源中使用本选择器，下游继续使用自己的三段版本。 */
export function isHarnessSource(root) {
  return existsSync(join(root, "Version.md"))
    && existsSync(join(root, ".agents", "skills", "desktop-instantiate-project", "SKILL.md"));
}

/** Windows 的 lstat.dev 可能为 0，而 fstat.dev 是卷号；仍要求 inode 一致。 */
export function sameOpenedFile(metadata, opened) {
  return opened.isFile() && opened.ino === metadata.ino
    && (metadata.dev === 0 || opened.dev === 0 || opened.dev === metadata.dev);
}

/** 通过非跟随文件描述符读取，保留后续定点替换所需的原始字节。 */
function readVersionDocument(root) {
  const file = join(root, "Version.md");
  const metadata = lstatSync(file);
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    throw new Error("Version.md must be a non-symbolic regular file");
  }
  const descriptor = openSync(file, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  let bytes;
  let opened;
  try {
    opened = fstatSync(descriptor);
    if (!sameOpenedFile(metadata, opened)) {
      throw new Error("Version.md changed while opening");
    }
    bytes = readFileSync(descriptor);
  } finally {
    closeSync(descriptor);
  }
  const source = UTF8_DECODER.decode(bytes);
  const lines = [...source.matchAll(CURRENT_VERSION_LINE)];
  if (lines.length !== 1 || !validHarnessTimestamp(lines[0][2])) {
    throw new Error("Version.md must declare one valid YYYYMMDDHHMM current version");
  }
  return { file, mode: opened.mode & 0o7777, bytes, source, line: lines[0], version: lines[0][2] };
}

/** 从固定字段读取当前 Version.md；发布上下文只接受这个精确版本。 */
export function readHarnessVersion(root) {
  return readVersionDocument(root).version;
}

/** 将时间取号凭证保存在仓库 Git common-dir，避免手工时间值冒充受管发布取号。 */
function stampPath(root) {
  const result = spawnSync("git", ["-C", root, "rev-parse", "--path-format=absolute", "--git-common-dir"], {
    encoding: "utf8", input: "", env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  if (result.error || result.status !== 0) throw new Error("cannot resolve Git common directory for Harness version stamp");
  const commonDir = resolve(root, result.stdout.trim());
  const directory = join(commonDir, "agent-first-harness");
  if (existsSync(directory)) {
    const metadata = lstatSync(directory);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) throw new Error("Harness Git state directory is unsafe");
  }
  return { directory, file: join(directory, STAMP_FILE) };
}

/** 只读验证本次版本确由上海当前分钟取号，且 Version.md 保持取号后的精确字节。 */
export function readHarnessVersionStamp(projectRoot, expectedVersion) {
  const root = gitRoot(projectRoot);
  const { file } = stampPath(root);
  if (!existsSync(file)) throw new Error("Harness release requires a managed current-minute version stamp");
  const metadata = lstatSync(file);
  if (metadata.isSymbolicLink() || !metadata.isFile()) throw new Error("Harness version stamp must be a regular file");
  let value;
  try { value = JSON.parse(UTF8_DECODER.decode(readFileSync(file))); }
  catch { throw new Error("Harness version stamp is invalid"); }
  if (!value || typeof value !== "object" || Array.isArray(value)
      || JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(["schemaVersion", "stampedAt", "version", "versionFileSha256"])
      || value.schemaVersion !== 1 || value.version !== expectedVersion
      || typeof value.stampedAt !== "string" || typeof value.versionFileSha256 !== "string"
      || !/^[0-9a-f]{64}$/u.test(value.versionFileSha256)) {
    throw new Error("Harness version stamp does not match the release version");
  }
  const instant = new Date(value.stampedAt);
  if (Number.isNaN(instant.getTime()) || instant.toISOString() !== value.stampedAt
      || shanghaiTimestamp(instant) !== expectedVersion) {
    throw new Error("Harness version stamp is not an actual Shanghai current minute");
  }
  const document = readVersionDocument(root);
  const digest = createHash("sha256").update(document.bytes).digest("hex");
  if (document.version !== expectedVersion || digest !== value.versionFileSha256) {
    throw new Error("Version.md changed after the managed Harness version stamp");
  }
  return value;
}

/** 无 tag 的首发仅可从已声明起始版本取号；已写入目标版本须复用。 */
function readInitialVersion(document) {
  const lines = [...document.source.matchAll(/^- 时间版本起始值：`([^`\r\n]+)`\r?$/gmu)];
  if (lines.length !== 1 || !validHarnessTimestamp(lines[0][1])) {
    throw new Error("Version.md must declare one valid initial version before the first tag");
  }
  return lines[0][1];
}

/** 从本地 Harness 时间版本 tag 中取最大值；任何相关畸形 tag 都失败关闭。 */
export function latestHarnessTagVersion(root) {
  const result = spawnSync("git", ["-C", root, "for-each-ref", "--format=%(refname:short)", "refs/tags"], {
    encoding: "utf8", input: "", env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  if (result.error || result.status !== 0) throw new Error("cannot read Harness release tags");
  let latest = null;
  for (const tag of result.stdout.split(/\r?\n/u).filter(Boolean)) {
    const match = TAG_PATTERN.exec(tag);
    if (!match) {
      if (/^v\d{12}(?:-|$)/u.test(tag)) throw new Error(`invalid Harness release tag: ${tag}`);
      continue;
    }
    const [, version, compactDate] = match;
    if (!validHarnessTimestamp(version) || !validCompactDate(compactDate)) {
      throw new Error(`invalid Harness release tag: ${tag}`);
    }
    if (latest === null || version > latest) latest = version;
  }
  return latest;
}

/** 只读选择正式发布此刻的时间版本，拒绝与已有 tag 同分钟或倒退。 */
export function selectHarnessVersion(projectRoot, now = new Date()) {
  const root = gitRoot(projectRoot);
  if (!isHarnessSource(root)) throw new Error("project root is not the Harness source");
  const version = shanghaiTimestamp(now);
  const previousTaggedVersion = latestHarnessTagVersion(root);
  const document = readVersionDocument(root);
  const recordedVersion = document.version;
  if (previousTaggedVersion === null) {
    if (recordedVersion !== readInitialVersion(document)) {
      throw new Error(`Version.md already records untagged release target ${recordedVersion}; reuse it`);
    }
  } else if (recordedVersion > previousTaggedVersion) {
    throw new Error(`Version.md already records untagged release target ${recordedVersion}; reuse it`);
  } else if (recordedVersion < previousTaggedVersion) {
    throw new Error(`Version.md version ${recordedVersion} is older than latest tag version ${previousTaggedVersion}`);
  }
  const previousVersion = previousTaggedVersion ?? recordedVersion;
  if (version <= previousVersion) {
    const source = previousTaggedVersion === null ? "recorded" : "tagged";
    throw new Error(`Shanghai current-minute version ${version} must be newer than ${source} version ${previousVersion}; wait for a later minute`);
  }
  return {
    status: "selected",
    version,
    previousTaggedVersion,
    recordedVersion,
  };
}

/** 正式发布显式调用时原子写入唯一版本字段；已选目标由调用方复用。 */
export function stampHarnessVersion(projectRoot, now = new Date()) {
  const selected = selectHarnessVersion(projectRoot, now);
  const root = gitRoot(projectRoot);
  const document = readVersionDocument(root);
  if (document.version !== selected.recordedVersion) {
    throw new Error("Version.md changed after version selection");
  }
  const versionStart = document.line.index + document.line[1].length;
  const byteStart = Buffer.byteLength(document.source.slice(0, versionStart), "utf8");
  const byteEnd = byteStart + Buffer.byteLength(document.version, "utf8");
  const updated = Buffer.concat([
    document.bytes.subarray(0, byteStart),
    Buffer.from(selected.version, "utf8"),
    document.bytes.subarray(byteEnd),
  ]);
  const stamp = {
    schemaVersion: 1,
    stampedAt: now.toISOString(),
    version: selected.version,
    versionFileSha256: createHash("sha256").update(updated).digest("hex"),
  };
  const { directory, file: receiptFile } = stampPath(root);
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const receiptTemporary = join(directory, `.harness-version-stamp.${process.pid}.${randomBytes(8).toString("hex")}.tmp`);
  let receiptDescriptor;
  try {
    receiptDescriptor = openSync(receiptTemporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | (constants.O_NOFOLLOW ?? 0), 0o600);
    writeFileSync(receiptDescriptor, `${JSON.stringify(stamp)}\n`);
    fsyncSync(receiptDescriptor);
    closeSync(receiptDescriptor);
    receiptDescriptor = undefined;
    renameSync(receiptTemporary, receiptFile);
  } finally {
    if (receiptDescriptor !== undefined) closeSync(receiptDescriptor);
    rmSync(receiptTemporary, { force: true });
  }
  const temporary = join(root, `.Version.md.${process.pid}.${randomBytes(8).toString("hex")}.tmp`);
  let descriptor;
  try {
    descriptor = openSync(temporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL, document.mode);
    try {
      fchmodSync(descriptor, document.mode);
      writeFileSync(descriptor, updated);
      fsyncSync(descriptor);
    } finally {
      closeSync(descriptor);
      descriptor = undefined;
    }
    const current = readVersionDocument(root);
    if (!current.bytes.equals(document.bytes) || current.mode !== document.mode) {
      throw new Error("Version.md changed during atomic version update");
    }
    renameSync(temporary, document.file);
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
    rmSync(temporary, { force: true });
  }
  return { ...selected, status: "stamped" };
}

/** 命令行只接受显式 select/stamp 与项目根，不接受时间覆盖。 */
export function main(argv = process.argv.slice(2)) {
  if (argv.length !== 3 || !["select", "stamp"].includes(argv[0]) || argv[1] !== "--project-root") {
    process.stderr.write("usage: harness_version_clock.mjs <select|stamp> --project-root <path>\n");
    return 2;
  }
  try {
    const result = argv[0] === "stamp" ? stampHarnessVersion(argv[2]) : selectHarnessVersion(argv[2]);
    process.stdout.write(`${JSON.stringify(result)}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ status: "error", error: error.message })}\n`);
    return 1;
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  process.exitCode = main();
}
