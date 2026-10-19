/** Git 生命周期分支、状态、Worktree 与 publish 黑盒回归。 */

import test from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { SCRIPT, collectProcess, lifecycleFixture, run, spawnHelper } from "./git_lifecycle_test_support.mjs";
import { newState, resolveRepository, statePath, validBranch } from "./git_lifecycle_core.mjs";

/** 在隔离 fixture 中执行测试并保证清理。 */
function scenario(name, callback) {
  test(name, async () => {
    const item = lifecycleFixture();
    try {
      await callback(item);
    } finally {
      item.cleanup();
    }
  });
}

test("help_and_invalid_arguments_are_single_line_json", () => {
  for (const [args, status] of [[["--help"], 0], [[], 1]]) {
    const result = run(process.execPath, [SCRIPT, ...args], { check: false });
    assert.equal(result.status, status);
    assert.equal(result.stderr, "");
    assert.equal(result.stdout.trimEnd().split("\n").length, 1);
    assert.equal(typeof JSON.parse(result.stdout), "object");
  }
});

scenario("start_without_remote_is_idempotent_and_uses_common_dir_state", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const started = item.helper(repository, ["start", "--summary", "offline-fix"]).payload;
  assert.match(started.branch, /^feature-offline-fix-\d{8}$/);
  assert.equal(started.remote, null);
  assert.equal(item.git(repository, "branch", "--show-current").stdout.trim(), started.branch);
  const repeated = item.helper(repository, ["start", "--summary", "offline-fix"]).payload;
  assert.equal(repeated.status, "already-started");
  assert.equal(repeated.branch, started.branch);
  const resolved = resolveRepository(repository);
  assert.equal(readFileSync(statePath(resolved), "utf8").includes("offline-fix"), true);
});

scenario("branch_validation_rejects_ambiguous_pseudo_refs", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const resolved = resolveRepository(repository);
  assert.equal(validBranch(resolved, "@"), false);
  assert.equal(validBranch(resolved, "HEAD"), false);
  assert.equal(validBranch(resolved, "feature/@-safe"), true);
});

scenario("quiescent_v2_state_is_migrated_and_v1_is_rejected", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const path = statePath(resolveRepository(repository));
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify({
    schemaVersion: 2, remote: null, defaultBranch: "main", cycle: null, lastRelease: null,
  }), "utf8");
  const inspected = item.helper(repository, ["inspect"]).payload;
  assert.equal(inspected.state.schemaVersion, 4);
  assert.equal(inspected.state.pendingPublish, null);
  assert.deepEqual(inspected.state.releasedResources, []);
  writeFileSync(path, JSON.stringify({
    schemaVersion: 1, remote: null, defaultBranch: "main", cycle: null,
    pendingPublish: null, lastRelease: null,
  }), "utf8");
  const rejected = item.helper(repository, ["inspect"], { success: false }).payload;
  assert.equal(rejected.code, "state-invalid");
});

scenario("v2_active_cycle_is_migrated_without_losing_registered_branch", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const branch = item.helper(repository, ["start", "--summary", "migrate-cycle"]).payload.branch;
  const path = statePath(resolveRepository(repository));
  const state = item.state(repository);
  state.schemaVersion = 2;
  delete state.releasedResources;
  state.cycle.branches[0].localDeleted = false;
  state.cycle.branches[0].remoteDeleted = false;
  writeFileSync(path, JSON.stringify(state), "utf8");
  const inspected = item.helper(repository, ["inspect"]).payload.state;
  assert.equal(inspected.schemaVersion, 4);
  assert.deepEqual(inspected.cycle.branches.map((entry) => entry.name), [branch]);
  assert.deepEqual(Object.keys(inspected.cycle.branches[0]).sort(), ["createdAt", "name", "summary"]);
});

scenario("v2_inflight_release_or_publish_is_rejected_without_state_rewrite", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const path = statePath(resolveRepository(repository));
  mkdirSync(dirname(path), { recursive: true });
  const base = {
    schemaVersion: 2, remote: null, defaultBranch: "main", cycle: null,
    pendingPublish: null, lastRelease: null,
  };
  const pending = {
    tag: "v1.0.0-20260901", head: null, date: "20260901", version: "1.0.0",
    gitPublication: "local", remote: null, releaseContextSha256: "0".repeat(64),
  };
  for (const state of [
    { ...base, cycle: { branches: [], worktrees: [], pendingRelease: pending } },
    { ...base, pendingPublish: { head: "0".repeat(40), targets: [] } },
  ]) {
    const bytes = JSON.stringify(state);
    writeFileSync(path, bytes, "utf8");
    const rejected = item.helper(repository, ["inspect"], { success: false }).payload;
    assert.equal(rejected.code, "legacy-inflight-unsupported");
    assert.equal(readFileSync(path, "utf8"), bytes);
  }
});

scenario("completed_v2_release_migrates_only_with_matching_local_tag_and_main", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  mkdirSync(join(repository, "docs"), { recursive: true });
  writeFileSync(join(repository, "docs/AGENT_POLICY.md"), [
    "---", "schema_version: 4", "confirmed_by: user", "confirmed_at: 2026-09-28",
    "decision_mode: reuse_then_infer_then_ask", "superpowers: disabled", "user_owned_tasks: disabled",
    "parallel_worktree_subagents: disabled", "acceptance_smoke: disabled", "e2e_hint: disabled",
    "post_release_action: push_release_branch", "---", "", "# Agent policy", "",
    "- `post_release_action`：`local_package` or `push_release_branch`", "",
    "发布后动作直接读取 `post_release_action`。下游冻结已确认的 `post_release_action`。", "",
    "`push-release --remote <name>`", "",
    "远端默认主分支、`release` 分支和 tag 同步到同一已发布 HEAD。", "",
  ].join("\n"), "utf8");
  item.git(repository, "add", "docs/AGENT_POLICY.md");
  item.git(repository, "commit", "--quiet", "-m", "chore: record post-release choice");
  const context = item.prepareReleaseContext(repository, { version: "1.2.3", date: "20260920" });
  item.helper(repository, [
    "release", "--version", "1.2.3", "--date", "20260920",
    "--release-context-sha256", context.digest,
  ]);
  const path = statePath(resolveRepository(repository));
  const state = item.state(repository);
  state.schemaVersion = 2;
  delete state.releasedResources;
  const { defaultBranch: _branch, postReleaseAction: _action, ...last } = state.lastRelease;
  state.lastRelease = { ...last, gitPublication: "local", remote: null };
  writeFileSync(path, JSON.stringify(state), "utf8");
  assert.equal(item.helper(repository, ["inspect"]).payload.state.lastRelease.head, context.head);
  state.lastRelease.releaseContextSha256 = "f".repeat(64);
  const mismatchedBytes = JSON.stringify(state);
  writeFileSync(path, mismatchedBytes, "utf8");
  assert.equal(item.helper(repository, ["inspect"], { success: false }).payload.code, "legacy-release-unverifiable");
  assert.equal(readFileSync(path, "utf8"), mismatchedBytes);
  state.lastRelease.releaseContextSha256 = context.digest;
  writeFileSync(path, JSON.stringify(state), "utf8");
  item.git(repository, "tag", "-d", "v1.2.3-20260920");
  assert.equal(item.helper(repository, ["inspect"], { success: false }).payload.code, "legacy-release-unverifiable");
});

scenario("quiescent_v3_release_actions_migrate_as_unbound_without_rewriting_state", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const path = statePath(resolveRepository(repository));
  mkdirSync(dirname(path), { recursive: true });
  const head = item.git(repository, "rev-parse", "HEAD").stdout.trim();
  const release = {
    tag: "v1.2.3-20260920", head, date: "20260920", version: "1.2.3",
    releaseContextSha256: "a".repeat(64), defaultBranch: "main",
  };
  const base = {
    schemaVersion: 3, remote: null, defaultBranch: "main", cycle: null,
    pendingPublish: null, lastRelease: release, releasedResources: [],
  };
  const bytes = JSON.stringify(base);
  writeFileSync(path, bytes, "utf8");
  const inspected = item.helper(repository, ["inspect"]).payload.state;
  assert.equal(inspected.schemaVersion, 4);
  assert.equal(inspected.lastRelease.postReleaseAction, null);
  assert.equal(inspected.lastRelease.head, head);
  assert.equal(readFileSync(path, "utf8"), bytes);

  const { defaultBranch: _defaultBranch, ...pending } = release;
  const inFlight = {
    ...base, lastRelease: null,
    cycle: { branches: [], worktrees: [], pendingRelease: { ...pending, head: null } },
  };
  const inFlightBytes = JSON.stringify(inFlight);
  writeFileSync(path, inFlightBytes, "utf8");
  assert.equal(item.helper(repository, ["inspect"], { success: false }).payload.code, "legacy-inflight-unsupported");
  assert.equal(readFileSync(path, "utf8"), inFlightBytes);
  const publishing = {
    ...base, pendingPublish: {
      head, targets: [{ remote: "origin", branch: "main", confirmed: false }],
    },
  };
  const publishingBytes = JSON.stringify(publishing);
  writeFileSync(path, publishingBytes, "utf8");
  assert.equal(item.helper(repository, ["inspect"], { success: false }).payload.code, "legacy-inflight-unsupported");
  assert.equal(readFileSync(path, "utf8"), publishingBytes);
  writeFileSync(path, JSON.stringify({ ...base, lastRelease: { ...release, postReleaseAction: "push_release_branch" } }), "utf8");
  assert.equal(item.helper(repository, ["inspect"], { success: false }).payload.code, "state-invalid");
});

scenario("post_release_check_uses_frozen_action_and_verifies_local_release_refs", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const noRelease = item.helper(repository, ["check-post-release", "--action", "local_package"], { success: false }).payload;
  assert.equal(noRelease.code, "no-release");
  const head = item.git(repository, "rev-parse", "HEAD").stdout.trim();
  const tag = "v1.2.3-20260920";
  item.git(repository, "tag", tag, head);
  const state = newState();
  state.defaultBranch = "main";
  state.lastRelease = {
    tag, head, date: "20260920", version: "1.2.3", defaultBranch: "main",
    releaseContextSha256: "a".repeat(64), postReleaseAction: "local_package",
  };
  const path = statePath(resolveRepository(repository));
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(state), "utf8");
  const confirmed = item.helper(repository, ["check-post-release", "--action", "local_package"]).payload;
  assert.deepEqual({ status: confirmed.status, action: confirmed.action, head: confirmed.head, tag: confirmed.tag }, {
    status: "post-release-action-confirmed", action: "local_package", head, tag,
  });
  assert.equal(item.helper(repository, ["check-post-release", "--action", "push_release_branch"], { success: false }).payload.code,
    "post-release-action-mismatch");
  assert.equal(item.helper(repository, ["check-post-release", "--action", "other"], { success: false }).payload.code,
    "invalid-argument");

  state.lastRelease.postReleaseAction = null;
  writeFileSync(path, JSON.stringify(state), "utf8");
  assert.equal(item.helper(repository, ["check-post-release", "--action", "local_package"], { success: false }).payload.code,
    "post-release-action-unbound");
  state.lastRelease.postReleaseAction = "local_package";
  writeFileSync(path, JSON.stringify(state), "utf8");
  item.git(repository, "tag", "-d", tag);
  assert.equal(item.helper(repository, ["check-post-release", "--action", "local_package"], { success: false }).payload.code,
    "tag-conflict");
  item.git(repository, "tag", tag, head);
  item.commitFile(repository, "new-main.txt", "later source\n");
  assert.equal(item.helper(repository, ["check-post-release", "--action", "local_package"], { success: false }).payload.code,
    "local-state-changed");
});

scenario("registered_remote_precedes_origin_and_conflicting_override_fails", (item) => {
  const { repository, bare } = item.initializeRepository({ remote: true });
  item.git(repository, "remote", "add", "github", bare);
  item.git(repository, "fetch", "--quiet", "github");
  item.git(repository, "remote", "set-head", "github", "--auto");
  const started = item.helper(repository, ["start", "--summary", "stored-remote", "--remote", "github"]).payload;
  assert.equal(started.remote, "github");
  assert.equal(item.helper(repository, ["inspect"]).payload.remote, "github");
  assert.equal(item.helper(repository, ["inspect", "--remote", "origin"], { success: false }).payload.code, "remote-conflict");
});

scenario("start_adds_numeric_suffix_for_local_name_collisions", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const date = new Intl.DateTimeFormat("en-CA", {
    timeZone: "Asia/Shanghai", year: "numeric", month: "2-digit", day: "2-digit",
  }).format(new Date()).replaceAll("-", "");
  const base = `feature-collision-${date}`;
  item.git(repository, "branch", base);
  item.git(repository, "branch", `${base}-2`);
  assert.equal(item.helper(repository, ["start", "--summary", "collision"]).payload.branch, `${base}-3`);
});

scenario("concurrent_task_starts_preserve_both_branches_and_worktrees", async (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const worktrees = [join(item.temporary, "task-one"), join(item.temporary, "task-two")];
  for (const worktree of worktrees) item.git(repository, "worktree", "add", "--quiet", "--detach", worktree, "HEAD");
  const children = worktrees.map((worktree, index) => spawnHelper(
    worktree,
    ["start", "--summary", index === 0 ? "parallel-one" : "parallel-two"],
    item.env,
  ));
  const results = await Promise.all(children.map(collectProcess));
  for (const result of results) {
    assert.equal(result.status, 0, result.stdout);
    assert.equal(result.stderr, "");
  }
  const branches = new Set(results.map((result) => JSON.parse(result.stdout).branch));
  const cycle = item.state(repository).cycle;
  assert.deepEqual(new Set(cycle.branches.map((entry) => entry.name)), branches);
  assert.deepEqual(
    new Set(cycle.worktrees.map((entry) => entry.path)),
    new Set(worktrees.map((worktree) => realpathSync(worktree))),
  );
});

scenario("track_worktree_registers_v4_branch_without_legacy_cleanup_flags", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  item.helper(repository, ["start", "--summary", "primary-task"]);
  const worktree = join(item.temporary, "parallel-task");
  item.git(repository, "worktree", "add", "--quiet", "-b", "feature-parallel-task", worktree, "main");
  const tracked = item.helper(repository, ["track-worktree", "--worktree", worktree]).payload;
  assert.equal(tracked.status, "worktree-tracked");
  assert.equal(tracked.branch, "feature-parallel-task");
  const state = item.state(repository);
  const branch = state.cycle.branches.find((entry) => entry.name === "feature-parallel-task");
  assert.deepEqual(Object.keys(branch).sort(), ["createdAt", "name", "summary"]);
  assert.deepEqual(state.cycle.worktrees.at(-1), { path: realpathSync(worktree), branch: "feature-parallel-task" });
  assert.equal(item.helper(repository, ["track-worktree", "--worktree", worktree]).payload.status, "worktree-already-tracked");
});

scenario("track_worktree_ignores_unrelated_missing_inventory_path_and_keeps_target_strict", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  item.helper(repository, ["start", "--summary", "primary-task"]);
  const missing = join(item.temporary, "disappeared-unit"); const target = join(item.temporary, "healthy-unit");
  item.git(repository, "worktree", "add", "--quiet", "-b", "feature-disappeared", missing, "main");
  item.git(repository, "worktree", "add", "--quiet", "-b", "feature-healthy", target, "main");
  // 保留 inventory 快照记录，模拟另一单元回滚后目录已消失。
  rmSync(missing, { recursive: true, force: true });
  const tracked = item.helper(repository, ["track-worktree", "--worktree", target]).payload;
  assert.equal(tracked.status, "worktree-tracked"); assert.equal(tracked.branch, "feature-healthy");
  assert.deepEqual(item.state(repository).cycle.worktrees, [{ path: realpathSync(target), branch: "feature-healthy" }]);
  const stateBefore = item.state(repository);
  const rejected = item.helper(repository, ["track-worktree", "--worktree", missing], { success: false }).payload;
  assert.equal(rejected.code, "not-a-worktree"); assert.deepEqual(item.state(repository), stateBefore);
});

scenario("publish_merges_switches_and_pushes_without_tag_or_cleanup", (item) => {
  const { repository } = item.initializeRepository({ remote: true });
  const branch = item.helper(repository, ["start", "--summary", "publish-only"]).payload.branch;
  const expectedHead = item.commitFile(repository, "published.txt", "published\n");
  item.git(repository, "push", "--quiet", "origin", `refs/heads/${branch}:refs/heads/${branch}`);
  const published = item.helper(repository, ["publish"]).payload;
  assert.equal(published.status, "published");
  assert.equal(published.branch, "main");
  assert.equal(published.head, expectedHead);
  assert.equal(published.tagged, false);
  assert.equal(published.cleaned, false);
  assert.equal(item.git(repository, "branch", "--show-current").stdout.trim(), "main");
  assert.equal(item.localBranchExists(repository, branch), true);
  assert.equal(item.remoteBranchExists(repository, "origin", branch), true);
  assert.equal(item.state(repository).cycle !== null, true);
});

scenario("publish_merges_current_remote_default_before_development_branches", (item) => {
  const { repository, bare } = item.initializeRepository({ remote: true });
  item.helper(repository, ["start", "--summary", "remote-advance"]);
  const featureHead = item.commitFile(repository, "feature.txt", "feature\n");
  const collaborator = join(item.temporary, "collaborator");
  item.git(item.temporary, "clone", "--quiet", bare, collaborator);
  item.git(collaborator, "config", "user.name", "Remote Collaborator");
  item.git(collaborator, "config", "user.email", "remote@example.invalid");
  const remoteHead = item.commitFile(collaborator, "remote.txt", "remote\n");
  item.git(collaborator, "push", "--quiet", "origin", "main");
  const published = item.helper(repository, ["publish"]).payload;
  assert.equal(item.gitUnchecked(repository, "merge-base", "--is-ancestor", remoteHead, published.head).status, 0);
  assert.equal(item.gitUnchecked(repository, "merge-base", "--is-ancestor", featureHead, published.head).status, 0);
});

scenario("publish_pushes_same_head_to_additional_remote_without_rebinding", (item) => {
  const { repository } = item.initializeRepository({ remote: true });
  item.git(repository, "remote", "rename", "origin", "github");
  item.addBareRemote(repository, "origin", "stable");
  item.helper(repository, ["start", "--summary", "multi-remote", "--remote", "github"]);
  const expectedHead = item.commitFile(repository, "multi-remote.txt", "shared head\n");
  const published = item.helper(repository, ["publish", "--also-remote", "origin"]).payload;
  assert.equal(published.remote, "github");
  assert.equal(published.head, expectedHead);
  assert.deepEqual(published.publishedRemotes, [
    { remote: "github", branch: "main" },
    { remote: "origin", branch: "stable" },
  ]);
  assert.equal(item.state(repository).remote, "github");
});

scenario("publish_rejects_invalid_additional_remote_sets_before_pushing", (item) => {
  const { repository } = item.initializeRepository({ remote: true });
  item.helper(repository, ["start", "--summary", "invalid-targets"]);
  item.commitFile(repository, "candidate.txt", "candidate\n");
  for (const args of [
    ["publish", "--also-remote", "origin"],
    ["publish", "--also-remote", "missing"],
    ["publish", "--also-remote", "../unsafe"],
  ]) {
    assert.equal(item.helper(repository, args, { success: false }).payload.code.match(/invalid-argument|remote-not-found/) !== null, true);
  }
});

scenario("publish_refuses_missing_registered_branch", (item) => {
  const { repository } = item.initializeRepository({ remote: true });
  const branch = item.helper(repository, ["start", "--summary", "missing-branch"]).payload.branch;
  item.git(repository, "switch", "--quiet", "main");
  item.git(repository, "branch", "-D", branch);
  const rejected = item.helper(repository, ["publish"], { success: false }).payload;
  assert.equal(rejected.code, "registered-branch-missing");
});

/** 远端与本地均前进时两次普通合并均使用中文消息，父提交不变。 */
scenario("publish_uses_chinese_remote_and_feature_merge_messages", (item) => {
  const { repository, bare } = item.initializeRepository({ remote: true });
  const branch = item.helper(repository, ["start", "--summary", "chinese-merge"]).payload.branch;
  const featureHead = item.commitFile(repository, "feature.txt", "feature\n");
  item.git(repository, "switch", "main");
  const localHead = item.commitFile(repository, "local.txt", "local\n");
  item.git(repository, "switch", branch);
  const collaborator = join(item.temporary, "collaborator");
  item.git(item.temporary, "clone", "--quiet", bare, collaborator);
  item.git(collaborator, "config", "user.name", "Remote Collaborator");
  item.git(collaborator, "config", "user.email", "remote@example.invalid");
  const remoteHead = item.commitFile(collaborator, "remote.txt", "remote\n");
  item.git(collaborator, "push", "--quiet", "origin", "main");
  const published = item.helper(repository, ["publish"]).payload;
  const remoteMerge = item.git(repository, "rev-parse", `${published.head}^1`).stdout.trim();
  assert.equal(item.git(repository, "log", "-1", "--format=%s", published.head).stdout.trim(), `合并开发分支 ${branch}`);
  assert.equal(item.git(repository, "log", "-1", "--format=%s", remoteMerge).stdout.trim(), "合并远端默认分支 origin/main");
  assert.deepEqual(item.git(repository, "show", "-s", "--format=%P", published.head).stdout.trim().split(" "), [remoteMerge, featureHead]);
  assert.deepEqual(item.git(repository, "show", "-s", "--format=%P", remoteMerge).stdout.trim().split(" "), [localHead, remoteHead]);
});
