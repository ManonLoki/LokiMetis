#!/usr/bin/env node

/** 管理下游 Rust 产品的确定性语义化版本状态。 */

import { randomUUID } from "node:crypto";
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
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const STATE_RELATIVE = path.join(".harness", "version-state.json");
const SEMVER_PATTERN = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;
const CHANGE_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$/;
const SOURCE_COMMIT_PATTERN = /^[0-9a-fA-F]{40}$/;
export const CARGO_SEMVER_COMPONENT_MAX = (1n << 64n) - 1n;
const CARGO_SEMVER_COMPONENT_MAX_TEXT = CARGO_SEMVER_COMPONENT_MAX.toString();
const VERSION_LINE_PATTERN = /^(?<prefix>\s*version\s*=\s*")(?<version>[^"]+)(?<suffix>"\s*(?:#.*)?(?:\r?\n)?)$/;
const KINDS = new Set(["feature", "bug-fix", "major", "maintenance", "record-reconciliation"]);
const CONFLICT_RECORD_PATTERN = /^(?:[A-Za-z0-9._/-]+|git:refs\/[A-Za-z0-9._/-]+)#[A-Za-z0-9._/-]+$/;
const CONFLICT_OPTIONS = new Map([
  ["--conflict-event-id", "value"],
  ["--conflict-record-a", "value"],
  ["--conflict-value-a", "value"],
  ["--conflict-record-b", "value"],
  ["--conflict-value-b", "value"],
  ["--conflict-confirmed", "flag"],
]);
const COMMAND_OPTIONS = new Map([
  ["init", new Map([
    ["--project-root", "value"],
    ["--migration-approved", "flag"],
  ])],
  ["plan", new Map([
    ["--project-root", "value"],
    ["--kind", "value"],
    ["--change-id", "value"],
    ["--major", "value"],
    ["--user-approved", "flag"],
    ...CONFLICT_OPTIONS,
  ])],
  ["apply", new Map([
    ["--project-root", "value"],
    ["--kind", "value"],
    ["--change-id", "value"],
    ["--major", "value"],
    ["--user-approved", "flag"],
    ...CONFLICT_OPTIONS,
  ])],
  ["check", new Map([
    ["--project-root", "value"],
    ["--phase", "value"],
  ])],
  ["finalize-release", new Map([
    ["--project-root", "value"],
    ["--released-version", "value"],
    ["--source-commit", "value"],
    ["--release-succeeded", "flag"],
  ])],
]);

export class GateError extends Error {
  constructor(message) {
    super(message);
    this.name = "GateError";
  }
}

function decimalIsAtMost(value, maximum) {
  const significant = value.replace(/^0+/, "") || "0";
  return significant.length < maximum.length
    || (significant.length === maximum.length && significant <= maximum);
}

export class Version {
  constructor(major, minor, patchComponent) {
    this.major = typeof major === "bigint" ? major : BigInt(major);
    this.minor = minor;
    this.patch = patchComponent;
    Object.freeze(this);
  }

  static parse(raw) {
    if (typeof raw !== "string") throw new GateError("version value must be a string");
    const match = SEMVER_PATTERN.exec(raw);
    if (!match) throw new GateError(`unsupported version ${JSON.stringify(raw)}; expected stable MAJOR.MINOR.PATCH`);
    const [, majorText, minorText, patchText] = match;
    if (!decimalIsAtMost(majorText, CARGO_SEMVER_COMPONENT_MAX_TEXT)) {
      throw new GateError("major component exceeds Cargo u64::MAX");
    }
    if (!decimalIsAtMost(minorText, "99") || !decimalIsAtMost(patchText, "99")) {
      throw new GateError("minor and patch components outside supported range 0..99");
    }
    return new Version(BigInt(majorText), Number(minorText), Number(patchText));
  }

  normalized() {
    const minor = this.minor + Math.floor(this.patch / 100);
    const patchComponent = this.patch % 100;
    const major = this.major + BigInt(Math.floor(minor / 100));
    if (major > CARGO_SEMVER_COMPONENT_MAX) {
      throw new GateError("automatic Major carry exceeds Cargo u64::MAX");
    }
    return new Version(major, minor % 100, patchComponent);
  }

  bumpMinor() {
    const current = this.normalized();
    return new Version(current.major, current.minor + 1, 0).normalized();
  }

  bumpPatch() {
    const current = this.normalized();
    return new Version(current.major, current.minor, current.patch + 1).normalized();
  }

  compare(other) {
    if (this.major !== other.major) return this.major < other.major ? -1 : 1;
    if (this.minor !== other.minor) return this.minor < other.minor ? -1 : 1;
    if (this.patch !== other.patch) return this.patch < other.patch ? -1 : 1;
    return 0;
  }

  equals(other) { return this.compare(other) === 0; }
  toString() { return `${this.major}.${this.minor}.${this.patch}`; }
}

function lstatOrNull(target) {
  try { return lstatSync(target); } catch (error) {
    if (error?.code === "ENOENT" || error?.code === "ENOTDIR") return null;
    throw error;
  }
}

function requireRegularFile(target, label) {
  const stat = lstatOrNull(target);
  if (!stat || stat.isSymbolicLink() || !stat.isFile()) {
    throw new GateError(`${label} must be a regular non-symlink file: ${target}`);
  }
}

function readUtf8FileStrict(target, label) {
  const bytes = readFileSync(target);
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw new GateError(`${label} must contain valid UTF-8: ${target}`);
  }
}

function projectDirectory(raw) {
  const candidatePath = path.resolve(raw);
  const unresolvedStat = lstatOrNull(raw);
  if (unresolvedStat?.isSymbolicLink()) throw new GateError(`project root must not be a symlink: ${raw}`);
  const stat = lstatOrNull(candidatePath);
  if (!stat?.isDirectory()) throw new GateError(`project root is not a directory: ${candidatePath}`);
  return realpathSync(candidatePath);
}

export function initializationRoot(raw) { return projectDirectory(raw); }

function runGit(root, args) {
  return spawnSync("git", args, { cwd: root, encoding: "utf8" });
}

function isIndependentGitRoot(root) {
  if (lstatOrNull(path.join(root, ".git"))) return true;
  const result = runGit(root, ["rev-parse", "--show-toplevel"]);
  return result.status === 0 && realpathSync(result.stdout.trim()) === root;
}

export function projectRoot(raw) {
  const root = projectDirectory(raw);
  const result = runGit(root, ["rev-parse", "--show-toplevel"]);
  if (result.status !== 0) throw new GateError("project root must be inside an independent Git repository");
  const gitRoot = realpathSync(result.stdout.trim());
  if (gitRoot !== root) throw new GateError(`Git top-level ${gitRoot} does not equal project root ${root}`);
  return root;
}

function splitLinesKeepingEnds(text) {
  return text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
}

function locateVersionLine(text) {
  let section = null;
  const matches = [];
  for (const [index, line] of splitLinesKeepingEnds(text).entries()) {
    const stripped = line.trim();
    if (stripped.startsWith("[") && stripped.endsWith("]")) {
      section = stripped.slice(1, -1).trim();
      continue;
    }
    if (section === "workspace.package") {
      const match = VERSION_LINE_PATTERN.exec(line);
      if (match) matches.push([index, match]);
    }
  }
  return matches;
}

export function cargoVersion(target) {
  requireRegularFile(target, "root Cargo.toml");
  const text = readUtf8FileStrict(target, "root Cargo.toml");
  const matches = locateVersionLine(text);
  if (matches.length !== 1) {
    throw new GateError("root Cargo.toml must contain exactly one string version in [workspace.package]");
  }
  return [Version.parse(matches[0][1].groups.version), text];
}

function replaceCargoVersion(text, version) {
  const matches = locateVersionLine(text);
  if (matches.length !== 1) throw new GateError("unable to update exactly one [workspace.package].version");
  const lines = splitLinesKeepingEnds(text);
  const [index, match] = matches[0];
  lines[index] = `${match.groups.prefix}${version}${match.groups.suffix}`;
  return lines.join("");
}

/** 已接入项目可声明实际安装包使用的 JSON 版本镜像，未声明时保持现有布局。 */
function versionMirrorPaths(root) {
  const cargo = readUtf8FileStrict(path.join(root, "Cargo.toml"), "root Cargo.toml");
  const lines = cargo.split(/\r?\n/u);
  const starts = lines.flatMap((line, index) => line.trim() === "[workspace.metadata.agent-first-harness]" ? [index] : []);
  if (starts.length === 0) return [];
  if (starts.length !== 1) throw new GateError("workspace metadata section must be unique");
  const end = lines.findIndex((line, index) => index > starts[0] && /^\s*\[/u.test(line));
  const section = lines.slice(starts[0] + 1, end < 0 ? undefined : end).join("\n");
  const mentions = [...section.matchAll(/^\s*version-mirrors\s*=/gmu)];
  const declarations = [...section.matchAll(/^\s*version-mirrors\s*=\s*(\[[^\n]*\])\s*$/gmu)];
  if (mentions.length === 0) return [];
  if (mentions.length !== 1 || declarations.length !== 1) throw new GateError("version-mirrors must be a single inline string array");
  let values;
  try { values = JSON.parse(declarations[0][1]); }
  catch { throw new GateError("version-mirrors must be a JSON-compatible TOML string array"); }
  if (!Array.isArray(values) || values.length === 0 || values.length > 10 || new Set(values).size !== values.length
      || values.some((value) => typeof value !== "string" || !value.endsWith(".json") || value.includes("\\")
        || value.includes(":") || path.isAbsolute(value)
        || value.startsWith("/") || value.split("/").some((segment) => !segment || segment === "." || segment === ".."))) {
    throw new GateError("version-mirrors must list 1..10 unique project-relative JSON paths");
  }
  return values;
}

function versionMirrors(root, current) {
  return versionMirrorPaths(root).map((relative) => {
    const segments = relative.split("/");
    let cursor = root;
    for (const segment of segments) {
      cursor = path.join(cursor, segment);
      if (lstatOrNull(cursor)?.isSymbolicLink()) throw new GateError(`version mirror path contains a symlink: ${relative}`);
    }
    requireRegularFile(cursor, `version mirror ${relative}`);
    const text = readUtf8FileStrict(cursor, `version mirror ${relative}`);
    let parsed;
    try { parsed = JSON.parse(text); }
    catch { throw new GateError(`version mirror must contain valid JSON: ${relative}`); }
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object" || typeof parsed.version !== "string") {
      throw new GateError(`version mirror must contain a top-level string version: ${relative}`);
    }
    const observed = Version.parse(parsed.version);
    if (!observed.equals(current)) throw new GateError(`version drift: ${relative}=${observed}, Cargo.toml=${current}`);
    const matches = [...text.matchAll(/"version"\s*:\s*"([^"\\]*)"/gu)];
    if (matches.length !== 1 || matches[0][1] !== parsed.version) {
      throw new GateError(`version mirror has ambiguous version text: ${relative}`);
    }
    return { relative, file: cursor, text, match: matches[0] };
  });
}

function replaceMirrorVersion(mirror, version) {
  const match = mirror.match;
  return `${mirror.text.slice(0, match.index)}${match[0].replace(`"${match[1]}"`, `"${version}"`)}${mirror.text.slice(match.index + match[0].length)}`;
}

function atomicWrite(target, content) {
  mkdirSync(path.dirname(target), { recursive: true });
  if (lstatOrNull(path.dirname(target))?.isSymbolicLink()) {
    throw new GateError(`refusing symlink state directory: ${path.dirname(target)}`);
  }
  const mode = existsSync(target) ? statSync(target).mode & 0o777 : 0o644;
  const temporary = path.join(path.dirname(target), `.${path.basename(target)}.${randomUUID()}`);
  let descriptor;
  try {
    descriptor = openSync(temporary, "wx", mode);
    writeFileSync(descriptor, content, { encoding: "utf8" });
    fsyncSync(descriptor);
    closeSync(descriptor);
    descriptor = undefined;
    chmodSync(temporary, mode);
    renameSync(temporary, target);
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
    rmSync(temporary, { force: true });
  }
}

function newState(version) {
  return {
    schema_version: 1,
    cycle_base_version: version.toString(),
    target_version: version.toString(),
    feature_bump_applied: false,
    pending_changes: [],
    applied_bug_ids: [],
    applied_reconciliations: [],
    last_release: null,
  };
}

function sortObject(value) {
  if (Array.isArray(value)) return value.map(sortObject);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortObject(value[key])]));
  }
  return value;
}

function stateJson(state) { return `${JSON.stringify(sortObject(state), null, 2)}\n`; }

function exactKeys(value, keys) {
  return value !== null && !Array.isArray(value) && typeof value === "object"
    && Object.keys(value).length === keys.size
    && Object.keys(value).every((key) => keys.has(key));
}

function validateChangeId(changeId) {
  if (typeof changeId !== "string" || !CHANGE_ID_PATTERN.test(changeId)) {
    throw new GateError("change_id must be 1..128 characters using letters, digits, dot, underscore, colon, slash, or hyphen");
  }
  return changeId;
}

/** 校验经人工核实的同一事件冲突证据；取值保留在受保护状态中供重试对账。 */
function validateConflictEvidence(evidence) {
  if (!exactKeys(evidence, new Set(["event_id", "record_a", "value_a", "record_b", "value_b", "confirmed"]))) {
    throw new GateError("record-reconciliation requires complete conflict evidence");
  }
  validateChangeId(evidence.event_id);
  for (const key of ["record_a", "record_b"]) {
    const value = evidence[key];
    if (typeof value !== "string" || value.length > 256 || !CONFLICT_RECORD_PATTERN.test(value) || value.includes("..")) {
      throw new GateError(`${key} must identify a relative record and field or a Git ref and field`);
    }
  }
  if (evidence.record_a === evidence.record_b) throw new GateError("conflict records must be distinct");
  for (const key of ["value_a", "value_b"]) {
    const value = evidence[key];
    if (typeof value !== "string" || value.length === 0 || value.length > 500 || value !== value.trim()
        || [...value].some((character) => character.codePointAt(0) < 32 || character.codePointAt(0) === 127)) {
      throw new GateError(`${key} must be 1..500 trimmed printable characters`);
    }
  }
  if (evidence.value_a === evidence.value_b) throw new GateError("conflict records must have different observed values");
  if (evidence.confirmed !== true) throw new GateError("record-reconciliation requires --conflict-confirmed after checking both records refer to the same event");
  return evidence;
}

function loadState(target) {
  requireRegularFile(target, "version state");
  let state;
  const text = readUtf8FileStrict(target, "version state");
  try { state = JSON.parse(text); }
  catch { throw new GateError(`invalid version state: ${target}`); }
  const required = new Set([
    "schema_version", "cycle_base_version", "target_version", "feature_bump_applied",
    "pending_changes", "applied_bug_ids", "last_release",
  ]);
  const withHistory = new Set([...required, "applied_reconciliations"]);
  if (!exactKeys(state, required) && !exactKeys(state, withHistory)) throw new GateError("version state has unexpected schema fields");
  const hasReconciliationHistory = Object.hasOwn(state, "applied_reconciliations");
  if (!hasReconciliationHistory) state.applied_reconciliations = [];
  if (state.schema_version !== 1) throw new GateError("unsupported version state schema");
  const base = Version.parse(state.cycle_base_version);
  const targetVersion = Version.parse(state.target_version);
  if (targetVersion.compare(base) < 0) throw new GateError("target_version cannot be lower than cycle_base_version");
  if (typeof state.feature_bump_applied !== "boolean") throw new GateError("feature_bump_applied must be boolean");
  if (!Array.isArray(state.pending_changes)) throw new GateError("pending_changes must be an array");
  if (!Array.isArray(state.applied_bug_ids)) throw new GateError("applied_bug_ids must be an array");
  if (!Array.isArray(state.applied_reconciliations)) throw new GateError("applied_reconciliations must be an array");
  const reconciliationIds = new Set();
  for (const item of state.applied_reconciliations) {
    if (!exactKeys(item, new Set(["change_id", "conflict_evidence", "required_version"]))) {
      throw new GateError("applied reconciliation has unexpected fields");
    }
    validateChangeId(item.change_id);
    validateConflictEvidence(item.conflict_evidence);
    if (Version.parse(item.required_version).compare(targetVersion) > 0) {
      throw new GateError("applied reconciliation required_version cannot exceed target_version");
    }
    if (reconciliationIds.has(item.change_id)) throw new GateError("applied reconciliation IDs must be unique");
    reconciliationIds.add(item.change_id);
  }
  const seen = new Set();
  for (const item of state.pending_changes) {
    const reconcile = item?.kind === "record-reconciliation";
    const keys = reconcile
      ? new Set(["change_id", "kind", "required_version", "conflict_evidence"])
      : new Set(["change_id", "kind", "required_version"]);
    if (!exactKeys(item, keys)) throw new GateError("pending change has unexpected fields");
    validateChangeId(item.change_id);
    if (typeof item.kind !== "string" || !new Set(["feature", "bug-fix", "major", "record-reconciliation"]).has(item.kind)) {
      throw new GateError("pending change has unsupported kind");
    }
    if (reconcile) validateConflictEvidence(item.conflict_evidence);
    if (reconcile) {
      const historical = state.applied_reconciliations.find((entry) => entry.change_id === item.change_id);
      if (hasReconciliationHistory && !historical) {
        throw new GateError("pending reconciliation must exist in retained history");
      }
      if (historical && (historical.required_version !== item.required_version
          || JSON.stringify(sortObject(historical.conflict_evidence)) !== JSON.stringify(sortObject(item.conflict_evidence)))) {
        throw new GateError("pending reconciliation must match retained history");
      }
    } else if (reconciliationIds.has(item.change_id)) {
      throw new GateError("historical reconciliation IDs cannot be reused by another kind");
    }
    if (Version.parse(item.required_version).compare(targetVersion) > 0) {
      throw new GateError("pending required_version cannot exceed target_version");
    }
    if (seen.has(item.change_id)) throw new GateError("pending change IDs must be unique");
    seen.add(item.change_id);
  }
  if (!hasReconciliationHistory) {
    state.applied_reconciliations = state.pending_changes
      .filter((item) => item.kind === "record-reconciliation")
      .map((item) => ({ change_id: item.change_id, conflict_evidence: structuredClone(item.conflict_evidence), required_version: item.required_version }));
    state.applied_reconciliations.forEach((item) => reconciliationIds.add(item.change_id));
  }
  const reconciliationEvents = new Set();
  for (const item of state.applied_reconciliations) {
    const eventId = item.conflict_evidence.event_id;
    if (reconciliationEvents.has(eventId)) throw new GateError("reconciliation event IDs must be unique across change IDs");
    reconciliationEvents.add(eventId);
  }
  const bugIds = state.applied_bug_ids;
  if (bugIds.some((item) => typeof item !== "string") || new Set(bugIds).size !== bugIds.length) {
    throw new GateError("applied_bug_ids must contain unique strings");
  }
  bugIds.forEach(validateChangeId);
  const bugIdSet = new Set(bugIds);
  if ([...reconciliationIds].some((id) => bugIdSet.has(id))) {
    throw new GateError("reconciliation IDs cannot reuse historical bug-fix IDs");
  }
  const requiresFeatureLock = state.pending_changes.some((item) => ["feature", "major", "record-reconciliation"].includes(item.kind));
  if (state.feature_bump_applied !== requiresFeatureLock) {
    throw new GateError("feature_bump_applied must match pending feature, major, or record-reconciliation changes");
  }
  const pendingBugIds = state.pending_changes.filter((item) => item.kind === "bug-fix").map((item) => item.change_id);
  if (pendingBugIds.some((id) => !bugIdSet.has(id))) throw new GateError("pending bug-fix IDs must exist in applied_bug_ids");
  const pendingNonBugIds = state.pending_changes.filter((item) => item.kind !== "bug-fix").map((item) => item.change_id);
  if (pendingNonBugIds.some((id) => bugIdSet.has(id))) throw new GateError("historical bug-fix IDs cannot be reused by another kind");
  if (state.last_release !== null) {
    if (!exactKeys(state.last_release, new Set(["source_commit", "version"]))) throw new GateError("last_release has unexpected fields");
    if (!Version.parse(state.last_release.version).equals(base)) throw new GateError("last_release version must equal cycle_base_version");
    if (typeof state.last_release.source_commit !== "string" || !SOURCE_COMMIT_PATTERN.test(state.last_release.source_commit)) {
      throw new GateError("last_release source_commit must be 40 hexadecimal characters");
    }
  }
  return state;
}

function consistentContext(root) {
  const cargoPath = path.join(root, "Cargo.toml");
  const statePath = path.join(root, STATE_RELATIVE);
  const [current, cargoText] = cargoVersion(cargoPath);
  const state = loadState(statePath);
  const target = Version.parse(state.target_version);
  if (!current.equals(target)) throw new GateError(`version drift: Cargo.toml=${current}, state target=${target}`);
  const mirrors = versionMirrors(root, current);
  return { cargoPath, statePath, current, cargoText, state, mirrors };
}

/** 只读读取 Git common-dir 的发布状态；首次开发周期尚无生命周期文件时返回空。 */
function readLifecycleState(root, { required = false } = {}) {
  const commonResult = runGit(root, ["rev-parse", "--path-format=absolute", "--git-common-dir"]);
  if (commonResult.status !== 0) throw new GateError("Git common directory is unavailable");
  const commonDir = path.resolve(root, commonResult.stdout.trim());
  const lifecyclePath = path.join(commonDir, "agent-first-harness", "git-lifecycle.json");
  if (!lstatOrNull(lifecyclePath) && !required) return null;
  requireRegularFile(lifecyclePath, "Git lifecycle state");
  let lifecycle;
  try { lifecycle = JSON.parse(readUtf8FileStrict(lifecyclePath, "Git lifecycle state")); }
  catch { throw new GateError("Git lifecycle state is invalid"); }
  if (!lifecycle || typeof lifecycle !== "object" || Array.isArray(lifecycle)
      || !Object.hasOwn(lifecycle, "lastRelease")) {
    throw new GateError("Git lifecycle release state is invalid");
  }
  return lifecycle;
}

/** 新 Git 发布必须先在下一开发分支 finalize，不能沿用旧功能锁计算版本。 */
function requireCurrentReleaseCycle(root, state) {
  const lifecycle = readLifecycleState(root);
  if (lifecycle === null) return;
  if (lifecycle.cycle?.pendingRelease) {
    throw new GateError("Git release is still pending; version plan/apply requires a completed release");
  }
  const gitRelease = lifecycle.lastRelease;
  if (gitRelease === null) return;
  if (!gitRelease || typeof gitRelease !== "object" || Array.isArray(gitRelease)
      || typeof gitRelease.head !== "string" || !SOURCE_COMMIT_PATTERN.test(gitRelease.head)) {
    throw new GateError("Git lifecycle lastRelease is invalid");
  }
  const gitVersion = Version.parse(gitRelease.version);
  const finalized = state.last_release;
  if (finalized === null || gitVersion.compare(Version.parse(finalized.version)) > 0) {
    throw new GateError("Git release is newer than version state; create the next feature branch and run finalize-release before plan/apply");
  }
  if (gitVersion.compare(Version.parse(finalized.version)) !== 0
      || gitRelease.head.toLowerCase() !== finalized.source_commit.toLowerCase()) {
    throw new GateError("Git lifecycle release and version state disagree; investigate before plan/apply");
  }
}

export function initialize(root, { migrationApproved = false } = {}) {
  const cargoPath = path.join(root, "Cargo.toml");
  const statePath = path.join(root, STATE_RELATIVE);
  if (lstatOrNull(statePath)) {
    const { current, state } = consistentContext(root);
    return {
      action: "init", changed: false, current_version: current.toString(), state: STATE_RELATIVE,
      pending_change_count: state.pending_changes.length, migration: false,
      history_status: "preserved-existing-state", unrecoverable_history: [],
    };
  }
  const [version] = cargoVersion(cargoPath);
  versionMirrors(root, version);
  const migration = isIndependentGitRoot(root);
  if (migration && !migrationApproved) {
    throw new GateError(
      "version state is missing in an existing independent Git project; rerun init with --migration-approved only after explicit user approval; prior pending_changes, applied_bug_ids, and last_release history cannot be recovered",
    );
  }
  if (lstatOrNull(path.dirname(statePath))?.isSymbolicLink()) throw new GateError(`refusing symlink state directory: ${path.dirname(statePath)}`);
  atomicWrite(statePath, stateJson(newState(version)));
  return {
    action: "init", changed: true, current_version: version.toString(), state: STATE_RELATIVE,
    pending_change_count: 0, migration,
    history_status: migration ? "unrecoverable-pre-migration-history" : "new-project-empty-baseline",
    unrecoverable_history: migration ? ["pending_changes", "applied_bug_ids", "last_release"] : [],
  };
}

function idempotentResult(requiredVersion, reason) {
  return { required_version: requiredVersion, version_bumped: false, idempotent: true, reason };
}

function transition(state, current, { kind, changeId, major, userApproved, conflictEvidence = null }) {
  const nextState = structuredClone(state);
  if (kind === "maintenance") return [current, nextState, idempotentResult(current.toString(), "maintenance-does-not-change-version")];
  const stableId = validateChangeId(changeId);
  if (kind === "record-reconciliation") validateConflictEvidence(conflictEvidence);
  const existing = state.pending_changes.find((item) => item.change_id === stableId);
  if (existing) {
    if (existing.kind !== kind) throw new GateError(`change_id ${JSON.stringify(stableId)} is already used by kind ${JSON.stringify(existing.kind)}`);
    if (kind === "record-reconciliation" && JSON.stringify(sortObject(existing.conflict_evidence)) !== JSON.stringify(sortObject(conflictEvidence))) {
      throw new GateError(`change_id ${JSON.stringify(stableId)} has different conflict evidence`);
    }
    return [current, nextState, idempotentResult(existing.required_version, "change-already-applied")];
  }
  const historicalReconciliation = state.applied_reconciliations.find((item) => item.change_id === stableId);
  if (historicalReconciliation) {
    if (kind !== "record-reconciliation") {
      throw new GateError(`change_id ${JSON.stringify(stableId)} is already used by kind 'record-reconciliation'`);
    }
    if (JSON.stringify(sortObject(historicalReconciliation.conflict_evidence)) !== JSON.stringify(sortObject(conflictEvidence))) {
      throw new GateError(`change_id ${JSON.stringify(stableId)} has different conflict evidence`);
    }
    return [current, nextState, idempotentResult(historicalReconciliation.required_version, "reconciliation-id-already-consumed")];
  }
  if (state.applied_bug_ids.includes(stableId)) {
    if (kind !== "bug-fix") throw new GateError(`change_id ${JSON.stringify(stableId)} is already used by kind 'bug-fix'`);
    return [current, nextState, idempotentResult(current.toString(), "bug-id-already-consumed")];
  }
  if (kind === "record-reconciliation" && state.applied_reconciliations.some(
    (item) => item.conflict_evidence.event_id === conflictEvidence.event_id,
  )) {
    throw new GateError(`conflict event ${JSON.stringify(conflictEvidence.event_id)} is already associated with another reconciliation ID`);
  }
  let nextVersion = current;
  let reason;
  if (kind === "feature") {
    if (state.feature_bump_applied) reason = "feature-bump-already-applied-in-release-cycle";
    else {
      nextVersion = current.bumpMinor();
      nextState.feature_bump_applied = true;
      reason = "first-feature-in-release-cycle";
    }
  } else if (kind === "bug-fix") {
    nextVersion = current.bumpPatch();
    nextState.applied_bug_ids.push(stableId);
    reason = "distinct-completed-bug-fix";
  } else if (kind === "record-reconciliation") {
    if (state.pending_changes.some((item) => item.kind === "record-reconciliation")) {
      reason = "record-reconciliation-already-applied-in-release-cycle";
    } else {
      nextVersion = current.bumpMinor();
      reason = "first-record-reconciliation-in-release-cycle";
    }
    nextState.feature_bump_applied = true;
  } else if (kind === "major") {
    if (!userApproved) throw new GateError("Major change requires explicit --user-approved");
    let approvedMajor;
    try { approvedMajor = typeof major === "bigint" ? major : BigInt(major); }
    catch { throw new GateError("approved Major must be inside Cargo u64 range"); }
    if (approvedMajor < 0n || approvedMajor > CARGO_SEMVER_COMPONENT_MAX) throw new GateError("approved Major must be inside Cargo u64 range");
    if (approvedMajor <= current.normalized().major) throw new GateError("approved Major must be greater than the current Major");
    nextVersion = new Version(approvedMajor, 0, 0);
    nextState.feature_bump_applied = true;
    reason = "explicit-user-approved-major";
  } else throw new GateError(`unsupported change kind: ${kind}`);
  nextState.target_version = nextVersion.toString();
  nextState.pending_changes.push({
    change_id: stableId, kind, required_version: nextVersion.toString(),
    ...(kind === "record-reconciliation" ? { conflict_evidence: structuredClone(conflictEvidence) } : {}),
  });
  if (kind === "record-reconciliation") {
    nextState.applied_reconciliations.push({
      change_id: stableId, conflict_evidence: structuredClone(conflictEvidence), required_version: nextVersion.toString(),
    });
  }
  return [nextVersion, nextState, {
    required_version: nextVersion.toString(), version_bumped: !nextVersion.equals(current), idempotent: false, reason,
  }];
}

export function evaluateChange(root, {
  action, kind, changeId = null, major = null, userApproved = false, conflictEvidence = null,
}) {
  const { cargoPath, statePath, current, cargoText, state, mirrors } = consistentContext(root);
  requireCurrentReleaseCycle(root, state);
  const [nextVersion, nextState, result] = transition(state, current, {
    kind, changeId, major, userApproved, conflictEvidence,
  });
  const changed = JSON.stringify(nextState) !== JSON.stringify(state) || !nextVersion.equals(current);
  if (action === "apply" && changed) {
    const newCargo = replaceCargoVersion(cargoText, nextVersion);
    const changes = [
      ...(newCargo === cargoText ? [] : [{ file: cargoPath, before: cargoText, after: newCargo }]),
      ...mirrors.map((mirror) => ({ file: mirror.file, before: mirror.text, after: replaceMirrorVersion(mirror, nextVersion) }))
        .filter((item) => item.after !== item.before),
    ];
    const written = [];
    try {
      for (const change of changes) {
        if (readUtf8FileStrict(change.file, "version source") !== change.before) throw new GateError(`version source changed before apply: ${change.file}`);
        atomicWrite(change.file, change.after);
        written.push(change);
      }
      atomicWrite(statePath, stateJson(nextState));
    } catch (error) {
      const rollbackErrors = [];
      for (const change of written.reverse()) {
        try { atomicWrite(change.file, change.before); }
        catch (rollbackError) { rollbackErrors.push(`${change.file}: ${rollbackError.message}`); }
      }
      if (rollbackErrors.length) throw new GateError(`version apply failed and rollback is incomplete: ${error.message}; ${rollbackErrors.join("; ")}`);
      throw error;
    }
  }
  return {
    action, kind, change_id: changeId, before_version: current.toString(), after_version: nextVersion.toString(),
    changed: action === "apply" && changed, pending_change_count: nextState.pending_changes.length,
    ...(kind === "record-reconciliation" ? { conflict_event_id: conflictEvidence.event_id } : {}),
    ...result,
  };
}

export function check(root, phase) {
  const { current, state, mirrors } = consistentContext(root);
  return {
    action: "check", phase, current_version: current.toString(), cycle_base_version: state.cycle_base_version,
    feature_bump_applied: state.feature_bump_applied, pending_change_count: state.pending_changes.length,
    version_mirrors: mirrors.map((mirror) => mirror.relative), passed: true,
  };
}

/** 只读复核已完成的本地 Git 发布，避免仅凭调用者声明重置受保护版本周期。 */
function verifiedLocalRelease(root, released, sourceCommit) {
  const lifecycle = readLifecycleState(root, { required: true });
  const last = lifecycle?.lastRelease;
  const branch = lifecycle?.defaultBranch;
  if (typeof branch !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._/-]*$/.test(branch) || branch.includes("..")
      || branch.startsWith("-") || !last || last.defaultBranch !== branch
      || last.version !== released.toString() || last.head !== sourceCommit.toLowerCase()
      || typeof last.date !== "string" || !/^[0-9]{8}$/.test(last.date)
      || last.tag !== `v${released}-${last.date}`) {
    throw new GateError("Git lifecycle release does not match the requested version, commit, and default branch");
  }
  const headResult = runGit(root, ["rev-parse", "--verify", "HEAD"]);
  const branchResult = runGit(root, ["symbolic-ref", "--quiet", "--short", "HEAD"]);
  const mainResult = runGit(root, ["rev-parse", "--verify", `refs/heads/${branch}^{commit}`]);
  const tagResult = runGit(root, ["rev-parse", "--verify", `refs/tags/${last.tag}^{commit}`]);
  if ([headResult, branchResult, mainResult, tagResult].some((result) => result.status !== 0)) {
    throw new GateError("local default branch or release tag cannot be verified");
  }
  const workBranch = branchResult.stdout.trim();
  if (!workBranch.startsWith("feature-") || workBranch === branch || headResult.stdout.trim() !== sourceCommit.toLowerCase()) {
    throw new GateError("finalize-release must run on a new feature branch before its first change");
  }
  if (mainResult.stdout.trim() !== sourceCommit.toLowerCase() || tagResult.stdout.trim() !== sourceCommit.toLowerCase()) {
    throw new GateError("local default branch and release tag must point to source_commit");
  }
  const tagList = runGit(root, ["for-each-ref", "--format=%(refname:short)", "refs/tags"]);
  if (tagList.status !== 0) throw new GateError("local release tags cannot be listed");
  const versionTagPattern = new RegExp(`^v${released.toString().replaceAll(".", "\\.")}-[0-9]{8}$`);
  const versionTags = tagList.stdout.trim().split(/\r?\n/).filter((name) => versionTagPattern.test(name));
  if (!versionTags.includes(last.tag)) throw new GateError("recorded local release tag is missing");
  for (const tag of versionTags) {
    const result = runGit(root, ["rev-parse", "--verify", `refs/tags/${tag}^{commit}`]);
    if (result.status !== 0 || result.stdout.trim() !== sourceCommit.toLowerCase()) {
      throw new GateError("same-version local release tags point to different commits");
    }
  }
  return { default_branch: branch, tag: last.tag };
}

export function finalizeRelease(root, releasedVersion, sourceCommit) {
  const { statePath, current, state } = consistentContext(root);
  const released = Version.parse(releasedVersion);
  if (!released.equals(current)) throw new GateError(`released version ${released} does not equal current target ${current}`);
  if (!SOURCE_COMMIT_PATTERN.test(sourceCommit)) throw new GateError("source_commit must be exactly 40 hexadecimal characters");
  const release = verifiedLocalRelease(root, released, sourceCommit);
  const nextState = structuredClone(state);
  nextState.cycle_base_version = released.toString();
  nextState.feature_bump_applied = false;
  nextState.pending_changes = [];
  nextState.last_release = { source_commit: sourceCommit.toLowerCase(), version: released.toString() };
  const changed = JSON.stringify(nextState) !== JSON.stringify(state);
  if (changed) atomicWrite(statePath, stateJson(nextState));
  return {
    action: "finalize-release", changed,
    released_version: released.toString(), source_commit: sourceCommit.toLowerCase(),
    default_branch: release.default_branch, tag: release.tag,
    retained_bug_id_count: nextState.applied_bug_ids.length,
    retained_reconciliation_count: nextState.applied_reconciliations.length,
    pending_change_count: 0,
  };
}

function parseArguments(argv) {
  const [command, ...tokens] = argv;
  const optionKinds = COMMAND_OPTIONS.get(command);
  if (!optionKinds) throw new GateError("unsupported command");
  const values = new Map();
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const optionKind = optionKinds.get(token);
    if (!optionKind) throw new GateError(`unsupported argument for ${command}: ${token}`);
    if (optionKind === "flag") {
      values.set(token, true);
      continue;
    }
    const value = tokens[index + 1];
    if (value === undefined || value.startsWith("--")) throw new GateError(`${token} requires a value`);
    values.set(token, value);
    index += 1;
  }
  if (!values.has("--project-root")) throw new GateError("--project-root is required");
  if ((command === "plan" || command === "apply") && !KINDS.has(values.get("--kind"))) throw new GateError("--kind is required and must be supported");
  if (command === "plan" || command === "apply") {
    const conflictKeys = [...CONFLICT_OPTIONS.keys()];
    if (values.get("--kind") === "record-reconciliation") {
      for (const key of conflictKeys) if (!values.has(key)) throw new GateError(`${key} is required for record-reconciliation`);
    } else if (conflictKeys.some((key) => values.has(key))) {
      throw new GateError("conflict evidence options require --kind record-reconciliation");
    }
  }
  if (values.has("--major") && !/^[0-9]+$/.test(values.get("--major"))) {
    throw new GateError("--major must be a decimal integer");
  }
  if (command === "check" && !new Set(["development", "build", "release"]).has(values.get("--phase"))) throw new GateError("--phase is required and must be supported");
  if (command === "finalize-release") {
    for (const option of ["--released-version", "--source-commit", "--release-succeeded"]) {
      if (!values.has(option)) throw new GateError(`${option} is required`);
    }
  }
  return { command, values };
}

export function main(argv = process.argv.slice(2)) {
  try {
    const { command, values } = parseArguments(argv);
    const root = command === "init" ? initializationRoot(values.get("--project-root")) : projectRoot(values.get("--project-root"));
    let result;
    if (command === "init") result = initialize(root, { migrationApproved: values.has("--migration-approved") });
    else if (command === "plan" || command === "apply") {
      result = evaluateChange(root, {
        action: command,
        kind: values.get("--kind"),
        changeId: values.get("--change-id") ?? null,
        major: values.has("--major") ? values.get("--major") : null,
        userApproved: values.has("--user-approved"),
        conflictEvidence: values.get("--kind") === "record-reconciliation" ? {
          event_id: values.get("--conflict-event-id"),
          record_a: values.get("--conflict-record-a"),
          value_a: values.get("--conflict-value-a"),
          record_b: values.get("--conflict-record-b"),
          value_b: values.get("--conflict-value-b"),
          confirmed: values.has("--conflict-confirmed"),
        } : null,
      });
    } else if (command === "check") result = check(root, values.get("--phase"));
    else result = finalizeRelease(root, values.get("--released-version"), values.get("--source-commit"));
    process.stdout.write(`${JSON.stringify(sortObject(result))}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`${JSON.stringify({ error: error.message })}\n`);
    return 2;
  }
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) process.exitCode = main();
