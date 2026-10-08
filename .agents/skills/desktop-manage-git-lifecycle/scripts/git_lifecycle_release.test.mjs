/** Git 发布只合并本地主分支并复读本地 tag；推送是独立操作。 */

import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { stampHarnessVersion } from "../../desktop-prepare-release/scripts/harness_version_clock.mjs";
import { canonicalBytes } from "../../desktop-prepare-release/scripts/release_context.mjs";
import { initialize as initializeProductVersion } from "../../desktop-manage-version/scripts/version_gate.mjs";
import { lifecycleFixture } from "./git_lifecycle_test_support.mjs";

/** 在隔离 Git 仓库中运行场景。 */
function scenario(name, callback) {
  test(name, () => {
    const item = lifecycleFixture();
    try {
      callback(item);
    } finally {
      item.cleanup();
    }
  });
}

/** 测试下游持久策略；null 表示旧项目尚未写入。 */
function configurePostReleasePolicy(item, repository, policy) {
  if (policy === null) return;
  if (policy.action === "local_package") {
    const gatePath = join(repository, ".agents/skills/desktop-manage-version/scripts/version_gate.mjs");
    mkdirSync(dirname(gatePath), { recursive: true });
    copyFileSync(new URL("../../desktop-manage-version/scripts/version_gate.mjs", import.meta.url), gatePath);
    writeFileSync(join(repository, "Cargo.toml"),
      '[workspace]\nmembers = []\n[workspace.package]\nversion = "1.2.3"\n[workspace.metadata.agent-first-harness]\ninterfaces = ["cli"]\ntarget-platforms = ["macos"]\n', "utf8");
    initializeProductVersion(repository, { migrationApproved: true });
    item.git(repository, "add", "Cargo.toml", ".harness/version-state.json", ".agents/skills/desktop-manage-version/scripts/version_gate.mjs");
    item.git(repository, "commit", "--quiet", "-m", "chore: record local package platform");
  }
  const fields = [
    "---", `schema_version: ${policy.schema}`,
    "confirmed_by: user", "confirmed_at: 2026-09-28",
    "decision_mode: reuse_then_infer_then_ask", "superpowers: disabled",
    "user_owned_tasks: disabled", "parallel_worktree_subagents: disabled",
    "acceptance_smoke: disabled", "e2e_hint: disabled",
    ...(policy.schema === 4 ? [`post_release_action: ${policy.action}`] : []),
    "---", "", "# Agent policy", "",
    "- `post_release_action`：`local_package` or `push_release_branch`", "",
    "发布后动作直接读取 `post_release_action`。下游冻结已确认的 `post_release_action`。", "",
    "`push-release --remote <name>`", "",
    "远端默认主分支、`release` 分支和 tag 同步到同一已发布 HEAD。", "",
  ];
  mkdirSync(join(repository, "docs"), { recursive: true });
  writeFileSync(join(repository, "docs/AGENT_POLICY.md"), fields.join("\n"), "utf8");
  item.git(repository, "add", "docs/AGENT_POLICY.md");
  item.git(repository, "commit", "--quiet", "-m", "chore: record post-release choice");
}

/** 创建本地候选并把 schema v3 上下文提交到登记分支。 */
function candidate(item, { remote = true, worktree = null, version = "1.2.3", date = "20260909", reviewSelection = "disabled",
  postReleasePolicy = { schema: 4, action: "push_release_branch" } } = {}) {
  const initialized = item.initializeRepository({ remote });
  const repository = initialized.repository;
  configurePostReleasePolicy(item, repository, postReleasePolicy);
  const source = worktree ?? repository;
  const branch = item.helper(source, ["start", "--summary", "release-flow"]).payload.branch;
  item.commitFile(source, "candidate.txt", "candidate\n");
  const context = item.prepareReleaseContext(source, { version, date, reviewSelection });
  return { ...initialized, source, branch, context, version, date };
}

function releaseArgs(value) {
  return [
    "release", "--version", value.version, "--date", value.date,
    "--release-context-sha256", value.context.digest,
  ];
}

scenario("release_rejects_out_of_range_or_prerelease_version_before_any_merge", (item) => {
  const value = candidate(item);
  const before = item.git(value.repository, "rev-parse", "refs/heads/main").stdout;
  for (const version of ["1.100.0", "1.2.100", "1.2.3-rc.1", "01.2.3"]) {
    const rejected = item.helper(value.source, ["release", "--version", version, "--date", value.date,
      "--release-context-sha256", value.context.digest], { success: false }).payload;
    assert.equal(rejected.code, "invalid-version", version);
  }
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/main").stdout, before);
  assert.equal(item.state(value.repository).cycle.pendingRelease, null);
});

scenario("release_rejects_stale_release_notes_before_tag", (item) => {
  const value = candidate(item);
  const notes = JSON.parse(item.git(value.source, "show", "HEAD:release-notes.json").stdout);
  notes.releases[0].version = "v1.2.2";
  item.commitFile(value.source, "release-notes.json", `${JSON.stringify(notes, null, 2)}\n`);
  const rejected = item.helper(value.source, releaseArgs(value), { success: false }).payload;
  assert.equal(rejected.code, "release-notes-mismatch");
  assert.equal(item.git(value.repository, "tag", "--list", "v1.2.3-20260909").stdout, "");
  assert.equal(item.state(value.repository).cycle.pendingRelease.head, null);
});

scenario("release_rejects_noncanonical_release_notes_before_tag", (item) => {
  const value = candidate(item);
  const notes = JSON.parse(item.git(value.source, "show", "HEAD:release-notes.json").stdout);
  notes.releases[0].version = "vv1.2.3";
  item.commitFile(value.source, "release-notes.json", `${JSON.stringify(notes, null, 2)}\n`);
  const rejected = item.helper(value.source, releaseArgs(value), { success: false }).payload;
  assert.equal(rejected.code, "release-notes-mismatch");
  assert.equal(item.state(value.repository).cycle.pendingRelease.head, null);
});

scenario("release_rejects_duplicate_release_notes_keys_before_tag", (item) => {
  const value = candidate(item);
  const notes = item.git(value.source, "show", "HEAD:release-notes.json").stdout;
  const duplicate = notes.replace('"schemaVersion": 2,', '"schemaVersion": 2,\n  "schemaVersion": 2,');
  assert.notEqual(duplicate, notes);
  item.commitFile(value.source, "release-notes.json", duplicate);
  const rejected = item.helper(value.source, releaseArgs(value), { success: false }).payload;
  assert.equal(rejected.code, "release-notes-mismatch");
  assert.equal(item.state(value.repository).cycle.pendingRelease.head, null);
});

scenario("release_merges_locally_tags_and_preserves_registered_resources_without_remote", (item) => {
  const value = candidate(item);
  const originalRemoteHead = item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/main").stdout;
  const released = item.helper(value.source, releaseArgs(value)).payload;
  assert.equal(released.status, "released");
  assert.equal(released.branch, "main");
  assert.equal(released.head, value.context.head);
  assert.equal(released.tag, "v1.2.3-20260909");
  assert.deepEqual(released.preservedBranches, [value.branch]);
  assert.equal(item.localBranchExists(value.repository, value.branch), true);
  assert.equal(item.git(value.repository, "rev-parse", "refs/tags/v1.2.3-20260909^{commit}").stdout.trim(), value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/main").stdout, originalRemoteHead);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
  const state = item.state(value.repository);
  assert.equal(state.schemaVersion, 4);
  assert.equal(state.cycle, null);
  assert.equal(state.lastRelease.head, value.context.head);
  assert.equal(state.lastRelease.postReleaseAction, "push_release_branch");
  assert.equal(state.lastRelease.defaultBranch, "main");
  assert.deepEqual(state.releasedResources[0].branches.map((entry) => entry.name), [value.branch]);
  const repeated = item.helper(value.repository, releaseArgs(value)).payload;
  assert.equal(repeated.status, "already-released");
});

scenario("release_succeeds_while_remote_is_offline", (item) => {
  const value = candidate(item);
  renameSync(value.bare, value.bare + ".offline");
  const released = item.helper(value.repository, releaseArgs(value)).payload;
  assert.equal(released.status, "released");
  assert.equal(item.localBranchExists(value.repository, value.branch), true);
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "remote-read-failed");
});

scenario("publish_after_release_requires_new_cycle_and_cannot_replace_push_release", (item) => {
  const value = candidate(item);
  const released = item.helper(value.repository, releaseArgs(value)).payload;
  const localHead = item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim();
  const remoteHead = item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/main").stdout;
  const rejected = item.helper(value.repository, ["publish", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "release-already-complete");
  assert.match(rejected.message, /push-release/);
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim(), localHead);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/main").stdout, remoteHead);
  assert.equal(item.state(value.repository).lastRelease.head, released.head);
  item.helper(value.repository, ["start", "--summary", "next-cycle"]);
  item.commitFile(value.repository, "next-cycle.txt", "next cycle\n");
  assert.equal(item.helper(value.repository, ["publish", "--remote", "origin"]).payload.status, "published");
});

scenario("first_release_without_registered_cycle_uses_local_default_branch", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  configurePostReleasePolicy(item, repository, { schema: 4, action: "push_release_branch" });
  const value = { repository, version: "2.0.0", date: "20260910" };
  value.context = item.prepareReleaseContext(repository, value);
  const released = item.helper(repository, releaseArgs(value)).payload;
  assert.equal(released.status, "released");
  assert.deepEqual(released.preservedBranches, []);
  assert.equal(item.state(repository).lastRelease.defaultBranch, "main");
});

scenario("harness_release_requires_managed_current_minute_stamp_before_merge_or_tag", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const branch = item.helper(repository, ["start", "--summary", "manual-future-version"]).payload.branch;
  const semver = item.prepareReleaseContext(repository, { version: "1.2.3", date: "20991231" });
  const initializationSkill = join(repository, ".agents/skills/desktop-instantiate-project/SKILL.md");
  mkdirSync(dirname(initializationSkill), { recursive: true });
  writeFileSync(initializationSkill, "# Active Harness initialization\n", "utf8");
  writeFileSync(join(repository, "Version.md"), "- 当前版本：`209912312359`\n- 时间版本起始值：`202607301002`\n", "utf8");
  const contextPath = join(repository, ".harness/release-context.json");
  const context = JSON.parse(readFileSync(contextPath, "utf8"));
  context.version = "209912312359";
  context.expectedTag = "v209912312359-20991231";
  const bytes = `${JSON.stringify(context, null, 2)}\n`;
  writeFileSync(contextPath, bytes, "utf8");
  item.git(repository, "add", "Version.md", ".agents/skills/desktop-instantiate-project/SKILL.md", ".harness/release-context.json");
  item.git(repository, "commit", "--quiet", "-m", "chore: construct future release context");
  const digest = createHash("sha256").update(bytes).digest("hex");
  const initialMain = item.git(repository, "rev-parse", "refs/heads/main").stdout.trim();
  const rejected = item.helper(repository, ["release", "--version", context.version, "--date", "20991231",
    "--release-context-sha256", digest], { success: false }).payload;
  assert.equal(rejected.code, "harness-version-stamp-invalid");
  assert.equal(item.git(repository, "rev-parse", "refs/heads/main").stdout.trim(), initialMain);
  assert.equal(item.gitUnchecked(repository, "show-ref", "--verify", "--quiet", `refs/tags/${context.expectedTag}`).status, 1);
  assert.equal(item.state(repository).cycle.branches[0].name, branch);
  assert.equal(item.state(repository).cycle.pendingRelease, null);
  assert.notEqual(semver.digest, digest);
});

scenario("harness_release_accepts_managed_timestamp_stamp", (item) => {
  const { repository } = item.initializeRepository({ remote: true });
  item.helper(repository, ["start", "--summary", "managed-time-version"]);
  const initializationSkill = join(repository, ".agents/skills/desktop-instantiate-project/SKILL.md");
  mkdirSync(dirname(initializationSkill), { recursive: true });
  writeFileSync(initializationSkill, "# Active Harness initialization\n", "utf8");
  writeFileSync(join(repository, "Version.md"), "- 当前版本：`202607301002`\n- 时间版本起始值：`202607301002`\n", "utf8");
  item.git(repository, "add", "Version.md", ".agents/skills/desktop-instantiate-project/SKILL.md");
  item.git(repository, "commit", "--quiet", "-m", "chore: add Harness version source");
  const { version } = stampHarnessVersion(repository);
  item.git(repository, "add", "Version.md");
  item.git(repository, "commit", "--quiet", "-m", "chore: stamp Harness release version");
  const date = version.slice(0, 8);
  const context = item.prepareReleaseContext(repository, { version, date });
  const released = item.helper(repository, ["release", "--version", version, "--date", date,
    "--release-context-sha256", context.digest]).payload;
  assert.equal(released.status, "released");
  assert.equal(released.tag, `v${version}-${date}`);
  assert.equal(item.state(repository).lastRelease.postReleaseAction, null);
  const pushed = item.helper(repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.equal(pushed.head, released.head);
  assert.equal(item.git(repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], released.head);
  assert.equal(item.git(repository, "ls-remote", "--tags", "origin", `refs/tags/${released.tag}`).stdout.split("\t")[0], released.head);
});

scenario("harness_release_rechecks_final_version_after_all_registered_merges", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  item.helper(repository, ["start", "--summary", "first-registered"]);
  const initializationSkill = join(repository, ".agents/skills/desktop-instantiate-project/SKILL.md");
  mkdirSync(dirname(initializationSkill), { recursive: true });
  writeFileSync(initializationSkill, "# Active Harness initialization\n", "utf8");
  writeFileSync(join(repository, "Version.md"), "- 当前版本：`202607301002`\n- 时间版本起始值：`202607301002`\n", "utf8");
  item.git(repository, "add", "Version.md", ".agents/skills/desktop-instantiate-project/SKILL.md");
  item.git(repository, "commit", "--quiet", "-m", "chore: add Harness version source");
  const { version } = stampHarnessVersion(repository);
  item.git(repository, "add", "Version.md");
  item.git(repository, "commit", "--quiet", "-m", "chore: stamp Harness release version");
  const date = version.slice(0, 8);
  const context = item.prepareReleaseContext(repository, { version, date });
  const laterWorktree = join(item.temporary, "later-registered");
  item.git(repository, "worktree", "add", "--quiet", "-b", "feature-later-version", laterWorktree, "HEAD");
  item.helper(repository, ["track-worktree", "--worktree", laterWorktree]);
  item.commitFile(laterWorktree, "Version.md", "- 当前版本：`209912312359`\n- 时间版本起始值：`202607301002`\n");
  const rejected = item.helper(repository, ["release", "--version", version, "--date", date,
    "--release-context-sha256", context.digest], { success: false }).payload;
  assert.equal(rejected.code, "harness-version-stamp-invalid");
  assert.equal(item.gitUnchecked(repository, "show-ref", "--verify", "--quiet", `refs/tags/v${version}-${date}`).status, 1);
  assert.equal(item.state(repository).cycle.pendingRelease.head, null);
});

scenario("downstream_release_rechecks_final_cargo_and_target_version_after_merges", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  configurePostReleasePolicy(item, repository, { schema: 4, action: "push_release_branch" });
  item.helper(repository, ["start", "--summary", "first-registered"]);
  const gatePath = join(repository, ".agents/skills/desktop-manage-version/scripts/version_gate.mjs");
  mkdirSync(dirname(gatePath), { recursive: true });
  copyFileSync(new URL("../../desktop-manage-version/scripts/version_gate.mjs", import.meta.url), gatePath);
  writeFileSync(join(repository, "Cargo.toml"), "[workspace]\nmembers = []\n[workspace.package]\nversion = \"1.2.3\"\n", "utf8");
  initializeProductVersion(repository, { migrationApproved: true });
  assert.equal(existsSync(join(repository, ".harness/version-state.json")), true);
  item.git(repository, "add", "Cargo.toml", ".harness/version-state.json", ".agents/skills/desktop-manage-version/scripts/version_gate.mjs");
  item.git(repository, "commit", "--quiet", "-m", "chore: add product version facts");
  const context = item.prepareReleaseContext(repository, { version: "1.2.3", date: "20260919" });
  const laterWorktree = join(item.temporary, "later-registered");
  item.git(repository, "worktree", "add", "--quiet", "-b", "feature-later-product-version", laterWorktree, "HEAD");
  item.helper(repository, ["track-worktree", "--worktree", laterWorktree]);
  writeFileSync(join(laterWorktree, "Cargo.toml"), "[workspace]\nmembers = []\n[workspace.package]\nversion = \"1.2.4\"\n", "utf8");
  const statePath = join(laterWorktree, ".harness/version-state.json");
  const state = JSON.parse(readFileSync(statePath, "utf8"));
  state.target_version = "1.2.4";
  writeFileSync(statePath, `${JSON.stringify(state, null, 2)}\n`, "utf8");
  item.git(laterWorktree, "add", "Cargo.toml", ".harness/version-state.json");
  item.git(laterWorktree, "commit", "--quiet", "-m", "chore: advance product version after context");
  const rejected = item.helper(repository, ["release", "--version", "1.2.3", "--date", "20260919",
    "--release-context-sha256", context.digest], { success: false }).payload;
  assert.equal(rejected.code, "release-version-mismatch");
  assert.equal(item.gitUnchecked(repository, "show-ref", "--verify", "--quiet", "refs/tags/v1.2.3-20260919").status, 1);
  assert.equal(item.state(repository).cycle.pendingRelease.head, null);
});

scenario("enabled_review_rejects_registered_branch_added_after_review", (item) => {
  const value = candidate(item, { remote: false, reviewSelection: "enabled" });
  const laterWorktree = join(item.temporary, "unreviewed-worktree");
  item.git(value.repository, "worktree", "add", "--quiet", "-b", "feature-unreviewed", laterWorktree, "HEAD");
  item.helper(value.repository, ["track-worktree", "--worktree", laterWorktree]);
  item.commitFile(laterWorktree, "unreviewed.txt", "unreviewed source\n");
  const mainBefore = item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim();
  const rejected = item.helper(value.source, releaseArgs(value), { success: false }).payload;
  assert.equal(rejected.code, "release-review-scope-changed");
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim(), mainBefore);
  assert.equal(item.gitUnchecked(value.repository, "show-ref", "--verify", "--quiet", "refs/tags/v1.2.3-20260909").status, 1);
  assert.equal(item.state(value.repository).cycle.pendingRelease, null);
});

scenario("enabled_review_releases_when_source_and_metadata_are_unchanged", (item) => {
  const value = candidate(item, { remote: false, reviewSelection: "enabled" });
  const released = item.helper(value.source, releaseArgs(value)).payload;
  assert.equal(released.status, "released");
  assert.equal(item.git(value.repository, "rev-parse", "refs/tags/v1.2.3-20260909^{commit}").stdout.trim(), released.head);
});

scenario("enabled_review_rejects_forged_scope_digest_before_release_state_changes", (item) => {
  const value = candidate(item, { remote: false, reviewSelection: "enabled" });
  const contextPath = join(value.repository, ".harness/release-context.json");
  const context = JSON.parse(readFileSync(contextPath, "utf8"));
  context.releaseReview.scopeDiffSha256 = "0".repeat(64);
  const bytes = canonicalBytes(context);
  writeFileSync(contextPath, bytes);
  item.git(value.repository, "add", ".harness/release-context.json");
  item.git(value.repository, "commit", "--quiet", "-m", "chore: forge review scope");
  const digest = createHash("sha256").update(bytes).digest("hex");
  const rejected = item.helper(value.repository, ["release", "--version", value.version, "--date", value.date,
    "--release-context-sha256", digest], { success: false }).payload;
  assert.equal(rejected.code, "release-context-invalid");
  assert.equal(item.state(value.repository).cycle.pendingRelease, null);
  assert.equal(item.gitUnchecked(value.repository, "show-ref", "--verify", "--quiet", "refs/tags/v1.2.3-20260909").status, 1);
});

scenario("enabled_review_rejects_source_added_after_review_even_if_premerged", (item) => {
  const value = candidate(item, { remote: false, reviewSelection: "enabled" });
  item.git(value.source, "branch", "feature-unreviewed", "HEAD");
  item.git(value.source, "switch", "feature-unreviewed");
  item.commitFile(value.source, "unreviewed.txt", "unreviewed source\n");
  item.git(value.source, "switch", value.branch);
  item.git(value.source, "merge", "--no-edit", "feature-unreviewed");
  const mainBefore = item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim();
  const rejected = item.helper(value.source, releaseArgs(value), { success: false }).payload;
  assert.equal(rejected.code, "release-review-scope-changed");
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim(), mainBefore);
  assert.equal(item.state(value.repository).cycle.pendingRelease, null);
});

scenario("enabled_review_rejects_default_branch_changes_after_merge_before_tag", (item) => {
  const value = candidate(item, { remote: false, reviewSelection: "enabled" });
  item.git(value.repository, "switch", "main");
  item.commitFile(value.repository, "main-only.txt", "unreviewed main change\n");
  item.git(value.repository, "switch", value.branch);
  const rejected = item.helper(value.source, releaseArgs(value), { success: false }).payload;
  assert.equal(rejected.code, "release-review-scope-changed");
  assert.equal(item.gitUnchecked(value.repository, "show-ref", "--verify", "--quiet", "refs/tags/v1.2.3-20260909").status, 1);
  assert.equal(item.state(value.repository).cycle.pendingRelease.head, null);
});

scenario("release_from_task_worktree_keeps_worktree_and_local_branch", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  configurePostReleasePolicy(item, repository, { schema: 4, action: "push_release_branch" });
  const task = join(item.temporary, "task-worktree");
  item.git(repository, "worktree", "add", "--quiet", "--detach", task, "HEAD");
  const branch = item.helper(task, ["start", "--summary", "task-result"]).payload.branch;
  item.commitFile(task, "task.txt", "task result\n");
  const context = item.prepareReleaseContext(task, { version: "3.0.0", date: "20260912" });
  const released = item.helper(task, [
    "release", "--version", "3.0.0", "--date", "20260912",
    "--release-context-sha256", context.digest,
  ], { cwd: task }).payload;
  assert.equal(released.status, "released");
  assert.equal(released.head, context.head);
  assert.equal(existsSync(task), true);
  assert.equal(item.localBranchExists(repository, branch), true);
  assert.deepEqual(item.state(repository).releasedResources[0].worktrees.map((entry) => entry.path), [realpathSync(task)]);
});

scenario("new_cycle_from_old_task_worktree_starts_at_verified_release_head", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  configurePostReleasePolicy(item, repository, { schema: 4, action: "push_release_branch" });
  const task = join(item.temporary, "task-worktree");
  item.git(repository, "worktree", "add", "--quiet", "--detach", task, "HEAD");
  const oldBranch = item.helper(task, ["start", "--summary", "old-task"]).payload.branch;
  item.commitFile(task, "task.txt", "task result\n");
  const context = item.prepareReleaseContext(task, { version: "3.2.0", date: "20260912" });
  item.commitFile(repository, "main-only.txt", "main result\n");
  const released = item.helper(task, [
    "release", "--version", "3.2.0", "--date", "20260912",
    "--release-context-sha256", context.digest,
  ]).payload;
  assert.notEqual(released.head, context.head);
  const started = item.helper(task, ["start", "--summary", "next-task"]).payload;
  assert.equal(started.head, released.head);
  assert.equal(item.git(task, "rev-parse", "HEAD").stdout.trim(), released.head);
  assert.deepEqual(item.state(repository).cycle.branches.map((entry) => entry.name), [started.branch]);
  assert.deepEqual(item.state(repository).releasedResources[0].branches.map((entry) => entry.name), [oldBranch]);
  assert.equal(item.localBranchExists(repository, oldBranch), true);
});

scenario("downstream_cannot_rerelease_same_or_lower_version_under_new_date", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  item.helper(value.repository, ["start", "--summary", "maintenance"]);
  item.commitFile(value.repository, "maintenance.txt", "maintenance\n");
  const before = item.git(value.repository, "rev-parse", "refs/heads/main").stdout;
  for (const version of ["1.2.3", "1.2.2"]) {
    const context = item.prepareReleaseContext(value.repository, { version, date: "20260910" });
    const rejected = item.helper(value.repository, ["release", "--version", version, "--date", "20260910",
      "--release-context-sha256", context.digest], { success: false }).payload;
    assert.equal(rejected.code, "release-version-not-newer", version);
    item.git(value.repository, "reset", "--hard", "--quiet", "HEAD~1");
  }
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/main").stdout, before);
  assert.equal(item.git(value.repository, "tag", "--list", "v1.2.*-20260910").stdout, "");
  assert.equal(item.state(value.repository).cycle.pendingRelease, null);
});

scenario("release_rejects_old_mode_flags_before_state_mutation", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const digest = "0".repeat(64);
  for (const flag of [["--local-only"], ["--remote", "origin"], ["--also-remote", "origin"]]) {
    const rejected = item.helper(repository, [
      "release", "--version", "3.1.0", "--date", "20260913",
      "--release-context-sha256", digest, ...flag,
    ], { success: false }).payload;
    assert.equal(rejected.code, "invalid-argument");
  }
  for (const args of [["push-release"], ["push-release", "--remote", "origin", "--also-remote", "mirror"]]) {
    assert.equal(item.helper(repository, args, { success: false }).payload.code, "invalid-argument");
  }
  assert.equal(existsSync(join(repository, ".git/agent-first-harness/git-lifecycle.json")), false);
});

scenario("invalid_context_fails_before_pending_release", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  const context = item.prepareReleaseContext(repository, { version: "4.0.0", date: "20260915" });
  const path = join(repository, ".harness/release-context.json");
  const value = JSON.parse(readFileSync(path, "utf8"));
  value.releaseReview.checks = ["unexpected"];
  writeFileSync(path, JSON.stringify(value, null, 2) + "\n", "utf8");
  item.git(repository, "add", ".harness/release-context.json");
  item.git(repository, "commit", "--quiet", "-m", "test: corrupt context");
  const rejected = item.helper(repository, [
    "release", "--version", "4.0.0", "--date", "20260915",
    "--release-context-sha256", context.digest,
  ], { success: false }).payload;
  assert.equal(rejected.code, "release-context-invalid");
  assert.equal(existsSync(join(repository, ".git/agent-first-harness/git-lifecycle.json")), false);
});

scenario("release_tag_conflict_keeps_registered_resources_and_pending_head", (item) => {
  const value = candidate(item, { remote: false, version: "5.0.0", date: "20260917" });
  const oldHead = item.git(value.repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
  item.git(value.repository, "tag", "v5.0.0-20260917", oldHead);
  const rejected = item.helper(value.repository, releaseArgs(value), { success: false }).payload;
  assert.equal(rejected.code, "tag-conflict");
  assert.equal(item.localBranchExists(value.repository, value.branch), true);
  assert.equal(item.state(value.repository).cycle.pendingRelease.head, value.context.head);
  assert.equal(item.state(value.repository).lastRelease, null);
});

scenario("dirty_registered_worktree_blocks_release_then_clean_retry_succeeds", (item) => {
  const { repository } = item.initializeRepository({ remote: false });
  configurePostReleasePolicy(item, repository, { schema: 4, action: "push_release_branch" });
  const task = join(item.temporary, "task");
  item.git(repository, "worktree", "add", "--quiet", "--detach", task, "HEAD");
  item.helper(task, ["start", "--summary", "dirty-task"]);
  item.commitFile(task, "task.txt", "task\n");
  const context = item.prepareReleaseContext(task, { version: "5.1.0", date: "20260918" });
  writeFileSync(join(task, "untracked.txt"), "dirty\n", "utf8");
  const args = ["release", "--version", "5.1.0", "--date", "20260918", "--release-context-sha256", context.digest];
  assert.equal(item.helper(task, args, { success: false }).payload.code, "dirty-worktree");
  item.git(task, "clean", "-f", "--", "untracked.txt");
  assert.equal(item.helper(task, args, { cwd: task }).payload.status, "released");
});

scenario("push_release_is_separate_explicit_idempotent_operation", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  const pushed = item.helper(value.repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.equal(pushed.branch, "release");
  assert.equal(pushed.defaultBranch, "main");
  assert.equal(pushed.head, value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout.split("\t")[0], value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/main").stdout.split("\t")[0], value.context.head);
  assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "already-pushed");
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/release^{commit}").stdout.trim(), value.context.head);
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
});

for (const [name, policy] of [
  ["missing", null],
  ["schema-v3", { schema: 3 }],
  ["pending", { schema: 4, action: "pending" }],
]) {
  scenario(`release_rejects_${name}_policy_before_any_ref_change`, (item) => {
    const value = candidate(item, { postReleasePolicy: policy });
    const main = item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim();
    const rejected = item.helper(value.repository, releaseArgs(value), { success: false }).payload;
    assert.equal(rejected.code, "post-release-action-invalid");
    assert.equal(item.localBranchExists(value.repository, "release"), false);
    assert.equal(item.remoteBranchExists(value.repository, "origin", "release"), false);
    assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
    assert.equal(item.git(value.repository, "rev-parse", "refs/heads/main").stdout.trim(), main);
    assert.equal(item.gitUnchecked(value.repository, "show-ref", "--verify", "--quiet", "refs/tags/v1.2.3-20260909").status, 1);
    assert.equal(item.state(value.repository).cycle.pendingRelease, null);
  });
}

scenario("push_release_rejects_frozen_local_package_choice_before_ref_change", (item) => {
  const value = candidate(item, { postReleasePolicy: { schema: 4, action: "local_package" } });
  const released = item.helper(value.repository, releaseArgs(value)).payload;
  assert.equal(item.state(value.repository).lastRelease.postReleaseAction, "local_package");
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "post-release-action-mismatch");
  assert.equal(item.localBranchExists(value.repository, "release"), false);
  assert.equal(item.remoteBranchExists(value.repository, "origin", "release"), false);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", `refs/tags/${released.tag}`).stdout, "");
});

scenario("push_release_keeps_the_release_choice_after_a_future_policy_switch", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  item.git(value.repository, "switch", "--quiet", "-c", "feature-next-policy");
  const policyPath = join(value.repository, "docs/AGENT_POLICY.md");
  writeFileSync(policyPath, readFileSync(policyPath, "utf8").replace(
    "post_release_action: push_release_branch", "post_release_action: local_package"), "utf8");
  writeFileSync(join(value.repository, "Cargo.toml"),
    '[workspace]\nmembers = []\n[workspace.package]\nversion = "1.2.3"\n[workspace.metadata.agent-first-harness]\ninterfaces = ["cli"]\ntarget-platforms = ["macos"]\n', "utf8");
  item.git(value.repository, "add", "docs/AGENT_POLICY.md", "Cargo.toml");
  item.git(value.repository, "commit", "--quiet", "-m", "chore: choose local package for future releases");
  const pushed = item.helper(value.repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.equal(item.state(value.repository).lastRelease.postReleaseAction, "push_release_branch");
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], value.context.head);
});

scenario("push_release_after_new_cycle_preserves_current_checkout", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  const next = item.helper(value.repository, ["start", "--summary", "next-cycle"]).payload;
  const checkoutHead = item.git(value.repository, "rev-parse", "HEAD").stdout.trim();
  assert.equal(next.branch.startsWith("feature-"), true);
  const pushed = item.helper(value.repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.equal(item.git(value.repository, "branch", "--show-current").stdout.trim(), next.branch);
  assert.equal(item.git(value.repository, "rev-parse", "HEAD").stdout.trim(), checkoutHead);
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/main^{commit}").stdout.trim(), value.context.head);
  assert.equal(item.git(value.repository, "rev-parse", "refs/tags/v1.2.3-20260909^{commit}").stdout.trim(), value.context.head);
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
});

scenario("push_release_targets_release_even_when_remote_default_differs", (item) => {
  const value = candidate(item, { remote: false });
  item.addBareRemote(value.repository, "mirror", "stable");
  item.helper(value.repository, releaseArgs(value));
  const pushed = item.helper(value.repository, ["push-release", "--remote", "mirror"]).payload;
  assert.equal(pushed.branch, "release");
  assert.equal(pushed.defaultBranch, "stable");
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "mirror", "refs/heads/release").stdout.split("\t")[0], value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "mirror", "refs/heads/stable").stdout.split("\t")[0], value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "mirror", `refs/tags/${pushed.tag}`).stdout.split("\t")[0], value.context.head);
});

scenario("push_release_rejects_option_like_remote_name", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  const rejected = item.helper(value.repository, ["push-release", "--remote", "-invalid"], { success: false }).payload;
  assert.equal(rejected.code, "invalid-argument");
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
});

scenario("push_release_rejects_foreign_local_release_branch_before_remote_write", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  const initialHead = item.git(value.repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
  item.git(value.repository, "branch", "release", initialHead);
  item.git(value.repository, "tag", "v0.0.1-20260901", initialHead);
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "local-release-conflict");
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/release^{commit}").stdout.trim(), initialHead);
  assert.equal(item.remoteBranchExists(value.repository, "origin", "release"), false);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
});

scenario("push_release_advances_historical_v2_release_branch_after_new_release", (item) => {
  const first = candidate(item);
  item.helper(first.repository, releaseArgs(first));
  const completed = item.state(first.repository);
  item.git(first.repository, "branch", "release", first.context.head);
  item.git(first.repository, "push", "--quiet", "origin", "refs/heads/release:refs/heads/release");
  item.git(first.repository, "push", "--quiet", "origin", `refs/tags/${completed.lastRelease.tag}`);
  const legacy = {
    schemaVersion: 2, remote: null, defaultBranch: "main", cycle: null, pendingPublish: null,
    lastRelease: {
      tag: completed.lastRelease.tag, head: completed.lastRelease.head,
      date: completed.lastRelease.date, version: completed.lastRelease.version,
      releaseContextSha256: completed.lastRelease.releaseContextSha256,
      gitPublication: "local", remote: null,
    },
  };
  const common = item.git(first.repository, "rev-parse", "--path-format=absolute", "--git-common-dir").stdout.trim();
  writeFileSync(join(common, "agent-first-harness/git-lifecycle.json"), `${JSON.stringify(legacy, null, 2)}\n`, "utf8");
  item.helper(first.repository, ["start", "--summary", "after-v2-release"]);
  assert.deepEqual(item.state(first.repository).releasedResources, []);
  const unbound = item.helper(first.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(unbound.code, "post-release-action-unbound");
  assert.equal(item.git(first.repository, "rev-parse", "refs/heads/release^{commit}").stdout.trim(), first.context.head);
  item.commitFile(first.repository, "after-v2.txt", "after migration\n");
  const second = {
    repository: first.repository, version: "1.2.4", date: "20260910",
    context: item.prepareReleaseContext(first.repository, { version: "1.2.4", date: "20260910" }),
  };
  item.helper(first.repository, releaseArgs(second));
  assert.equal(item.state(first.repository).releasedResources.some((entry) => entry.head === first.context.head), false);
  const pushed = item.helper(first.repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.equal(item.git(first.repository, "rev-parse", "refs/heads/release^{commit}").stdout.trim(), second.context.head);
  assert.equal(item.git(first.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], second.context.head);
  assert.equal(item.git(first.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.4-20260910").stdout.split("\t")[0], second.context.head);
});

scenario("push_release_advances_recorded_local_branch_after_checked_out_worktree_is_removed", (item) => {
  const first = candidate(item);
  item.helper(first.repository, releaseArgs(first));
  item.helper(first.repository, ["push-release", "--remote", "origin"]);
  item.helper(first.repository, ["start", "--summary", "following-release"]);
  item.commitFile(first.repository, "following-release.txt", "following\n");
  const second = {
    repository: first.repository,
    version: "1.2.4",
    date: "20260910",
    context: item.prepareReleaseContext(first.repository, { version: "1.2.4", date: "20260910" }),
  };
  item.helper(first.repository, releaseArgs(second));
  const releaseWorktree = join(item.temporary, "release-worktree");
  item.git(first.repository, "worktree", "add", "--quiet", releaseWorktree, "release");
  const rejected = item.helper(first.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "local-release-conflict");
  assert.equal(item.git(first.repository, "rev-parse", "refs/heads/release^{commit}").stdout.trim(), first.context.head);
  assert.equal(item.git(first.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], first.context.head);
  item.git(first.repository, "worktree", "remove", "--force", releaseWorktree);
  const pushed = item.helper(first.repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.equal(item.git(first.repository, "rev-parse", "refs/heads/release^{commit}").stdout.trim(), second.context.head);
  assert.equal(item.git(first.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], second.context.head);
  assert.equal(item.git(first.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.4-20260910").stdout.split("\t")[0], second.context.head);
});

scenario("push_release_rejects_non_fast_forward_remote_release_branch", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  const initialHead = item.git(value.repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
  item.git(value.repository, "switch", "--quiet", "-c", "divergent-release", initialHead);
  const foreignHead = item.commitFile(value.repository, "foreign-release.txt", "foreign\n");
  item.git(value.repository, "push", "--quiet", "origin", "HEAD:refs/heads/release");
  item.git(value.repository, "switch", "--quiet", "main");
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "release-push-partial");
  assert.match(rejected.message, /default branch was confirmed/u);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/main").stdout.split("\t")[0], value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], foreignHead);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
});

scenario("remote_tag_conflict_is_rejected_before_branch_push", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  const oldHead = item.git(value.repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
  item.git(value.bare, "tag", "v1.2.3-20260909", oldHead);
  const branchBefore = item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout;
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "tag-conflict");
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout, branchBefore);
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
});

scenario("legacy_uppercase_release_branch_is_rejected_locally_and_remotely", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  const oldHead = item.git(value.repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
  item.git(value.repository, "branch", "Release", oldHead);
  const local = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(local.code, "local-release-conflict");
  assert.equal(item.git(value.repository, "rev-parse", "refs/heads/Release^{commit}").stdout.trim(), oldHead);
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "--tags", "origin", "refs/heads/release", "refs/tags/v1.2.3-20260909").stdout, "");
  item.git(value.repository, "branch", "-D", "Release");
  item.git(value.bare, "branch", "Release", oldHead);
  const remote = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(remote.code, "remote-release-conflict");
  assert.equal(item.git(value.repository, "for-each-ref", "--format=%(refname)", "refs/heads/release").stdout, "");
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
});

scenario("tag_push_failure_reports_partial_success_and_retry_keeps_release_complete", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  item.installHook(value.bare, 'while read old new ref; do case "$ref" in refs/tags/*) exit 1;; esac; done\nexit 0\n');
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "release-push-partial");
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], value.context.head);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
  rmSync(join(value.bare, "hooks/pre-receive"));
  assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "release-pushed");
});

scenario("push_release_rechecks_release_branch_and_tag_together_after_tag_push", (item) => {
  const value = candidate(item);
  const originalRemoteHead = item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/main").stdout.split("\t")[0];
  item.helper(value.repository, releaseArgs(value));
  const hook = join(value.bare, "hooks/post-receive");
  writeFileSync(hook, `#!/bin/sh\nwhile read old new ref; do\n  case "$ref" in refs/tags/*) git update-ref refs/heads/release ${originalRemoteHead};; esac\ndone\n`, "utf8");
  chmodSync(hook, 0o755);
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "release-push-uncertain");
  assert.equal(item.git(value.repository, "ls-remote", "--heads", "origin", "refs/heads/release").stdout.split("\t")[0], originalRemoteHead);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout.split("\t")[0], value.context.head);
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
  rmSync(hook);
  assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "release-pushed");
});

scenario("push_release_rechecks_local_tag_after_push_hook_changes_it", (item) => {
  const value = candidate(item);
  const oldHead = item.git(value.repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
  item.helper(value.repository, releaseArgs(value));
  const hook = join(value.repository, ".git/hooks/pre-push");
  writeFileSync(hook, `#!/bin/sh\nwhile read local_ref local_sha remote_ref remote_sha; do\n  case "$local_ref" in refs/tags/*) git tag -f v1.2.3-20260909 ${oldHead} >/dev/null;; esac\ndone\n`, "utf8");
  chmodSync(hook, 0o755);
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "release-push-uncertain");
  assert.equal(item.git(value.repository, "rev-parse", "refs/tags/v1.2.3-20260909^{commit}").stdout.trim(), oldHead);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout.split("\t")[0], value.context.head);
  assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
  rmSync(hook);
  item.git(value.repository, "tag", "-f", "v1.2.3-20260909", value.context.head);
  assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "already-pushed");
});

scenario("push_release_repairs_default_branch_when_release_and_tag_already_match", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  item.git(value.repository, "push", "--quiet", "origin", `${value.context.head}:refs/heads/release`, `refs/tags/v1.2.3-20260909`);
  const pushed = item.helper(value.repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.equal(item.git(value.bare, "rev-parse", "refs/heads/main").stdout.trim(), value.context.head);
  assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "already-pushed");
});

for (const mode of ["rejected", "divergent"]) {
  scenario(`push_release_default_branch_${mode}_stops_before_release_and_tag`, (item) => {
    const value = candidate(item);
    item.helper(value.repository, releaseArgs(value));
    if (mode === "rejected") {
      item.installHook(value.bare, 'while read old new ref; do case "$ref" in refs/heads/main) exit 1;; esac; done\nexit 0\n');
    } else {
      const initial = item.git(value.repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
      item.git(value.repository, "switch", "--quiet", "-c", "foreign-main", initial);
      item.commitFile(value.repository, "foreign.txt", "foreign\n");
      item.git(value.repository, "push", "--quiet", "origin", "HEAD:refs/heads/main");
      item.git(value.repository, "switch", "--quiet", "main");
    }
    const before = item.git(value.bare, "rev-parse", "refs/heads/main").stdout;
    const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
    assert.equal(rejected.code, "release-push-failed");
    assert.match(rejected.message, /release branch and tag were not attempted/u);
    assert.equal(item.git(value.bare, "rev-parse", "refs/heads/main").stdout, before);
    assert.equal(item.remoteBranchExists(value.repository, "origin", "release"), false);
    assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
    assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
  });
}

scenario("push_release_branch_failure_reports_default_success_and_retries", (item) => {
  const value = candidate(item);
  item.helper(value.repository, releaseArgs(value));
  item.installHook(value.bare, 'while read old new ref; do case "$ref" in refs/heads/release) exit 1;; esac; done\nexit 0\n');
  const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
  assert.equal(rejected.code, "release-push-partial");
  assert.match(rejected.message, /default branch was confirmed/u);
  assert.equal(item.git(value.bare, "rev-parse", "refs/heads/main").stdout.trim(), value.context.head);
  assert.equal(item.remoteBranchExists(value.repository, "origin", "release"), false);
  assert.equal(item.git(value.repository, "ls-remote", "--tags", "origin", "refs/tags/v1.2.3-20260909").stdout, "");
  rmSync(join(value.bare, "hooks/pre-receive"));
  assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "release-pushed");
});

for (const mode of ["detached", "missing"]) {
  scenario(`push_release_rejects_${mode}_remote_default_before_ref_changes`, (item) => {
    const value = candidate(item);
    item.helper(value.repository, releaseArgs(value));
    const initial = item.git(value.bare, "rev-parse", "HEAD").stdout.trim();
    if (mode === "detached") item.git(value.bare, "update-ref", "--no-deref", "HEAD", initial);
    else {
      item.git(value.bare, "symbolic-ref", "HEAD", `refs/heads/${mode}`);
    }
    const before = item.git(value.bare, "show-ref").stdout;
    const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
    assert.equal(rejected.code, "remote-default-unavailable");
    assert.equal(item.localBranchExists(value.repository, "release"), false);
    assert.equal(item.git(value.bare, "show-ref").stdout, before);
  });
}

for (const mode of ["head", "name"]) {
  scenario(`push_release_detects_remote_default_${mode}_drift_after_tag_push`, (item) => {
    const value = candidate(item);
    const oldHead = item.git(value.bare, "rev-parse", "HEAD").stdout.trim();
    item.helper(value.repository, releaseArgs(value));
    if (mode === "name") item.git(value.bare, "branch", "stable", oldHead);
    const command = mode === "head" ? `git update-ref refs/heads/main ${oldHead}`
      : `git update-ref refs/heads/stable ${value.context.head}; git symbolic-ref HEAD refs/heads/stable`;
    const hook = join(value.bare, "hooks/post-receive");
    writeFileSync(hook, `#!/bin/sh\nwhile read old new ref; do\n  case "$ref" in refs/tags/*) ${command};; esac\ndone\n`, "utf8");
    chmodSync(hook, 0o755);
    const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
    assert.equal(rejected.code, "release-push-uncertain");
    assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
    rmSync(hook);
    item.git(value.bare, "symbolic-ref", "HEAD", "refs/heads/main");
    assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status,
      mode === "head" ? "release-pushed" : "already-pushed");
  });
}

scenario("push_release_supports_release_as_remote_default_without_duplicate_push", (item) => {
  const value = candidate(item);
  item.git(value.bare, "branch", "release", "HEAD");
  item.git(value.bare, "symbolic-ref", "HEAD", "refs/heads/release");
  item.helper(value.repository, releaseArgs(value));
  const pushed = item.helper(value.repository, ["push-release", "--remote", "origin"]).payload;
  assert.equal(pushed.defaultBranch, "release");
  assert.equal(item.git(value.bare, "rev-parse", "HEAD").stdout.trim(), value.context.head);
  assert.equal(item.git(value.bare, "rev-parse", `refs/tags/${pushed.tag}`).stdout.trim(), value.context.head);
  assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "already-pushed");
});

for (const mode of ["rejected", "readback_failed"]) {
  scenario(`push_release_alias_default_${mode}_reports_only_tag_unattempted`, (item) => {
    const value = candidate(item);
    item.git(value.bare, "branch", "release", "HEAD");
    item.git(value.bare, "symbolic-ref", "HEAD", "refs/heads/release");
    const oldHead = item.git(value.bare, "rev-parse", "HEAD").stdout.trim();
    item.helper(value.repository, releaseArgs(value));
    const pushHook = join(value.repository, ".git/hooks/pre-push");
    if (mode === "rejected") item.installHook(value.bare, "exit 1\n");
    else {
      writeFileSync(pushHook, "#!/bin/sh\ngit remote set-url origin ./missing-release-readback\n", "utf8");
      chmodSync(pushHook, 0o755);
    }
    const rejected = item.helper(value.repository, ["push-release", "--remote", "origin"], { success: false }).payload;
    assert.equal(rejected.code, mode === "rejected" ? "release-push-failed" : "release-push-uncertain");
    assert.match(rejected.message, /tag was not attempted/u);
    assert.doesNotMatch(rejected.message, /release branch and tag were not attempted/u);
    assert.equal(item.git(value.bare, "rev-parse", "HEAD").stdout.trim(),
      mode === "rejected" ? oldHead : value.context.head);
    assert.equal(item.git(value.bare, "for-each-ref", "--format=%(refname)", "refs/tags/v1.2.3-20260909").stdout, "");
    assert.equal(item.state(value.repository).lastRelease.head, value.context.head);
    if (mode === "rejected") rmSync(join(value.bare, "hooks/pre-receive"));
    else {
      rmSync(pushHook);
      item.git(value.repository, "remote", "set-url", "origin", value.bare);
    }
    assert.equal(item.helper(value.repository, ["push-release", "--remote", "origin"]).payload.status, "release-pushed");
  });
}
