#!/usr/bin/env node
/** 隔离验证并行 Worktree 助手的项目绑定、所有权与保守清理。 */

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const SCRIPT = fileURLToPath(new URL("./parallel_worktrees.mjs", import.meta.url));
const MOCK_LIFECYCLE = `#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { parseArgs } from "node:util";
const command = process.argv[2];
const { values } = parseArgs({ args: process.argv.slice(3), options: { "project-root": { type: "string" }, worktree: { type: "string" } }, strict: true });
const project = fs.realpathSync(values["project-root"]);
const raw = spawnSync("git", ["-C", project, "rev-parse", "--git-common-dir"], { encoding: "utf8" }).stdout.trim();
const common = fs.realpathSync(path.isAbsolute(raw) ? raw : path.join(project, raw));
const recordsPath = path.join(common, "mock-tracked-worktrees");
if (command === "inspect") {
  if (fs.existsSync(path.join(common, "mock-inspect-failure"))) process.exit(9);
  const records = fs.existsSync(recordsPath) ? fs.readdirSync(recordsPath).map((name) => JSON.parse(fs.readFileSync(path.join(recordsPath, name), "utf8"))) : [];
  const statePath = path.join(common, "agent-first-harness", "git-lifecycle.json");
  process.stdout.write(JSON.stringify({ status: "inspected", statePath, state: { schemaVersion: 4, cycle: { branches: records.map(({ branch }) => ({ name: branch })), worktrees: records.map(({ branch, worktree }) => ({ path: worktree, branch })) }, releasedResources: [] } }));
  process.exit(0);
}
const worktree = fs.realpathSync(values.worktree);
const branch = spawnSync("git", ["-C", worktree, "branch", "--show-current"], { encoding: "utf8" }).stdout.trim();
const unit = path.basename(worktree);
if (fs.existsSync(path.join(common, "mock-track-block-" + unit))) {
  fs.writeFileSync(path.join(common, "mock-track-entered-" + unit), "entered");
  const deadline = Date.now() + 15_000;
  const wait = new Int32Array(new SharedArrayBuffer(4));
  while (!fs.existsSync(path.join(common, "mock-track-release-" + unit))) {
    if (Date.now() >= deadline) process.exit(8);
    Atomics.wait(wait, 0, 0, 10);
  }
}
if (fs.existsSync(path.join(common, "mock-track-failure")) || fs.existsSync(path.join(common, "mock-track-failure-" + unit))) {
  process.stdout.write(JSON.stringify({ status: "error", code: "forced-test-failure", message: "forced lifecycle tracking failure" }));
  process.exit(7);
}
fs.mkdirSync(recordsPath, { recursive: true });
fs.writeFileSync(path.join(recordsPath, unit + ".json"), JSON.stringify({ branch, worktree }));
if (fs.existsSync(path.join(common, "mock-track-failure-after-save"))) {
  process.stdout.write(JSON.stringify({ status: "error", code: "state-lock-release-failed", message: "tracking was saved before lock cleanup failed" }));
  process.exit(7);
}
process.stdout.write(JSON.stringify({ status: "worktree-tracked", branch, worktree, remote: null, command }));
`;

function runGit(root, ...args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result;
}

function fixture(t, { realLifecycle = false } = {}) {
  const tempRoot = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "parallel_worktrees_")));
  t.after(() => fs.rmSync(tempRoot, { recursive: true, force: true }));
  const root = path.join(tempRoot, "sample_project");
  const source = path.join(tempRoot, "feature_task");
  fs.mkdirSync(root);
  runGit(root, "init", "-b", "main");
  runGit(root, "config", "user.name", "Harness Test");
  runGit(root, "config", "user.email", "harness-test@example.invalid");
  const lifecycleScript = path.join(root, ".agents", "skills", "desktop-manage-git-lifecycle", "scripts", "git_lifecycle.mjs");
  fs.mkdirSync(path.dirname(lifecycleScript), { recursive: true });
  if (realLifecycle) {
    fs.cpSync(fileURLToPath(new URL("../../desktop-manage-git-lifecycle/scripts/", import.meta.url)), path.dirname(lifecycleScript), { recursive: true });
    const dependencies = ["desktop-prepare-release/scripts/release_context.mjs", "desktop-prepare-release/scripts/harness_version_clock.mjs", "desktop-prepare-release/scripts/release_notes.mjs", "desktop-switch-post-release-action/scripts/post_release_action.mjs", "desktop-implement-change/scripts/project_lock_policy.mjs"];
    for (const relative of dependencies) {
      const target = path.join(root, ".agents", "skills", relative); fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.copyFileSync(fileURLToPath(new URL(`../../${relative}`, import.meta.url)), target);
    }
  }
  else fs.writeFileSync(lifecycleScript, MOCK_LIFECYCLE);
  fs.writeFileSync(path.join(root, "README.md"), "baseline\n");
  runGit(root, "add", "README.md", ".agents");
  runGit(root, "commit", "-m", "baseline");
  runGit(root, "worktree", "add", "-b", "feature-current-20260909", source, "HEAD");
  const ctx = { tempRoot, root, source };
  ctx.git = (cwd, ...args) => runGit(cwd ?? root, ...args);
  ctx.commonDir = () => {
    const raw = runGit(root, "rev-parse", "--git-common-dir").stdout.trim();
    return fs.realpathSync(path.isAbsolute(raw) ? raw : path.join(root, raw));
  };
  ctx.helper = (args, { cwd, projectRoot, sourceWorktree, nodeArgs = [] } = {}) => {
    const command = args[0];
    const invocation = [...nodeArgs, SCRIPT, ...args, "--project-root", projectRoot ?? root];
    if (command !== "inspect") invocation.push("--source-worktree", sourceWorktree ?? source);
    const result = spawnSync(process.execPath, invocation, { encoding: "utf8", cwd: cwd ?? (command === "inspect" ? root : (sourceWorktree ?? source)) });
    return [result, JSON.parse(result.stdout)];
  };
  ctx.create = (unit, ...ownership) => {
    const args = ["create", "--task", "feature", "--unit", unit];
    for (const target of ownership) args.push("--write-target", target);
    return ctx.helper(args);
  };
  ctx.spawnCreate = (unit, ...ownership) => ctx.spawnCreateWithNodeArgs([], unit, ...ownership);
  ctx.spawnCreateWithNodeArgs = (nodeArgs, unit, ...ownership) => {
    const args = [...nodeArgs, SCRIPT, "create", "--project-root", root, "--source-worktree", source, "--task", "feature", "--unit", unit];
    for (const target of ownership) args.push("--write-target", target);
    const child = spawn(process.execPath, args, { cwd: source, stdio: ["ignore", "pipe", "pipe"] });
    t.after(() => { if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL"); });
    let stdout = ""; let stderr = "";
    child.stdout.setEncoding("utf8"); child.stderr.setEncoding("utf8");
    child.stdout.on("data", (data) => { stdout += data; }); child.stderr.on("data", (data) => { stderr += data; });
    const completion = new Promise((resolve, reject) => {
      child.on("error", reject);
      child.on("close", (status, signal) => resolve([{ status, signal, stdout, stderr }, stdout ? JSON.parse(stdout) : null]));
    });
    return { child, completion };
  };
  ctx.tracked = () => {
    const directory = path.join(ctx.commonDir(), "mock-tracked-worktrees");
    return fs.existsSync(directory) ? fs.readdirSync(directory).sort().map((name) => JSON.parse(fs.readFileSync(path.join(directory, name), "utf8"))) : [];
  };
  return ctx;
}

function assertNoUnitCreateSideEffects(ctx, unit) {
  assert.equal(fs.existsSync(path.join(ctx.tempRoot, ".codex-worktrees", path.basename(ctx.root), "feature", unit)), false);
  assert.equal(fs.existsSync(path.join(ctx.commonDir(), "codex-parallel-worktrees", "feature", `${unit}.json`)), false);
  const branch = spawnSync("git", ["-C", ctx.root, "show-ref", "--verify", "--quiet", `refs/heads/codex/unit-feature-${unit}`]);
  assert.notEqual(branch.status, 0);
}

/** 有界等待隔离测试 marker，不依赖子进程调度先后或固定睡眠。 */
async function waitForFile(file) {
  const deadline = Date.now() + 10_000;
  while (!fs.existsSync(file)) {
    assert.ok(Date.now() < deadline, `等待 marker 超时：${file}`);
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

test("concurrent_create_overlaps_checkout_tracking_and_registers_exact_common_baseline", async (t) => {
  const ctx = fixture(t); const baseHead = runGit(ctx.source, "rev-parse", "HEAD").stdout.trim();
  const common = ctx.commonDir();
  for (const unit of ["one", "two"]) fs.writeFileSync(path.join(common, `mock-track-block-${unit}`), "block");
  const first = ctx.spawnCreate("one", "src"); const second = ctx.spawnCreate("two", "docs");
  await Promise.all(["one", "two"].map((unit) => waitForFile(path.join(common, `mock-track-entered-${unit}`))));
  for (const unit of ["one", "two"]) {
    const reserved = JSON.parse(fs.readFileSync(path.join(common, "codex-parallel-worktrees", "feature", `${unit}.json`), "utf8"));
    assert.equal(reserved.status, "creating"); assert.equal(reserved.baseHead, baseHead);
    assert.equal(runGit(reserved.worktreePath, "rev-parse", "HEAD").stdout.trim(), baseHead);
    fs.writeFileSync(path.join(common, `mock-track-release-${unit}`), "release");
  }
  const results = await Promise.all([first.completion, second.completion]);
  for (const [result, payload] of results) {
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(payload.status, "ready"); assert.equal(payload.baseHead, baseHead);
    assert.equal(payload.lifecycleTracking.status, "worktree-tracked");
  }
  assert.deepEqual(ctx.tracked().map((item) => item.branch), ["codex/unit-feature-one", "codex/unit-feature-two"]);
});

test("concurrent_create_overlapping_ownership_has_exactly_one_winner", async (t) => {
  const ctx = fixture(t); const first = ctx.spawnCreate("one", "src"); const second = ctx.spawnCreate("two", "src/shared.rs");
  const results = await Promise.all([first.completion, second.completion]);
  assert.equal(results.filter(([result]) => result.status === 0).length, 1);
  const rejected = results.find(([result]) => result.status !== 0);
  assert.equal(rejected[1].error.code, "ownership_overlap_across_units");
  assert.equal(ctx.tracked().length, 1);
  const loser = rejected[1].error.message.includes("已有单元 one") ? "two" : "one";
  assertNoUnitCreateSideEffects(ctx, loser);
});

test("concurrent_create_preserves_both_registrations_in_real_lifecycle_state", async (t) => {
  const ctx = fixture(t, { realLifecycle: true });
  const lifecycle = path.join(ctx.root, ".agents", "skills", "desktop-manage-git-lifecycle", "scripts", "git_lifecycle.mjs");
  const started = spawnSync(process.execPath, [lifecycle, "start", "--project-root", ctx.root, "--summary", "parallel-fixture"], { cwd: ctx.root, encoding: "utf8" });
  assert.equal(started.status, 0, started.stdout + started.stderr);
  const first = ctx.spawnCreate("one", "src"); const second = ctx.spawnCreate("two", "docs");
  const results = await Promise.all([first.completion, second.completion]);
  for (const [result, payload] of results) {
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.equal(payload.lifecycleTracking.status, "worktree-tracked");
  }
  const lifecycleState = JSON.parse(fs.readFileSync(path.join(ctx.commonDir(), "agent-first-harness", "git-lifecycle.json"), "utf8"));
  const expectedBranches = results.map(([, payload]) => payload.branch).sort();
  assert.deepEqual(lifecycleState.cycle.worktrees.map((entry) => entry.branch).sort(), expectedBranches);
  for (const [, payload] of results) {
    assert.equal(lifecycleState.cycle.branches.some((entry) => entry.name === payload.branch), true);
    assert.equal(lifecycleState.cycle.worktrees.some((entry) => entry.path === payload.worktreePath), true);
  }
});

test("creating_and_abandoned_reservations_cannot_guard_verify_remove_or_lose_ownership", async (t) => {
  const ctx = fixture(t); const common = ctx.commonDir(); fs.writeFileSync(path.join(common, "mock-track-block-pending"), "block");
  const pending = ctx.spawnCreate("pending", "src");
  await waitForFile(path.join(common, "mock-track-entered-pending"));
  const statePath = path.join(common, "codex-parallel-worktrees", "feature", "pending.json");
  const state = JSON.parse(fs.readFileSync(statePath, "utf8"));
  for (const command of ["guard", "verify", "remove"]) {
    const args = [command, "--task", "feature", "--unit", "pending"];
    if (command === "guard") args.push("--write-target", "src");
    const [result, payload] = ctx.helper(args, { cwd: command === "remove" ? ctx.source : state.worktreePath });
    assert.equal(result.status, 4); assert.equal(payload.error.code, "unit_not_ready");
  }
  pending.child.kill("SIGKILL"); await pending.completion;
  fs.writeFileSync(path.join(common, "mock-track-release-pending"), "release");
  await waitForFile(path.join(common, "mock-tracked-worktrees", "pending.json"));
  assert.equal(JSON.parse(fs.readFileSync(statePath, "utf8")).status, "creating");
  const [result, payload] = ctx.create("conflict", "src/new.rs");
  assert.equal(result.status, 4); assert.equal(payload.error.code, "ownership_overlap_across_units");
  assert.equal(fs.existsSync(state.worktreePath), true);
});

test("concurrent_create_failure_rolls_back_only_its_own_resources", async (t) => {
  const ctx = fixture(t); fs.writeFileSync(path.join(ctx.commonDir(), "mock-track-failure-failed"), "fail");
  const failed = ctx.spawnCreate("failed", "src"); const successful = ctx.spawnCreate("successful", "docs");
  const [[failure, error], [success, ready]] = await Promise.all([failed.completion, successful.completion]);
  assert.equal(failure.status, 4); assert.equal(error.error.code, "lifecycle_worktree_tracking_failed");
  assert.equal(success.status, 0, success.stdout + success.stderr); assert.equal(ready.status, "ready");
  assertNoUnitCreateSideEffects(ctx, "failed");
  assert.equal(fs.existsSync(ready.worktreePath), true);
  assert.deepEqual(ctx.tracked(), [{ branch: ready.branch, worktree: ready.worktreePath }]);
});

test("concurrent_create_waits_for_short_state_lock_without_manual_retry", async (t) => {
  const ctx = fixture(t); const lock = path.join(ctx.commonDir(), "codex-parallel-worktrees", "feature", ".lock");
  const baseHead = runGit(ctx.source, "rev-parse", "HEAD").stdout.trim();
  fs.mkdirSync(lock, { recursive: true });
  const marker = path.join(ctx.tempRoot, "waiting-for-lock"), preload = path.join(ctx.tempRoot, "observe_lock_attempt.mjs");
  fs.writeFileSync(preload, [
    'import fs from "node:fs";',
    'const mkdir = fs.mkdirSync; let observed = false;',
    'fs.mkdirSync = function(file, ...args) {',
    `  if (!observed && String(file) === ${JSON.stringify(lock)}) { observed = true; fs.writeFileSync(${JSON.stringify(marker)}, "captured-source-head"); }`,
    '  return mkdir.call(this, file, ...args);',
    '};',
  ].join("\n"));
  const pending = ctx.spawnCreateWithNodeArgs(["--import", preload], "waiting", "src");
  await waitForFile(marker);
  assert.equal(pending.child.exitCode, null);
  runGit(ctx.source, "commit", "--allow-empty", "-m", "advance source while creation waits");
  fs.rmdirSync(lock);
  const [result, payload] = await pending.completion;
  assert.equal(result.status, 0, result.stdout + result.stderr); assert.equal(payload.status, "ready");
  assert.equal(payload.baseHead, baseHead); assert.equal(runGit(payload.worktreePath, "rev-parse", "HEAD").stdout.trim(), baseHead);
});

test("create_tracks_exact_worktree_in_current_release_cycle", (t) => {
  const ctx = fixture(t); const [result, payload] = ctx.create("tracked", "src");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(payload.sourceBranch, "feature-current-20260909");
  assert.equal(payload.baseHead, runGit(ctx.source, "rev-parse", "HEAD").stdout.trim());
  assert.equal(payload.lifecycleTracking.status, "worktree-tracked");
  assert.deepEqual(ctx.tracked(), [{ branch: "codex/unit-feature-tracked", worktree: fs.realpathSync(payload.worktreePath) }]);
});

test("create_rolls_back_when_lifecycle_tracking_fails", (t) => {
  const ctx = fixture(t); fs.writeFileSync(path.join(ctx.commonDir(), "mock-track-failure"), "fail\n");
  const [result, payload] = ctx.create("rollback", "src");
  assert.equal(result.status, 4); assert.equal(payload.error.code, "lifecycle_worktree_tracking_failed");
  assertNoUnitCreateSideEffects(ctx, "rollback");
});

test("create_preserves_registered_resources_when_tracker_reports_failure_after_save", (t) => {
  const ctx = fixture(t); fs.writeFileSync(path.join(ctx.commonDir(), "mock-track-failure-after-save"), "fail");
  const [result, payload] = ctx.create("retained", "src");
  assert.equal(result.status, 4); assert.equal(payload.error.code, "lifecycle_worktree_tracking_failed");
  assert.match(payload.error.message, /registered/u);
  const state = JSON.parse(fs.readFileSync(path.join(ctx.commonDir(), "codex-parallel-worktrees", "feature", "retained.json"), "utf8"));
  assert.equal(state.status, "failed"); assert.equal(fs.existsSync(state.worktreePath), true);
  runGit(ctx.root, "show-ref", "--verify", `refs/heads/${state.branch}`);
  assert.deepEqual(ctx.tracked(), [{ branch: state.branch, worktree: state.worktreePath }]);
});

test("create_preserves_resources_when_lifecycle_registration_cannot_be_confirmed", (t) => {
  const ctx = fixture(t); const common = ctx.commonDir();
  fs.writeFileSync(path.join(common, "mock-track-failure"), "fail"); fs.writeFileSync(path.join(common, "mock-inspect-failure"), "fail");
  const [result, payload] = ctx.create("uncertain", "src");
  assert.equal(result.status, 4); assert.equal(payload.error.code, "lifecycle_worktree_tracking_failed"); assert.match(payload.error.message, /unknown/u);
  const state = JSON.parse(fs.readFileSync(path.join(common, "codex-parallel-worktrees", "feature", "uncertain.json"), "utf8"));
  assert.equal(state.status, "failed"); assert.equal(fs.existsSync(state.worktreePath), true);
  runGit(ctx.root, "show-ref", "--verify", `refs/heads/${state.branch}`);
  const [blocked, guard] = ctx.helper(["guard", "--task", "feature", "--unit", "uncertain", "--write-target", "src"], { cwd: state.worktreePath });
  assert.equal(blocked.status, 4); assert.equal(guard.error.code, "unit_not_ready");
});

test("create_preserves_registered_resources_when_ready_state_write_fails", (t) => {
  const ctx = fixture(t, { realLifecycle: true });
  const lifecycle = path.join(ctx.root, ".agents", "skills", "desktop-manage-git-lifecycle", "scripts", "git_lifecycle.mjs");
  const started = spawnSync(process.execPath, [lifecycle, "start", "--project-root", ctx.root, "--summary", "ready-failure"], { cwd: ctx.root, encoding: "utf8" });
  assert.equal(started.status, 0, started.stdout + started.stderr);
  const preload = path.join(ctx.tempRoot, "fail_ready_write.mjs");
  fs.writeFileSync(preload, [
    'import fs from "node:fs";',
    'const write = fs.writeFileSync; let failed = false;',
    'fs.writeFileSync = function(file, data, ...args) {',
    '  if (!failed && typeof data === "string" && data.includes(\'"status":"ready"\')) { failed = true; throw Object.assign(new Error("forced ready write failure"), { code: "EIO" }); }',
    '  return write.call(this, file, data, ...args);',
    '};',
  ].join("\n"));
  const [result, payload] = ctx.helper(["create", "--task", "feature", "--unit", "readyfail", "--write-target", "src"], { nodeArgs: ["--import", preload] });
  assert.equal(result.status, 4); assert.match(payload.error.message, /forced ready write failure/u); assert.match(payload.error.message, /registered/u);
  const state = JSON.parse(fs.readFileSync(path.join(ctx.commonDir(), "codex-parallel-worktrees", "feature", "readyfail.json"), "utf8"));
  assert.equal(state.status, "failed"); assert.equal(fs.existsSync(state.worktreePath), true); runGit(ctx.root, "show-ref", "--verify", `refs/heads/${state.branch}`);
  const registered = JSON.parse(fs.readFileSync(path.join(ctx.commonDir(), "agent-first-harness", "git-lifecycle.json"), "utf8")).cycle;
  assert.equal(registered.branches.some((entry) => entry.name === state.branch), true);
  assert.equal(registered.worktrees.some((entry) => entry.path === state.worktreePath && entry.branch === state.branch), true);
});

test("remove_retains_worktree_and_branch_for_separate_cleanup", (t) => {
  const ctx = fixture(t); const [created, payload] = ctx.create("docs", "docs.txt"); assert.equal(created.status, 0, created.stderr);
  fs.writeFileSync(path.join(payload.worktreePath, "docs.txt"), "done\n"); runGit(payload.worktreePath, "add", "docs.txt"); runGit(payload.worktreePath, "commit", "-m", "complete docs unit");
  const [removed, removal] = ctx.helper(["remove", "--task", "feature", "--unit", "docs"]);
  assert.equal(removed.status, 0, removed.stderr); assert.equal(removal.resourcesRetained, true); assert.equal(removal.stateRemoved, true);
  assert.equal(fs.existsSync(payload.worktreePath), true); runGit(ctx.root, "show-ref", "--verify", "refs/heads/codex/unit-feature-docs");
  assert.equal(fs.existsSync(path.join(ctx.commonDir(), "codex-parallel-worktrees", "feature", "docs.json")), false);
});

test("create_rejects_dirty_source_including_untracked_files", (t) => {
  const ctx = fixture(t); fs.writeFileSync(path.join(ctx.source, "local.txt"), "user work\n");
  const [result, payload] = ctx.create("core", "src"); assert.equal(result.status, 4); assert.equal(payload.error.code, "source_worktree_dirty");
});

test("create_accepts_local_primary_source_and_requires_exact_source_cwd", (t) => {
  const ctx = fixture(t);
  let [result, payload] = ctx.helper(["create", "--task", "feature", "--unit", "core", "--write-target", "src"], { projectRoot: ctx.source, sourceWorktree: ctx.source });
  assert.equal(result.status, 4); assert.equal(payload.error.code, "project_root_not_primary_worktree");
  runGit(ctx.root, "switch", "-c", "feature-local-20260910");
  [result, payload] = ctx.helper(["create", "--task", "local", "--unit", "core", "--write-target", "src"], { cwd: ctx.root, sourceWorktree: ctx.root });
  assert.equal(result.status, 0, result.stderr); assert.equal(payload.sourceWorktree, ctx.root); assert.equal(payload.sourceBranch, "feature-local-20260910");
  [result, payload] = ctx.helper(["create", "--task", "feature", "--unit", "core", "--write-target", "src"], { cwd: ctx.root });
  assert.equal(result.status, 4); assert.equal(payload.error.code, "source_cwd_mismatch");
});

test("create_rejects_source_from_another_repository", (t) => {
  const ctx = fixture(t); const other = path.join(ctx.tempRoot, "other_project"); fs.mkdirSync(other);
  runGit(other, "init", "-b", "feature-current-20260909"); runGit(other, "config", "user.name", "Harness Test"); runGit(other, "config", "user.email", "harness-test@example.invalid"); runGit(other, "commit", "--allow-empty", "-m", "other");
  const [result, payload] = ctx.helper(["create", "--task", "feature", "--unit", "core", "--write-target", "src"], { cwd: other, sourceWorktree: other });
  assert.equal(result.status, 4); assert.equal(payload.error.code, "source_repository_mismatch");
});

test("create_rejects_detached_source_without_requiring_branch_prefix", (t) => {
  const ctx = fixture(t); runGit(ctx.source, "checkout", "--detach");
  const [result, payload] = ctx.create("core", "src"); assert.equal(result.status, 4); assert.equal(payload.error.code, "source_branch_unavailable");
});

test("create_rejects_symlinked_external_worktree_container", (t) => {
  const ctx = fixture(t); const redirect = path.join(ctx.tempRoot, "redirected"); fs.mkdirSync(redirect);
  try { fs.symlinkSync(redirect, path.join(ctx.tempRoot, ".codex-worktrees"), "dir"); } catch (error) { return t.skip(error.message); }
  const [result, payload] = ctx.create("core", "src"); assert.equal(result.status, 4); assert.equal(payload.error.code, "worktree_container_unsafe"); assert.deepEqual(fs.readdirSync(redirect), []);
});

test("create_requires_safe_non_root_ownership", (t) => {
  const ctx = fixture(t); let [result, payload] = ctx.create("missing"); assert.equal(result.status, 4); assert.equal(payload.error.code, "ownership_required");
  for (const [unit, target] of [["root", "."], ["absolute", path.join(ctx.tempRoot, "outside")], ["traversal", "../outside"]]) {
    [result, payload] = ctx.create(unit, target); assert.equal(result.status, 4); assert.equal(payload.error.code, "ownership_path_invalid");
  }
});

test("create_rejects_overlapping_ownership_within_and_across_units", (t) => {
  const ctx = fixture(t); let [result, payload] = ctx.create("within", "src", "src/lib.rs"); assert.equal(result.status, 4); assert.equal(payload.error.code, "ownership_overlap_within_unit");
  [result] = ctx.create("one", "src"); assert.equal(result.status, 0, result.stderr);
  [result, payload] = ctx.create("two", "src/shared.rs"); assert.equal(result.status, 4); assert.equal(payload.error.code, "ownership_overlap_across_units");
});

test("guard_accepts_registered_subpath_and_rejects_unregistered_target", (t) => {
  const ctx = fixture(t); const [created, payload] = ctx.create("guarded", "src"); assert.equal(created.status, 0, created.stderr);
  let [result, guard] = ctx.helper(["guard", "--task", "feature", "--unit", "guarded", "--write-target", "src/new_file.rs"], { cwd: payload.worktreePath });
  assert.equal(result.status, 0, result.stderr); assert.deepEqual(guard.writeTargets, ["src/new_file.rs"]);
  [result, guard] = ctx.helper(["guard", "--task", "feature", "--unit", "guarded", "--write-target", "docs/new_file.md"], { cwd: payload.worktreePath });
  assert.equal(result.status, 4); assert.equal(guard.error.code, "write_target_not_owned");
});

test("guard_rejects_missing_target_wrong_cwd_and_detached_unit", (t) => {
  const ctx = fixture(t); const [created, payload] = ctx.create("guarded", "src"); assert.equal(created.status, 0, created.stderr);
  let [result, body] = ctx.helper(["guard", "--task", "feature", "--unit", "guarded"], { cwd: payload.worktreePath }); assert.equal(result.status, 4); assert.equal(body.error.code, "write_target_required");
  [result, body] = ctx.helper(["guard", "--task", "feature", "--unit", "guarded", "--write-target", "src"], { cwd: ctx.source }); assert.equal(result.status, 4); assert.equal(body.error.code, "unit_cwd_mismatch");
  runGit(payload.worktreePath, "checkout", "--detach");
  [result, body] = ctx.helper(["guard", "--task", "feature", "--unit", "guarded", "--write-target", "src"], { cwd: payload.worktreePath }); assert.equal(result.status, 4); assert.equal(body.error.code, "unit_branch_mismatch");
});

test("guard_rejects_git_root_mismatch_and_symlink_escape", (t) => {
  const ctx = fixture(t); let [created, payload] = ctx.create("rootcheck", "root-owned"); assert.equal(created.status, 0, created.stderr);
  fs.unlinkSync(path.join(payload.worktreePath, ".git")); runGit(path.dirname(payload.worktreePath), "init", "-b", "unexpected");
  let [result, body] = ctx.helper(["guard", "--task", "feature", "--unit", "rootcheck", "--write-target", "root-owned"], { cwd: payload.worktreePath }); assert.equal(result.status, 4); assert.equal(body.error.code, "unit_git_root_mismatch");
  [created, payload] = ctx.create("symlink", "symlink-owned"); assert.equal(created.status, 0, created.stderr);
  const outside = path.join(ctx.tempRoot, "outside"); fs.mkdirSync(outside); fs.mkdirSync(path.join(payload.worktreePath, "symlink-owned"));
  try { fs.symlinkSync(outside, path.join(payload.worktreePath, "symlink-owned", "escape"), "dir"); } catch (error) { return t.skip(error.message); }
  [result, body] = ctx.helper(["guard", "--task", "feature", "--unit", "symlink", "--write-target", "symlink-owned/escape/changed.txt"], { cwd: payload.worktreePath }); assert.equal(result.status, 4); assert.equal(body.error.code, "write_target_outside_worktree");
});

test("verify_accepts_committed_and_untracked_changes_inside_ownership", (t) => {
  const ctx = fixture(t); const [created, payload] = ctx.create("verified", "src"); assert.equal(created.status, 0, created.stderr);
  fs.mkdirSync(path.join(payload.worktreePath, "src")); fs.writeFileSync(path.join(payload.worktreePath, "src", "committed.rs"), "done\n"); runGit(payload.worktreePath, "add", "src/committed.rs"); runGit(payload.worktreePath, "commit", "-m", "add owned change"); fs.writeFileSync(path.join(payload.worktreePath, "src", "untracked.rs"), "pending\n");
  const [result, body] = ctx.helper(["verify", "--task", "feature", "--unit", "verified"], { cwd: payload.worktreePath }); assert.equal(result.status, 0, result.stderr); assert.deepEqual(body.changedPaths, ["src/committed.rs", "src/untracked.rs"]);
});

test("verify_rejects_committed_untracked_and_renamed_escape", (t) => {
  const ctx = fixture(t);
  let [created, payload] = ctx.create("committed", "src-committed"); assert.equal(created.status, 0, created.stderr); fs.writeFileSync(path.join(payload.worktreePath, "outside.txt"), "escape\n"); runGit(payload.worktreePath, "add", "outside.txt"); runGit(payload.worktreePath, "commit", "-m", "out of scope");
  let [result, body] = ctx.helper(["verify", "--task", "feature", "--unit", "committed"], { cwd: payload.worktreePath }); assert.equal(result.status, 4); assert.equal(body.error.code, "actual_change_outside_ownership");
  [created, payload] = ctx.create("untracked", "src-untracked"); assert.equal(created.status, 0, created.stderr); fs.writeFileSync(path.join(payload.worktreePath, "outside.txt"), "escape\n");
  [result, body] = ctx.helper(["verify", "--task", "feature", "--unit", "untracked"], { cwd: payload.worktreePath }); assert.equal(result.status, 4); assert.equal(body.error.code, "actual_change_outside_ownership");
  [created, payload] = ctx.create("rename", "README.md"); assert.equal(created.status, 0, created.stderr); runGit(payload.worktreePath, "mv", "README.md", "renamed.md"); runGit(payload.worktreePath, "commit", "-m", "rename outside ownership");
  [result, body] = ctx.helper(["verify", "--task", "feature", "--unit", "rename"], { cwd: payload.worktreePath }); assert.equal(result.status, 4); assert.equal(body.error.code, "actual_change_outside_ownership");
});

test("create_rejects_unsafe_identifier_and_existing_path", (t) => {
  const ctx = fixture(t); let [result, payload] = ctx.helper(["create", "--task", "../escape", "--unit", "core", "--write-target", "src"]); assert.equal(result.status, 2); assert.equal(payload.error.code, "invalid_identifier");
  fs.mkdirSync(path.join(ctx.tempRoot, ".codex-worktrees", path.basename(ctx.root), "feature", "core"), { recursive: true });
  [result, payload] = ctx.create("core", "src"); assert.equal(result.status, 4); assert.equal(payload.error.code, "worktree_path_exists");
});

test("remove_rejects_dirty_but_accepts_clean_unmerged_unit", (t) => {
  const ctx = fixture(t); const [created, payload] = ctx.create("tests", "src"); assert.equal(created.status, 0, created.stderr);
  fs.mkdirSync(path.join(payload.worktreePath, "src")); fs.writeFileSync(path.join(payload.worktreePath, "src", "pending.txt"), "pending\n");
  let [result, body] = ctx.helper(["remove", "--task", "feature", "--unit", "tests"]); assert.equal(result.status, 4); assert.equal(body.error.code, "worktree_dirty");
  runGit(payload.worktreePath, "add", "src/pending.txt"); runGit(payload.worktreePath, "commit", "-m", "unintegrated work");
  [result, body] = ctx.helper(["remove", "--task", "feature", "--unit", "tests"]); assert.equal(result.status, 0, result.stderr); assert.equal(body.stateRemoved, true); assert.equal(fs.existsSync(payload.worktreePath), true);
});

test("remove_runs_postflight_before_clean_state_cleanup", (t) => {
  const ctx = fixture(t); const [created, payload] = ctx.create("escape", "src"); assert.equal(created.status, 0, created.stderr);
  fs.writeFileSync(path.join(payload.worktreePath, "outside.txt"), "escape\n"); runGit(payload.worktreePath, "add", "outside.txt"); runGit(payload.worktreePath, "commit", "-m", "out of scope");
  const [result, body] = ctx.helper(["remove", "--task", "feature", "--unit", "escape"]); assert.equal(result.status, 4); assert.equal(body.error.code, "actual_change_outside_ownership"); assert.equal(fs.existsSync(payload.worktreePath), true);
});
