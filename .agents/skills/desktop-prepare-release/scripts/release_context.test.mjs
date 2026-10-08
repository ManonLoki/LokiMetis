/** 发布上下文 helper 的真实 Git 回归。 */

import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { stampHarnessVersion } from "./harness_version_clock.mjs";

const SCRIPT_DIRECTORY = dirname(fileURLToPath(import.meta.url));
const SCRIPT = join(SCRIPT_DIRECTORY, "release_context.mjs");

/** 使用发布审查约定的 Git patch 原始字节独立计算测试输入。 */
function scopeDiffSha256(root, base, head, env) {
  const result = spawnSync("git", [
    "-C", root, "-c", "core.quotePath=true",
    "diff", "--binary", "--full-index", "--no-renames", "--no-ext-diff", "--no-textconv",
    "--no-color", "--no-relative", "--src-prefix=a/", "--dst-prefix=b/",
    "--diff-algorithm=myers", "--no-indent-heuristic", "--unified=3",
    "--inter-hunk-context=0", "--submodule=short", "--ignore-submodules=none",
    base, head, "--",
  ], { env, maxBuffer: 16 * 1024 * 1024 });
  if (result.error || result.status !== 0) assert.fail(`cannot compute fixture Git diff: ${result.stderr?.toString("utf8")}`);
  return createHash("sha256").update(result.stdout).digest("hex");
}

/** 执行子进程并保留退出码、stdout 与 stderr。 */
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    env: options.env ?? process.env,
    encoding: "utf8",
    input: "",
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (options.check !== false && result.status !== 0) {
    assert.fail(`${command} ${args.join(" ")} failed: ${result.stderr}`);
  }
  return result;
}

/** 建立有主远端和待发布功能分支的隔离仓库。 */
function fixture({ harness = false } = {}) {
  const temporary = mkdtempSync(join(tmpdir(), "release-context-"));
  const root = join(temporary, "project");
  const remote = join(temporary, "remote.git");
  mkdirSync(root);
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: join(temporary, "global.gitconfig"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_TERMINAL_PROMPT: "0",
    GCM_INTERACTIVE: "Never",
  };
  const git = (...args) => run("git", ["-C", root, ...args], { env });
  const invoke = (...args) => run(process.execPath, [SCRIPT, ...args], {
    env,
    cwd: root,
    check: false,
  });
  run("git", ["init", "--quiet", "--bare", remote], { env });
  git("init", "--quiet", "--initial-branch=main");
  git("config", "--local", "user.name", "Release Context Test");
  git("config", "--local", "user.email", "release-context@example.com");
  writeFileSync(join(root, "README.md"), "baseline\n", "utf8");
  if (harness) {
    writeFileSync(join(root, "Version.md"), "- 当前版本：`202609080000`\n- 时间版本起始值：`202609080000`\n", "utf8");
    const skill = join(root, ".agents", "skills", "desktop-instantiate-project", "SKILL.md");
    mkdirSync(dirname(skill), { recursive: true });
    writeFileSync(skill, "# Harness source\n", "utf8");
  }
  git("add", ".");
  git("commit", "--quiet", "-m", "chore: baseline");
  const baseline = git("rev-parse", "HEAD").stdout.trim();
  run("git", ["-C", remote, "symbolic-ref", "HEAD", "refs/heads/main"], { env });
  git("remote", "add", "origin", remote);
  git("push", "--quiet", "-u", "origin", "main");
  git("switch", "--quiet", "-c", "feature-release-context-20260909");
  if (harness) stampHarnessVersion(root, new Date("2026-09-09T01:30:00.000Z"));
  writeFileSync(join(root, "source.txt"), "ready\n", "utf8");
  git("add", harness ? "." : "source.txt");
  git("commit", "--quiet", "-m", "feat: prepare source");
  const sourceHead = git("rev-parse", "HEAD").stdout.trim();
  const scopeHash = scopeDiffSha256(root, baseline, sourceHead, env);
  const writeContext = (extra = [], version = "1.2.3", reviewSelection = "disabled") => invoke(
    "write",
    "--project-root", root,
    "--source-head", sourceHead,
    "--version", version,
    "--release-date", "2026-09-09",
    "--default-branch", "main",
    "--review-selection", reviewSelection,
    "--scope-base", baseline,
    "--scope-diff-sha256", scopeHash,
    ...(reviewSelection === "enabled" ? [
      "--review-evidence-summary", "All required review checks passed for the recorded source diff.",
    ] : [
      "--review-reason", "Optional semantic review was not requested.",
      "--review-remaining-risk", "Semantic issues outside required checks may remain.",
    ]),
    ...extra,
  );
  const commitAndTag = ({ version = "1.2.3", reviewSelection = "disabled" } = {}) => {
    const written = writeContext([], version, reviewSelection);
    assert.equal(written.status, 0, written.stderr);
    const digest = JSON.parse(written.stdout).releaseContextSha256;
    git("add", ".harness/release-context.json");
    git("commit", "--quiet", "-m", "chore(release): record release context");
    git("switch", "--quiet", "main");
    git("merge", "--quiet", "--no-edit", "feature-release-context-20260909");
    const head = git("rev-parse", "HEAD").stdout.trim();
    git("tag", `v${version}-20260909`);
    return { head, digest };
  };
  const writeLifecycleState = (overrides = {}) => {
    const commonDir = git("rev-parse", "--path-format=absolute", "--git-common-dir").stdout.trim();
    const directory = join(commonDir, "agent-first-harness");
    mkdirSync(directory, { recursive: true });
    writeFileSync(join(directory, "git-lifecycle.json"), `${JSON.stringify({
      schemaVersion: 3,
      remote: null,
      defaultBranch: "main",
      cycle: null,
      pendingPublish: null,
      lastRelease: null,
      releasedResources: [],
      ...overrides,
    })}\n`, "utf8");
  };
  return {
    temporary, root, remote, git, invoke, writeContext, commitAndTag, writeLifecycleState, baseline, sourceHead, scopeHash,
  };
}

/** 确保每个 Git fixture 都在测试完成后精确清理。 */
function withFixture(name, callback, options = {}) {
  test(name, () => {
    const item = fixture(options);
    try {
      callback(item);
    } finally {
      rmSync(item.temporary, { recursive: true, force: true });
    }
  });
}

withFixture("write_derives_tag_and_canonical_context", (item) => {
  const result = item.writeContext();
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.expectedTag, "v1.2.3-20260909");
  const checked = item.invoke(
    "check", "--project-root", item.root,
    "--expected-version", "1.2.3",
    "--expected-sha256", payload.releaseContextSha256,
  );
  assert.equal(checked.status, 0, checked.stderr);
  const context = JSON.parse(readFileSync(join(item.root, ".harness/release-context.json"), "utf8"));
  assert.equal(context.schemaVersion, 3);
  assert.deepEqual(Object.keys(context), [
    "schemaVersion", "sourceHead", "version", "releaseDate", "expectedTag", "defaultBranch", "releaseReview",
  ]);
  assert.equal(context.sourceHead, item.sourceHead);
  assert.equal(context.defaultBranch, "main");
});

withFixture("enabled_review_recomputes_real_diff_on_write_check_and_verify", (item) => {
  const { head, digest } = item.commitAndTag({ reviewSelection: "enabled" });
  const contextPath = join(item.root, ".harness/release-context.json");
  const context = JSON.parse(readFileSync(contextPath, "utf8"));
  assert.equal(context.releaseReview.scopeDiffSha256, item.scopeHash);
  assert.equal(context.releaseReview.selection, "enabled");
  assert.equal(item.invoke("check", "--project-root", item.root).status, 0);
  const verified = item.invoke(
    "verify", "--project-root", item.root, "--expected-head", head, "--expected-sha256", digest,
  );
  assert.equal(verified.status, 0, verified.stderr);
});

withFixture("enabled_review_write_rejects_wrong_diff_digest_before_writing", (item) => {
  const rejected = item.writeContext(["--scope-diff-sha256", "0".repeat(64)], "1.2.3", "enabled");
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /scopeDiffSha256 does not match the Git diff/);
  assert.equal(item.git("status", "--porcelain").stdout, "");
});

withFixture("enabled_review_check_and_verify_reject_tampered_diff_digest", (item) => {
  item.commitAndTag({ reviewSelection: "enabled" });
  const path = join(item.root, ".harness/release-context.json");
  const context = JSON.parse(readFileSync(path, "utf8"));
  context.releaseReview.scopeDiffSha256 = "0".repeat(64);
  writeFileSync(path, `${JSON.stringify(context, null, 2)}\n`, "utf8");
  for (const mode of ["check", "verify"]) {
    const rejected = item.invoke(mode, "--project-root", item.root);
    assert.equal(rejected.status, 1);
    assert.match(rejected.stderr, /scopeDiffSha256 does not match the Git diff/);
  }
});

withFixture("enabled_review_requires_scope_base_to_precede_source_head", (item) => {
  item.git("switch", "--quiet", "-c", "unrelated-review-base", item.baseline);
  writeFileSync(join(item.root, "unrelated.txt"), "unrelated\n", "utf8");
  item.git("add", "unrelated.txt");
  item.git("commit", "--quiet", "-m", "feat: unrelated review base");
  const unrelated = item.git("rev-parse", "HEAD").stdout.trim();
  item.git("switch", "--quiet", "feature-release-context-20260909");
  const rejected = item.writeContext(["--scope-base", unrelated], "1.2.3", "enabled");
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /scopeBase must be an ancestor of sourceHead/);
});

withFixture("verify_accepts_clean_crlf_checkout_of_canonical_context", (item) => {
  item.git("config", "--local", "core.autocrlf", "true");
  const { head, digest } = item.commitAndTag();
  const path = join(item.root, ".harness/release-context.json");
  const canonical = readFileSync(path).toString("utf8").replaceAll("\r\n", "\n");
  writeFileSync(path, canonical.replaceAll("\n", "\r\n"), "utf8");
  item.git("add", ".harness/release-context.json");
  assert.equal(item.git("status", "--porcelain").stdout, "");
  const verified = item.invoke(
    "verify", "--project-root", item.root,
    "--expected-version", "1.2.3",
    "--expected-sha256", digest,
    "--expected-head", head,
  );
  assert.equal(verified.status, 0, verified.stderr);
  assert.equal(JSON.parse(verified.stdout).sourceCommit, head);
});

withFixture("schema_v2_release_context_is_rejected_without_silent_migration", (item) => {
  assert.equal(item.writeContext().status, 0);
  const path = join(item.root, ".harness/release-context.json");
  const context = JSON.parse(readFileSync(path, "utf8"));
  context.schemaVersion = 2;
  writeFileSync(path, `${JSON.stringify(context)}\n`, "utf8");
  const rejected = item.invoke("check", "--project-root", item.root);
  assert.equal(rejected.status, 1);
  assert.match(rejected.stderr, /fields or schemaVersion/);
});

withFixture("write_uses_explicit_branch_without_accessing_remote", (item) => {
  renameSync(item.remote, `${item.remote}.offline`);
  const result = item.writeContext();
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.defaultBranch, "main");
  assert.equal("gitPublication" in payload, false);
  assert.equal("remote" in payload, false);
  const context = JSON.parse(readFileSync(join(item.root, ".harness/release-context.json"), "utf8"));
  assert.equal(context.defaultBranch, "main");
});

withFixture("write_rejects_source_head_that_is_not_current_head", (item) => {
  writeFileSync(join(item.root, "drift.txt"), "later change\n", "utf8");
  item.git("add", "drift.txt");
  item.git("commit", "--quiet", "-m", "chore: drift after source review");
  const result = item.writeContext();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /sourceHead must equal the current HEAD before metadata commit/);
});

withFixture("verify_requires_clean_main_and_local_tag", (item) => {
  const { head, digest } = item.commitAndTag();
  const result = item.invoke(
    "verify", "--project-root", item.root,
    "--expected-head", head, "--expected-sha256", digest,
  );
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.sourceCommit, head);
  assert.equal(payload.expectedTag, "v1.2.3-20260909");
  assert.equal("gitPublication" in payload, false);
});

withFixture("verify_succeeds_without_remote_access", (item) => {
  const { head, digest } = item.commitAndTag();
  renameSync(item.remote, `${item.remote}.offline`);
  const result = item.invoke(
    "verify", "--project-root", item.root,
    "--expected-head", head, "--expected-sha256", digest,
  );
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.status, "released");
  assert.equal("remote" in payload, false);
});

withFixture("verify_rejects_unmerged_registered_cycle_branch", (item) => {
  const { head, digest } = item.commitAndTag();
  item.git("switch", "--quiet", "-c", "feature-unmerged", item.baseline);
  writeFileSync(join(item.root, "unmerged.txt"), "not released\n", "utf8");
  item.git("add", "unmerged.txt");
  item.git("commit", "--quiet", "-m", "feat: unmerged work");
  item.git("switch", "--quiet", "main");
  item.writeLifecycleState({ cycle: {
    branches: [{ name: "feature-unmerged", summary: "unmerged", createdAt: "2026-09-09T00:00:00Z" }],
    worktrees: [], pendingRelease: null,
  } });
  const result = item.invoke("verify", "--project-root", item.root, "--expected-head", head, "--expected-sha256", digest);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /registered branch feature-unmerged is not included/);
});

withFixture("verify_rejects_tag_that_omits_reviewed_source", (item) => {
  const { head } = item.commitAndTag();
  item.git("switch", "--quiet", "-c", "unrelated-release", item.baseline);
  item.git("restore", `--source=${head}`, "--", ".harness/release-context.json");
  item.git("add", ".harness/release-context.json");
  item.git("commit", "--quiet", "-m", "chore: copy release context without source");
  const unrelated = item.git("rev-parse", "HEAD").stdout.trim();
  item.git("branch", "-f", "main", unrelated);
  item.git("tag", "-f", "v1.2.3-20260909", unrelated);
  item.git("switch", "--quiet", "main");
  const result = item.invoke("verify", "--project-root", item.root, "--expected-head", unrelated);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /reviewed sourceHead is not included/);
});

withFixture("verify_accepts_manually_merged_registered_cycle", (item) => {
  const { head, digest } = item.commitAndTag();
  item.writeLifecycleState({ cycle: {
    branches: [{ name: "feature-release-context-20260909", summary: "release-context", createdAt: "2026-09-09T00:00:00Z" }],
    worktrees: [], pendingRelease: null,
  } });
  const result = item.invoke("verify", "--project-root", item.root, "--expected-head", head, "--expected-sha256", digest);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).status, "released");
});

withFixture("verify_matches_recorded_release_and_ignores_later_cycle", (item) => {
  const { head, digest } = item.commitAndTag();
  item.git("branch", "feature-next-cycle", head);
  item.writeLifecycleState({
    lastRelease: {
      tag: "v1.2.3-20260909", head, defaultBranch: "main", version: "1.2.3",
      date: "20260909", releaseContextSha256: digest,
    },
    cycle: {
      branches: [{ name: "feature-next-cycle", summary: "next-cycle", createdAt: "2026-09-10T00:00:00Z" }],
      worktrees: [], pendingRelease: null,
    },
  });
  item.git("switch", "--quiet", "feature-next-cycle");
  writeFileSync(join(item.root, "next.txt"), "new cycle\n", "utf8");
  item.git("add", "next.txt");
  item.git("commit", "--quiet", "-m", "feat: next cycle");
  item.git("switch", "--quiet", "main");
  const result = item.invoke("verify", "--project-root", item.root, "--expected-head", head, "--expected-sha256", digest);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).sourceCommit, head);
});

withFixture("verify_rejects_matching_last_release_with_conflicting_context", (item) => {
  const { head, digest } = item.commitAndTag();
  item.writeLifecycleState({ lastRelease: {
    tag: "v1.2.3-20260909", head, defaultBranch: "main", version: "1.2.3",
    date: "20260909", releaseContextSha256: "f".repeat(64),
  } });
  const result = item.invoke("verify", "--project-root", item.root, "--expected-head", head, "--expected-sha256", digest);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /completed release differs/);
});

withFixture("verify_rejects_unfinished_lifecycle_journals", (item) => {
  const { head, digest } = item.commitAndTag();
  item.writeLifecycleState({ schemaVersion: 4, cycle: {
    branches: [], worktrees: [], pendingRelease: {
      tag: "v1.2.3-20260909", head, date: "20260909", version: "1.2.3", releaseContextSha256: digest,
      postReleaseAction: null,
    },
  } });
  const pendingRelease = item.invoke("verify", "--project-root", item.root);
  assert.equal(pendingRelease.status, 1);
  assert.match(pendingRelease.stderr, /unfinished publication or release journal/);
  item.writeLifecycleState({
    schemaVersion: 4,
    remote: "origin",
    pendingPublish: { head, targets: [{ remote: "origin", branch: "main", confirmed: false }] },
  });
  const pendingPublish = item.invoke("verify", "--project-root", item.root);
  assert.equal(pendingPublish.status, 1);
  assert.match(pendingPublish.stderr, /unfinished publication or release journal/);
});

withFixture("verify_rejects_missing_local_tag", (item) => {
  assert.equal(item.writeContext().status, 0);
  item.git("add", ".harness/release-context.json");
  item.git("commit", "--quiet", "-m", "chore(release): record release context");
  item.git("switch", "--quiet", "main");
  item.git("merge", "--quiet", "--no-edit", "feature-release-context-20260909");
  renameSync(item.remote, `${item.remote}.offline`);
  const result = item.invoke("verify", "--project-root", item.root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /local ref is missing or invalid/);
});

withFixture("verify_rejects_dirty_or_untracked_context_bytes", (item) => {
  item.commitAndTag();
  writeFileSync(join(item.root, ".harness/release-context.json"), "{}\n", "utf8");
  const result = item.invoke("verify", "--project-root", item.root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /fields or schemaVersion/);
});

withFixture("write_requires_default_branch_and_rejects_old_modes", (item) => {
  const missingBranch = item.invoke(
    "write", "--project-root", item.root, "--source-head", item.sourceHead,
    "--version", "1.2.3", "--release-date", "2026-09-09", "--review-selection", "disabled",
    "--scope-base", item.baseline, "--scope-diff-sha256", item.scopeHash,
  );
  assert.equal(missingBranch.status, 2);
  assert.match(missingBranch.stderr, /required/);
  for (const option of [["--remote", "origin"], ["--local-only"], ["--macos-signing-selection", "disabled"]]) {
    const result = item.writeContext(option);
    assert.equal(result.status, 2);
    assert.match(result.stderr, /unrecognized arguments/);
  }
});

withFixture("local_default_branch_rejects_ambiguous_ref_syntax", (item) => {
  for (const branch of ["main^{}", "HEAD", "@"]) {
    const rejected = item.writeContext(["--default-branch", branch]);
    assert.equal(rejected.status, 1);
    assert.match(rejected.stderr, /must name one safe Git branch/);
  }
  assert.equal(item.writeContext().status, 0);
  const path = join(item.root, ".harness/release-context.json");
  const context = JSON.parse(readFileSync(path, "utf8"));
  for (const branch of ["main^{}", "HEAD", "@"]) {
    context.defaultBranch = branch;
    writeFileSync(path, `${JSON.stringify(context, null, 2)}\n`, "utf8");
    const rejected = item.invoke("check", "--project-root", item.root);
    assert.equal(rejected.status, 1);
    assert.match(rejected.stderr, /defaultBranch is invalid/);
  }
});

withFixture("check_rejects_retired_publication_and_candidate_fields", (item) => {
  assert.equal(item.writeContext().status, 0);
  const path = join(item.root, ".harness/release-context.json");
  const context = JSON.parse(readFileSync(path, "utf8"));
  context.remote = null;
  writeFileSync(path, `${JSON.stringify(context, null, 2)}\n`, "utf8");
  const retiredRemote = item.invoke("check", "--project-root", item.root);
  assert.equal(retiredRemote.status, 1);
  assert.match(retiredRemote.stderr, /fields or schemaVersion/);
  delete context.remote;
  context.candidateSelections = {};
  writeFileSync(path, `${JSON.stringify(context, null, 2)}\n`, "utf8");
  const retiredSelections = item.invoke("check", "--project-root", item.root);
  assert.equal(retiredSelections.status, 1);
  assert.match(retiredSelections.stderr, /fields or schemaVersion/);
});

withFixture("check_rejects_invalid_utf8", (item) => {
  assert.equal(item.writeContext().status, 0);
  writeFileSync(join(item.root, ".harness/release-context.json"), Buffer.from([0xff, 0xfe]));
  const result = item.invoke("check", "--project-root", item.root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /valid UTF-8 JSON/);
});

withFixture("harness_context_uses_version_md_timestamp_and_git_tag", (item) => {
  const { head, digest } = item.commitAndTag({ version: "202609090930" });
  const verified = item.invoke(
    "verify", "--project-root", item.root,
    "--expected-version", "202609090930",
    "--expected-sha256", digest,
    "--expected-head", head,
  );
  assert.equal(verified.status, 0, verified.stderr);
  assert.equal(JSON.parse(verified.stdout).expectedTag, "v202609090930-20260909");
}, { harness: true });

withFixture("harness_write_rejects_any_version_other_than_version_md", (item) => {
  for (const version of ["1.2.3", "202609090931"]) {
    const result = item.writeContext([], version);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /must equal Version.md current YYYYMMDDHHMM version/);
  }
  assert.equal(item.git("status", "--porcelain").stdout, "");
}, { harness: true });

withFixture("harness_write_rejects_hand_edited_future_time_even_when_version_md_matches", (item) => {
  writeFileSync(join(item.root, "Version.md"), "- 当前版本：`202609100930`\n- 时间版本起始值：`202609080000`\n", "utf8");
  const result = item.writeContext([], "202609100930");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /managed current-minute version stamp|does not match the release version/u);
}, { harness: true });

withFixture("harness_check_rejects_version_md_drift", (item) => {
  const written = item.writeContext([], "202609090930");
  assert.equal(written.status, 0, written.stderr);
  writeFileSync(join(item.root, "Version.md"), "- 当前版本：`202609090931`\n", "utf8");
  const checked = item.invoke("check", "--project-root", item.root);
  assert.equal(checked.status, 1);
  assert.match(checked.stderr, /must equal Version.md current YYYYMMDDHHMM version/);
}, { harness: true });

withFixture("downstream_timestamp_cannot_bypass_harness_source_binding", (item) => {
  const result = item.writeContext([], "202609090930");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Harness timestamp release requires Version.md/);
});

withFixture("harness_write_rejects_missing_version_md", (item) => {
  rmSync(join(item.root, "Version.md"));
  const result = item.writeContext([], "202609090930");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Harness timestamp release requires Version.md/);
}, { harness: true });

withFixture("harness_write_rejects_version_already_used_by_tag", (item) => {
  item.git("tag", "v202609090930-20260909");
  const result = item.writeContext([], "202609090930");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /must be newer than tagged version 202609090930/);
}, { harness: true });

withFixture("harness_release_date_can_follow_version_clock_date", (item) => {
  const result = item.writeContext(
    ["--release-date", "2026-09-10"],
    "202609090930",
  );
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).expectedTag, "v202609090930-20260910");
  const checked = item.invoke("check", "--project-root", item.root);
  assert.equal(checked.status, 0, checked.stderr);
}, { harness: true });
