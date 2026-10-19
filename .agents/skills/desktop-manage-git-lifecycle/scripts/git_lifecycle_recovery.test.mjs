/** 跨机器发布恢复与当次精确推送授权的隔离回归。 */
import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { initialize } from "../../desktop-manage-version/scripts/version_gate.mjs";
import { lifecycleFixture } from "./git_lifecycle_test_support.mjs";

/** 计算恢复调用需要的原字节摘要。 */
function digest(bytes) { return createHash("sha256").update(bytes).digest("hex"); }

/** 建立本机旧清单、已吸收的远端发布和下一开发分支。 */
function recoveryFixture() {
  const item = lifecycleFixture();
  const { repository } = item.initializeRepository({ remote: true });
  const cargo = '[workspace]\nmembers = []\n[workspace.package]\nversion = "0.3.11"\n';
  writeFileSync(join(repository, "Cargo.toml"), cargo);
  initialize(repository, { migrationApproved: true });
  item.git(repository, "add", "Cargo.toml", ".harness/version-state.json");
  item.git(repository, "commit", "--quiet", "-m", "chore: version baseline");
  const old = item.prepareReleaseContext(repository, { version: "0.3.11", date: "20261018" });
  item.git(repository, "tag", "v0.3.11-20261018");
  writeFileSync(join(repository, "Cargo.toml"), cargo.replace("0.3.11", "0.3.13"));
  const updated = JSON.parse(readFileSync(join(repository, ".harness/version-state.json"), "utf8"));
  updated.target_version = "0.3.13";
  updated.cycle_base_version = "0.3.13";
  writeFileSync(join(repository, ".harness/version-state.json"), JSON.stringify(updated, null, 2) + "\n");
  item.git(repository, "add", "Cargo.toml", ".harness/version-state.json");
  item.git(repository, "commit", "--quiet", "-m", "chore: next version baseline");
  item.commitFile(repository, "source.txt", "release source\n");
  const context = item.prepareReleaseContext(repository, { version: "0.3.13", date: "20261019", reviewSelection: "enabled" });
  const tag = "v0.3.13-20261019";
  item.git(repository, "tag", tag);
  const versionState = JSON.parse(readFileSync(join(repository, ".harness/version-state.json"), "utf8"));
  versionState.last_release = { version: "0.3.13", source_commit: context.head };
  versionState.cycle_base_version = "0.3.13";
  item.commitFile(repository, ".harness/version-state.json", JSON.stringify(versionState, null, 2) + "\n");
  item.helper(repository, ["start", "--summary", "recover-release"]);
  const stateFile = join(repository, ".git/agent-first-harness/git-lifecycle.json");
  const state = item.state(repository);
  state.lastRelease = {
    tag: "v0.3.11-20261018", head: old.head, date: "20261018", version: "0.3.11",
    defaultBranch: "main", releaseContextSha256: old.digest, postReleaseAction: "local_package",
  };
  state.releasedResources = [{ tag: state.lastRelease.tag, head: old.head, branches: [], worktrees: [] }];
  writeFileSync(stateFile, JSON.stringify(state, null, 2) + "\n");
  const args = ["recover-release", "--tag", tag, "--source-commit", context.head,
    "--release-context-sha256", context.digest, "--expected-state-sha256", digest(readFileSync(stateFile)),
    "--recovery-approved", "yes"];
  return { ...item, repository, context, tag, stateFile, args };
}

/** 每个场景使用独立仓库并自动清理。 */
function scenario(name, callback) {
  test(name, () => {
    const item = recoveryFixture();
    try { callback(item); } finally { item.cleanup(); }
  });
}

scenario("recover_preserves_exact_bytes_resources_refs_and_unknown_historical_action", (item) => {
  const before = readFileSync(item.stateFile);
  const state = JSON.parse(before);
  const refs = item.git(item.repository, "show-ref").stdout;
  const result = item.helper(item.repository, item.args).payload;
  assert.equal(result.status, "release-recovered");
  assert.deepEqual(readFileSync(result.backup), before);
  const after = item.state(item.repository);
  assert.deepEqual(after, { ...state, lastRelease: {
    tag: item.tag, head: item.context.head, date: "20261019", version: "0.3.13", defaultBranch: "main",
    releaseContextSha256: item.context.digest, postReleaseAction: null,
  } });
  assert.equal(item.git(item.repository, "show-ref").stdout, refs);
  assert.equal(item.helper(item.repository, item.args).payload.status, "already-recovered");
});

scenario("recover_preserves_valid_pending_changes_after_the_exact_finalized_baseline", (item) => {
  const file = join(item.repository, ".harness/version-state.json");
  const versionState = JSON.parse(readFileSync(file, "utf8"));
  versionState.target_version = "0.3.14";
  versionState.applied_bug_ids.push("AUTOSTART-HOME-001");
  versionState.pending_changes.push({ change_id: "AUTOSTART-HOME-001", kind: "bug-fix", required_version: "0.3.14" });
  item.commitFile(item.repository, "Cargo.toml", readFileSync(join(item.repository, "Cargo.toml"), "utf8").replace("0.3.13", "0.3.14"));
  item.commitFile(item.repository, ".harness/version-state.json", JSON.stringify(versionState, null, 2) + "\n");
  const before = readFileSync(file);
  const refs = item.git(item.repository, "show-ref").stdout;
  const resources = item.state(item.repository).cycle;
  assert.equal(item.helper(item.repository, item.args).payload.status, "release-recovered");
  assert.deepEqual(readFileSync(file), before);
  assert.deepEqual(item.state(item.repository).cycle, resources);
  assert.equal(item.git(item.repository, "show-ref").stdout, refs);
  assert.equal(item.state(item.repository).lastRelease.head, item.context.head);
});

scenario("recover_rejects_invalid_pending_changes_without_writing", (item) => {
  const file = join(item.repository, ".harness/version-state.json");
  const versionState = JSON.parse(readFileSync(file, "utf8"));
  versionState.pending_changes.push({ change_id: "INVALID-001", kind: "bug-fix", required_version: "0.3.14" });
  item.commitFile(item.repository, ".harness/version-state.json", JSON.stringify(versionState));
  rejectsWithoutWrites(item, item.args, "recovery-state-conflict");
});

scenario("recover_rejects_pending_changes_at_or_before_the_finalized_baseline", (item) => {
  const file = join(item.repository, ".harness/version-state.json");
  const versionState = JSON.parse(readFileSync(file, "utf8"));
  versionState.applied_bug_ids.push("STALE-001");
  versionState.pending_changes.push({ change_id: "STALE-001", kind: "bug-fix", required_version: "0.3.13" });
  item.commitFile(item.repository, ".harness/version-state.json", JSON.stringify(versionState));
  rejectsWithoutWrites(item, item.args, "recovery-state-conflict");
});

/** 断言拒绝不会改变清单、引用或生成恢复备份。 */
function rejectsWithoutWrites(item, args, code) {
  const before = readFileSync(item.stateFile);
  const refs = item.git(item.repository, "show-ref").stdout;
  assert.equal(item.helper(item.repository, args, { success: false }).payload.code, code);
  assert.deepEqual(readFileSync(item.stateFile), before);
  assert.equal(item.git(item.repository, "show-ref").stdout, refs);
  assert.equal(existsSync(join(item.repository, ".git/agent-first-harness/recovery-audits")), false);
}

scenario("recover_rejects_missing_approval_and_stale_state_digest", (item) => {
  rejectsWithoutWrites(item, item.args.slice(0, -2), "invalid-argument");
  rejectsWithoutWrites(item, item.args.map((value) => value === "--expected-state-sha256" ? value : value === digest(readFileSync(item.stateFile)) ? "0".repeat(64) : value), "recovery-state-changed");
});

scenario("recover_rejects_pending_operations", (item) => {
  const state = item.state(item.repository);
  state.pendingPublish = { head: item.context.head, targets: [{ remote: "origin", branch: "main", confirmed: false }] };
  state.remote = "origin";
  writeFileSync(item.stateFile, JSON.stringify(state));
  rejectsWithoutWrites(item, item.args, "lifecycle-in-progress");
});

scenario("recover_rejects_pending_release_and_legacy_schema", (item) => {
  const state = item.state(item.repository);
  state.cycle.pendingRelease = { ...state.lastRelease };
  delete state.cycle.pendingRelease.defaultBranch;
  writeFileSync(item.stateFile, JSON.stringify(state));
  rejectsWithoutWrites(item, item.args, "lifecycle-in-progress");
  state.cycle.pendingRelease = null;
  state.schemaVersion = 3;
  delete state.lastRelease.postReleaseAction;
  writeFileSync(item.stateFile, JSON.stringify(state));
  rejectsWithoutWrites(item, item.args, "recovery-state-conflict");
});

scenario("recover_rejects_context_digest_and_tag_commit_mismatch", (item) => {
  rejectsWithoutWrites(item, item.args.map((value) => value === item.context.digest ? "0".repeat(64) : value), "recovery-release-unverifiable");
  rejectsWithoutWrites(item, item.args.map((value) => value === item.context.head ? "0".repeat(40) : value), "recovery-release-unverifiable");
});

scenario("recover_rejects_current_finalized_state_conflict", (item) => {
  const state = JSON.parse(readFileSync(join(item.repository, ".harness/version-state.json"), "utf8"));
  state.last_release.source_commit = "0".repeat(40);
  item.commitFile(item.repository, ".harness/version-state.json", JSON.stringify(state));
  rejectsWithoutWrites(item, item.args, "recovery-state-conflict");
});

scenario("recover_rejects_divergent_default_branch", (item) => {
  item.git(item.repository, "update-ref", "refs/heads/main", item.state(item.repository).lastRelease.head);
  rejectsWithoutWrites(item, item.args, "recovery-state-conflict");
});

scenario("recover_rejects_same_version_tags_with_different_source", (item) => {
  item.git(item.repository, "tag", "v0.3.13-20261020", item.state(item.repository).lastRelease.head);
  rejectsWithoutWrites(item, item.args, "recovery-state-conflict");
});

scenario("recover_rejects_unreviewed_source_after_context_source_head", (item) => {
  item.commitFile(item.repository, "unreviewed.txt", "unreviewed\n");
  const head = item.git(item.repository, "rev-parse", "HEAD").stdout.trim();
  item.git(item.repository, "tag", "-f", item.tag, head);
  rejectsWithoutWrites(item, item.args.map((value) => value === item.context.head ? head : value), "recovery-release-unverifiable");
});

scenario("recover_rejects_symbolic_context_in_the_historical_commit", (item) => {
  const contextPath = join(item.repository, ".harness/release-context.json");
  rmSync(contextPath);
  symlinkSync("../release-notes.json", contextPath);
  item.git(item.repository, "add", ".harness/release-context.json");
  item.git(item.repository, "commit", "--quiet", "-m", "test: replace context with link");
  const head = item.git(item.repository, "rev-parse", "HEAD").stdout.trim();
  item.git(item.repository, "tag", "-f", item.tag, head);
  rejectsWithoutWrites(item, item.args.map((value) => value === item.context.head ? head : value), "recovery-release-unverifiable");
});

scenario("recover_rejects_dirty_tree_and_inconsistent_idempotent_identity", (item) => {
  writeFileSync(join(item.repository, "uncommitted.txt"), "keep user data\n");
  rejectsWithoutWrites(item, item.args, "dirty-worktree");
  rmSync(join(item.repository, "uncommitted.txt"));
  item.helper(item.repository, item.args);
  const state = item.state(item.repository);
  state.lastRelease.defaultBranch = "different";
  writeFileSync(item.stateFile, JSON.stringify(state));
  const before = readFileSync(item.stateFile);
  assert.equal(item.helper(item.repository, item.args, { success: false }).payload.code, "recovery-state-conflict");
  assert.deepEqual(readFileSync(item.stateFile), before);
});

scenario("recover_rejects_unsafe_backup_directory_without_state_change", (item) => {
  const outside = join(item.temporary, "outside");
  mkdirSync(outside);
  symlinkSync(outside, join(item.repository, ".git/agent-first-harness/recovery-audits"), "dir");
  const before = readFileSync(item.stateFile);
  assert.equal(item.helper(item.repository, item.args, { success: false }).payload.code, "recovery-backup-failed");
  assert.deepEqual(readFileSync(item.stateFile), before);
});

scenario("explicit_push_requires_both_exact_release_identifiers_and_preserves_policy", (item) => {
  item.helper(item.repository, item.args);
  const state = item.state(item.repository);
  const head = item.git(item.repository, "rev-parse", "HEAD").stdout.trim();
  state.lastRelease = { ...state.lastRelease, head, postReleaseAction: "local_package" };
  item.git(item.repository, "tag", "-f", item.tag, head);
  item.git(item.repository, "switch", "main");
  item.git(item.repository, "merge", "--ff-only", state.cycle.branches[0].name);
  writeFileSync(item.stateFile, JSON.stringify(state));
  const args = ["push-release", "--remote", "origin"];
  const before = readFileSync(item.stateFile);
  const remoteRefs = item.git(item.repository, "ls-remote", "origin").stdout;
  assert.equal(item.helper(item.repository, args, { success: false }).payload.code, "post-release-action-mismatch");
  for (const binding of [["--explicit-head", head], ["--explicit-tag", item.tag], ["--explicit-head", "0".repeat(40), "--explicit-tag", item.tag]]) {
    assert.equal(item.helper(item.repository, [...args, ...binding], { success: false }).payload.code, "explicit-release-mismatch");
    assert.equal(item.git(item.repository, "ls-remote", "origin").stdout, remoteRefs);
  }
  const pushed = item.helper(item.repository, [...args, "--explicit-head", head, "--explicit-tag", item.tag]).payload;
  assert.equal(pushed.status, "release-pushed");
  assert.deepEqual(readFileSync(item.stateFile), before);
  for (const ref of ["refs/heads/main", "refs/heads/release", `refs/tags/${item.tag}`]) {
    assert.equal(item.git(item.repository, "ls-remote", "origin", ref).stdout.trim().split(/\s/u)[0], head);
  }
});
