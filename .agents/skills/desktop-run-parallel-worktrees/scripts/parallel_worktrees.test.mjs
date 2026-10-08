#!/usr/bin/env node
/** 隔离验证并行 Worktree 助手的项目绑定、所有权与保守清理。 */

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
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
const worktree = fs.realpathSync(values.worktree);
const raw = spawnSync("git", ["-C", project, "rev-parse", "--git-common-dir"], { encoding: "utf8" }).stdout.trim();
const common = fs.realpathSync(path.isAbsolute(raw) ? raw : path.join(project, raw));
if (fs.existsSync(path.join(common, "mock-track-failure"))) {
  process.stdout.write(JSON.stringify({ status: "error", code: "forced-test-failure", message: "forced lifecycle tracking failure" }));
  process.exit(7);
}
const branch = spawnSync("git", ["-C", worktree, "branch", "--show-current"], { encoding: "utf8" }).stdout.trim();
const recordsPath = path.join(common, "mock-tracked-worktrees.json");
const records = fs.existsSync(recordsPath) ? JSON.parse(fs.readFileSync(recordsPath, "utf8")) : [];
records.push({ branch, worktree });
fs.writeFileSync(recordsPath, JSON.stringify(records));
process.stdout.write(JSON.stringify({ status: "worktree-tracked", branch, worktree, remote: null, command }));
`;

function runGit(root, ...args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result;
}

function fixture(t) {
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
  fs.writeFileSync(lifecycleScript, MOCK_LIFECYCLE);
  fs.writeFileSync(path.join(root, "README.md"), "baseline\n");
  runGit(root, "add", "README.md", path.relative(root, lifecycleScript));
  runGit(root, "commit", "-m", "baseline");
  runGit(root, "worktree", "add", "-b", "feature-current-20260909", source, "HEAD");
  const ctx = { tempRoot, root, source };
  ctx.git = (cwd, ...args) => runGit(cwd ?? root, ...args);
  ctx.commonDir = () => {
    const raw = runGit(root, "rev-parse", "--git-common-dir").stdout.trim();
    return fs.realpathSync(path.isAbsolute(raw) ? raw : path.join(root, raw));
  };
  ctx.helper = (args, { cwd, projectRoot, sourceWorktree } = {}) => {
    const command = args[0];
    const invocation = [SCRIPT, ...args, "--project-root", projectRoot ?? root];
    if (command !== "inspect") invocation.push("--source-worktree", sourceWorktree ?? source);
    const result = spawnSync(process.execPath, invocation, { encoding: "utf8", cwd: cwd ?? (command === "inspect" ? root : (sourceWorktree ?? source)) });
    return [result, JSON.parse(result.stdout)];
  };
  ctx.create = (unit, ...ownership) => {
    const args = ["create", "--task", "feature", "--unit", unit];
    for (const target of ownership) args.push("--write-target", target);
    return ctx.helper(args);
  };
  ctx.tracked = () => {
    const record = path.join(ctx.commonDir(), "mock-tracked-worktrees.json");
    return fs.existsSync(record) ? JSON.parse(fs.readFileSync(record, "utf8")) : [];
  };
  return ctx;
}

function assertNoUnitCreateSideEffects(ctx, unit) {
  assert.equal(fs.existsSync(path.join(ctx.tempRoot, ".codex-worktrees")), false);
  assert.equal(fs.existsSync(path.join(ctx.commonDir(), "codex-parallel-worktrees")), false);
  const branch = spawnSync("git", ["-C", ctx.root, "show-ref", "--verify", "--quiet", `refs/heads/codex/unit-feature-${unit}`]);
  assert.notEqual(branch.status, 0);
}

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
