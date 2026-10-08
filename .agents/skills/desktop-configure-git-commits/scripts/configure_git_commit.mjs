#!/usr/bin/env node
/** 为独立 Git 仓库管理提交模板与仓库级提交身份。 */

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const SOURCE_TEMPLATE = path.join(SCRIPT_DIRECTORY, "..", "assets", "commit-template.txt");
const MANAGED_DIRECTORY = "harness";
const MANAGED_TEMPLATE = "meaningful-commit-template.txt";
const IDENTITY_KEYS = ["user.name", "user.email"];

export class ConfigurationError extends Error {}

function expandHome(value) {
  if (value === "~") return os.homedir();
  if (value.startsWith(`~${path.sep}`)) return path.join(os.homedir(), value.slice(2));
  return value;
}

function lstatOrNull(value) {
  try { return fs.lstatSync(value); } catch (error) { if (error?.code === "ENOENT") return null; throw error; }
}

/** 在精确项目根运行 Git，并把失败转换为可读且不含 shell 展开的错误。 */
export function runGit(root, args, { check = true, env = process.env } = {}) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: null, env });
  if (result.error?.code === "ENOENT") throw new ConfigurationError("git executable is unavailable");
  if (result.error) throw new ConfigurationError(`git ${args.join(" ")} failed: ${result.error.message}`);
  let stdout;
  let stderr;
  try {
    const decoder = new TextDecoder("utf-8", { fatal: true });
    stdout = decoder.decode(result.stdout ?? Buffer.alloc(0));
    stderr = decoder.decode(result.stderr ?? Buffer.alloc(0));
  } catch {
    throw new ConfigurationError(`git ${args.join(" ")} returned non-UTF-8 output`);
  }
  const completed = { returncode: result.status ?? 1, stdout, stderr };
  if (check && completed.returncode !== 0) {
    const detail = completed.stderr.trim() || completed.stdout.trim() || "unknown git failure";
    throw new ConfigurationError(`git ${args.join(" ")} failed: ${detail}`);
  }
  return completed;
}

/** 解析独立工作树与 Git common dir，拒绝父仓库、裸仓库和符号链接目录。 */
export function resolveRepository(projectRoot) {
  const supplied = path.resolve(expandHome(projectRoot));
  const suppliedStat = lstatOrNull(supplied);
  if (suppliedStat?.isSymbolicLink()) throw new ConfigurationError("project root must not be a symbolic link");
  if (!suppliedStat) throw new ConfigurationError("project root does not exist");
  const root = fs.realpathSync(supplied);
  if (!fs.statSync(root).isDirectory()) throw new ConfigurationError("project root is not a directory");
  const query = runGit(root, ["rev-parse", "--is-inside-work-tree", "--show-toplevel", "--git-common-dir"]).stdout.trimEnd().split(/\r?\n/);
  if (query.length !== 3) throw new ConfigurationError("unexpected output from git rev-parse");
  const [inside, topLevelRaw, commonRaw] = query;
  if (inside !== "true") throw new ConfigurationError("project root is not a Git work tree");
  const topLevel = fs.realpathSync(topLevelRaw);
  if (topLevel !== root) throw new ConfigurationError("project root must equal the independent Git top level");
  const commonCandidate = path.isAbsolute(commonRaw) ? commonRaw : path.join(root, commonRaw);
  const commonLstat = lstatOrNull(commonCandidate);
  if (commonLstat?.isSymbolicLink()) throw new ConfigurationError("Git common dir must be a real directory");
  const commonDir = fs.realpathSync(commonCandidate);
  if (!fs.statSync(commonDir).isDirectory()) throw new ConfigurationError("Git common dir must be a real directory");
  return { root, commonDir };
}

/** 读取受跟踪模板，并确保未编辑模板不会留下可提交的正文。 */
export function readSourceTemplate() {
  const stat = lstatOrNull(SOURCE_TEMPLATE);
  if (!stat?.isFile() || stat.isSymbolicLink()) throw new ConfigurationError("source commit template must be a regular file");
  const data = fs.readFileSync(SOURCE_TEMPLATE);
  let text;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(data); }
  catch { throw new ConfigurationError("source commit template must be valid UTF-8"); }
  if (!text.endsWith("\n")) throw new ConfigurationError("source commit template must end with a newline");
  if (text.split(/\r?\n/).some((line) => line.trim() && !line.startsWith("#"))) {
    throw new ConfigurationError("source commit template must contain comments only");
  }
  return data;
}

export function managedPath(commonDir) {
  return path.join(commonDir, MANAGED_DIRECTORY, MANAGED_TEMPLATE);
}

/** 一次性列出仓库本地配置并按键分组，避免逐键各起一个 Git 进程。 */
export function readLocalSettings(root, keys) {
  const result = runGit(root, ["config", "--local", "--list", "--null"], { check: false });
  if (result.returncode === 1) return Object.fromEntries(keys.map((key) => [key, []]));
  if (result.returncode !== 0) throw new ConfigurationError(`cannot read local Git settings: ${result.stderr.trim() || result.stdout.trim() || "unknown git failure"}`);
  const wanted = new Map(keys.map((key) => [key.toLowerCase(), key]));
  const values = Object.fromEntries(keys.map((key) => [key, []]));
  for (const entry of result.stdout.split("\0")) {
    if (!entry) continue;
    const separator = entry.indexOf("\n");
    const name = separator < 0 ? entry : entry.slice(0, separator);
    const value = separator < 0 ? "" : entry.slice(separator + 1);
    const original = wanted.get(name.toLowerCase());
    if (original) values[original].push(value);
  }
  return values;
}

/** 读取一个最终生效的 Git 设置及其 scope/origin，不猜测配置优先级。 */
export function readEffectiveSetting(root, key) {
  const result = runGit(root, ["config", "--null", "--show-origin", "--show-scope", "--get", key], { check: false });
  if (result.returncode === 1) return null;
  if (result.returncode !== 0) throw new ConfigurationError(`cannot read effective Git setting ${key}: ${result.stderr.trim() || result.stdout.trim() || "unknown git failure"}`);
  const fields = result.stdout.split("\0");
  if (fields.at(-1) === "") fields.pop();
  if (fields.length !== 3) throw new ConfigurationError(`unexpected effective Git setting output for ${key}`);
  return { scope: fields[0], origin: fields[1], value: fields[2] };
}

function containsControl(value) {
  return [...value].some((character) => { const code = character.codePointAt(0); return code < 32 || code === 127; });
}

export function validateExistingName(name) {
  if (!name.trim() || containsControl(name)) throw new ConfigurationError("existing Git user.name is invalid; refusing silent replacement");
}

export function validateExistingEmail(email) {
  if (!email.trim() || containsControl(email) || !/^[^@\s]+@[^@\s]+$/u.test(email)) throw new ConfigurationError("existing Git user.email is invalid; refusing silent replacement");
}

export function validateFallbackUsername(value) {
  if (!/^[A-Za-z](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$/u.test(value)) throw new ConfigurationError("fallback username must be an Agent-provided ASCII English device username");
  return value;
}

export function replaceConfigValues(root, key, values) {
  const unset = runGit(root, ["config", "--local", "--unset-all", key], { check: false });
  if (![0, 5].includes(unset.returncode)) throw new ConfigurationError(`cannot clear local Git setting ${key}: ${unset.stderr.trim() || unset.stdout.trim() || "unknown git failure"}`);
  for (const value of values) runGit(root, ["config", "--local", "--add", key, value]);
}

export function identityReport(projectRoot, { requireComplete }) {
  const { root } = resolveRepository(projectRoot);
  const observed = Object.fromEntries(IDENTITY_KEYS.map((key) => [key, readEffectiveSetting(root, key)]));
  const missing = IDENTITY_KEYS.filter((key) => observed[key] === null);
  if (!missing.length) { validateExistingName(observed["user.name"].value); validateExistingEmail(observed["user.email"].value); }
  else if (requireComplete) throw new ConfigurationError(`missing effective Git identity: ${missing.join(", ")}`);
  return { status: missing.length ? "missing" : "ok", projectRoot: root, identity: { name: observed["user.name"], email: observed["user.email"], missing } };
}

/** 只为缺失的有效身份字段写入仓库本地回退值，并保留已有字段。 */
export function bootstrapIdentity(projectRoot, { fallbackUsername }) {
  const { root } = resolveRepository(projectRoot);
  const before = Object.fromEntries(IDENTITY_KEYS.map((key) => [key, readEffectiveSetting(root, key)]));
  for (const [key, setting] of Object.entries(before)) {
    if (!setting) continue;
    if (key === "user.name") validateExistingName(setting.value); else validateExistingEmail(setting.value);
  }
  const missing = IDENTITY_KEYS.filter((key) => before[key] === null);
  let derivedUsername = null;
  if (missing.length) {
    if (!fallbackUsername) throw new ConfigurationError("missing Git identity requires --fallback-username");
    derivedUsername = validateFallbackUsername(fallbackUsername);
  }
  const fallbackValues = {};
  if (missing.includes("user.name")) fallbackValues["user.name"] = derivedUsername;
  if (missing.includes("user.email")) fallbackValues["user.email"] = `${derivedUsername}@gmail.com`;
  const keys = Object.keys(fallbackValues);
  const previousLocal = readLocalSettings(root, keys);
  try {
    for (const [key, value] of Object.entries(fallbackValues)) replaceConfigValues(root, key, [value]);
  } catch (error) {
    const rollbackErrors = [];
    for (const key of [...keys].reverse()) try { replaceConfigValues(root, key, previousLocal[key]); } catch (rollbackError) { rollbackErrors.push(rollbackError.message); }
    throw new ConfigurationError(`identity bootstrap failed: ${error.message}${rollbackErrors.length ? `; rollback errors: ${rollbackErrors.join("; ")}` : ""}`);
  }
  const after = Object.fromEntries(IDENTITY_KEYS.map((key) => [key, readEffectiveSetting(root, key)]));
  if (!after["user.name"] || !after["user.email"]) throw new ConfigurationError("identity bootstrap completed but effective identity remains incomplete");
  validateExistingName(after["user.name"].value); validateExistingEmail(after["user.email"].value);
  return {
    status: "configured", projectRoot: root, changed: keys.length > 0,
    identity: {
      name: { ...after["user.name"], source: keys.includes("user.name") ? "repo-local-bootstrap" : "existing" },
      email: { ...after["user.email"], source: keys.includes("user.email") ? "repo-local-bootstrap" : "existing" },
      writtenLocalKeys: [...keys].sort(),
      derivation: { asciiDeviceUsername: derivedUsername, emailRule: derivedUsername ? "<asciiDeviceUsername>@gmail.com" : null },
    },
  };
}

export function desiredSettings(templatePath) {
  return { "commit.template": templatePath, "commit.cleanup": "strip", "commit.verbose": "true", "core.commentChar": "#" };
}

/** 在 Git common dir 内原子写入模板，并拒绝已有符号链接或非普通文件。 */
export function writeAtomic(target, data) {
  const directory = path.dirname(target);
  const directoryStat = lstatOrNull(directory);
  if (directoryStat?.isSymbolicLink() || (directoryStat && !directoryStat.isDirectory())) throw new ConfigurationError("managed Git metadata path is not a real directory");
  if (!directoryStat) fs.mkdirSync(directory, { mode: 0o700 });
  const targetStat = lstatOrNull(target);
  if (targetStat?.isSymbolicLink() || (targetStat && !targetStat.isFile())) throw new ConfigurationError("managed commit template is not a regular file");
  const temporary = path.join(directory, `.commit-template-${crypto.randomUUID()}`);
  let descriptor;
  try {
    descriptor = fs.openSync(temporary, "wx", 0o600);
    fs.writeFileSync(descriptor, data);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor); descriptor = undefined;
    fs.renameSync(temporary, target);
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    fs.rmSync(temporary, { force: true });
  }
}

function restoreTemplate(target, previous) {
  if (previous === null) {
    const stat = lstatOrNull(target);
    if (stat?.isFile() && !stat.isSymbolicLink()) fs.unlinkSync(target);
  } else writeAtomic(target, previous);
}

function loadContext(projectRoot) {
  const { root, commonDir } = resolveRepository(projectRoot);
  const source = readSourceTemplate();
  const target = managedPath(commonDir);
  return { root, target, source, desired: desiredSettings(target) };
}

/** 安装模板并事务式设置仓库本地配置；冲突默认失败关闭。 */
export function install(projectRoot, { replace }) {
  const { root, target, source, desired } = loadContext(projectRoot);
  const previousSettings = readLocalSettings(root, Object.keys(desired));
  const conflicts = {};
  let settingsChanged = false;
  for (const [key, value] of Object.entries(desired)) {
    const values = previousSettings[key];
    if (values.length !== 1 || values[0] !== value) { settingsChanged = true; if (values.length) conflicts[key] = values; }
  }
  if (Object.keys(conflicts).length && !replace) throw new ConfigurationError(`conflicting local Git settings: ${Object.keys(conflicts).sort().join(", ")}; rerun with --replace only after approval`);
  let previousTemplate = null;
  const stat = lstatOrNull(target);
  if (stat) {
    if (stat.isSymbolicLink() || !stat.isFile()) throw new ConfigurationError("managed commit template is not a regular file");
    previousTemplate = fs.readFileSync(target);
  }
  const changed = previousTemplate === null || !previousTemplate.equals(source) || settingsChanged;
  writeAtomic(target, source);
  try {
    for (const [key, value] of Object.entries(desired)) replaceConfigValues(root, key, [value]);
  } catch (error) {
    const rollbackErrors = [];
    for (const key of Object.keys(desired).reverse()) try { replaceConfigValues(root, key, previousSettings[key]); } catch (rollbackError) { rollbackErrors.push(rollbackError.message); }
    try { restoreTemplate(target, previousTemplate); } catch (rollbackError) { rollbackErrors.push(rollbackError.message); }
    throw new ConfigurationError(`installation failed: ${error.message}${rollbackErrors.length ? `; rollback errors: ${rollbackErrors.join("; ")}` : ""}`);
  }
  return { status: "installed", projectRoot: root, templatePath: target, settings: desired, changed, replacedConflicts: Object.keys(conflicts).length > 0 };
}

export function checkInstallation(projectRoot) {
  const { root, target, source, desired } = loadContext(projectRoot);
  const problems = [];
  const stat = lstatOrNull(target);
  if (!stat?.isFile() || stat.isSymbolicLink()) problems.push("managed commit template is missing or not a regular file");
  else if (!fs.readFileSync(target).equals(source)) problems.push("managed commit template differs from the tracked source");
  const observed = readLocalSettings(root, Object.keys(desired));
  for (const [key, value] of Object.entries(desired)) if (observed[key].length !== 1 || observed[key][0] !== value) problems.push(`local Git setting ${key} does not equal ${JSON.stringify(value)}`);
  if (problems.length) throw new ConfigurationError(problems.join("; "));
  return { status: "ok", projectRoot: root, templatePath: target, settings: desired };
}

function parseCommand(argv) {
  const command = argv[0];
  if (!["install", "check", "identity-bootstrap", "identity-check", "identity-report"].includes(command)) throw new ConfigurationError("a valid command is required");
  const options = { "project-root": { type: "string" } };
  if (command === "install") options.replace = { type: "boolean", default: false };
  if (command === "identity-bootstrap") options["fallback-username"] = { type: "string" };
  const { values } = parseArgs({ args: argv.slice(1), options, strict: true });
  if (!values["project-root"]) throw new ConfigurationError("--project-root is required");
  return { command, values };
}

function main(argv) {
  const { command, values } = parseCommand(argv);
  if (command === "install") return install(values["project-root"], { replace: values.replace });
  if (command === "check") return checkInstallation(values["project-root"]);
  if (command === "identity-bootstrap") return bootstrapIdentity(values["project-root"], { fallbackUsername: values["fallback-username"] });
  return identityReport(values["project-root"], { requireComplete: command === "identity-check" });
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try { process.stdout.write(`${JSON.stringify(main(process.argv.slice(2)))}\n`); }
  catch (error) { process.stderr.write(`${JSON.stringify({ status: "error", error: error.message })}\n`); process.exitCode = 1; }
}
