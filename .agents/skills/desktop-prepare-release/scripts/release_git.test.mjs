/** 验证精确范围发布提交，不施加分支形态策略。 */

import test from "node:test";
import assert from "node:assert/strict";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "release_git.mjs");

/** 运行测试命令并按需断言成功。 */
function run(command, args, { env = process.env, check = true } = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", env, input: "", maxBuffer: 32 * 1024 * 1024 });
  if (result.error) throw result.error;
  if (check && result.status !== 0) assert.fail(`${command} ${args.join(" ")} failed: ${result.stderr}`);
  return result;
}

/** 建立隔离仓库并封装 inspect/commit 操作。 */
function fixture() {
  const temporary = mkdtempSync(join(tmpdir(), "release-git-"));
  const root = join(temporary, "project");
  mkdirSync(root);
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: join(temporary, "global.gitconfig"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_TERMINAL_PROMPT: "0",
    GCM_INTERACTIVE: "Never",
  };
  const git = (...args) => run("git", ["-C", root, ...args], { env });
  git("init", "--quiet", "--initial-branch=main");
  git("config", "--local", "user.name", "Release Test");
  git("config", "--local", "user.email", "release-test@example.com");
  writeFileSync(join(root, "README.md"), "baseline\n", "utf8");
  git("add", "README.md");
  git("commit", "--quiet", "-m", "chore: baseline");
  git("switch", "--quiet", "-c", "feature-release-scope-20260909");
  const invoke = (...args) => run(process.execPath, [SCRIPT, ...args], { env, check: false });
  const inspect = () => {
    const result = invoke("inspect", "--project-root", root);
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  const commit = (snapshot, ...paths) => {
    const args = [
      "commit", "--project-root", root,
      "--expected-status-sha256", snapshot.statusSha256,
      "--message", "feat: complete reviewed release scope",
    ];
    for (const path of paths) args.push("--path", path);
    return invoke(...args);
  };
  return { temporary, root, env, git, inspect, commit, cleanup: () => rmSync(temporary, { recursive: true, force: true }) };
}

test("inspect_reports_exact_head_branch_and_dirty_snapshot", () => {
  const item = fixture();
  try {
    writeFileSync(join(item.root, "source.txt"), "done\n", "utf8");
    const payload = item.inspect();
    assert.equal(payload.status, "dirty");
    assert.equal(payload.branch, "feature-release-scope-20260909");
    assert.match(payload.head, /^[0-9a-f]{40}$/);
    assert.match(payload.statusSha256, /^[0-9a-f]{64}$/);
    assert.ok(payload.records.some((record) => record.includes("source.txt")));
  } finally { item.cleanup(); }
});

test("commit_stages_only_reviewed_paths_and_finishes_clean", () => {
  const item = fixture();
  try {
    writeFileSync(join(item.root, "source.txt"), "done\n", "utf8");
    const result = item.commit(item.inspect(), "source.txt");
    assert.equal(result.status, 0, result.stderr);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.clean, true);
    assert.notEqual(payload.previousHead, payload.head);
    assert.equal(item.git("status", "--porcelain").stdout, "");
  } finally { item.cleanup(); }
});

test("commit_allows_any_named_branch_without_protection_rules", () => {
  const item = fixture();
  try {
    for (const branch of ["main", "master", "topic/custom", "maintenance"]) {
      item.git("switch", "--quiet", "-C", branch);
      const name = `${branch.replaceAll("/", "-")}.txt`;
      writeFileSync(join(item.root, name), `${branch}\n`, "utf8");
      const result = item.commit(item.inspect(), name);
      assert.equal(result.status, 0, result.stderr);
    }
  } finally { item.cleanup(); }
});

test("only_known_harness_metadata_paths_are_approvable", () => {
  const item = fixture();
  try {
    mkdirSync(join(item.root, ".harness"));
    for (const name of ["release-context.json", "upstream-lock.json"]) {
      writeFileSync(join(item.root, ".harness", name), "{}\n", "utf8");
      const result = item.commit(item.inspect(), `.harness/${name}`);
      assert.equal(result.status, 0, result.stderr);
    }
    writeFileSync(join(item.root, "source.txt"), "next\n", "utf8");
    const rejected = item.commit(item.inspect(), ".harness/other.json");
    assert.equal(rejected.status, 1);
    assert.match(rejected.stderr, /cannot be approved/);
  } finally { item.cleanup(); }
});

test("tracked_version_state_is_approvable_harness_metadata", () => {
  const item = fixture();
  try {
    mkdirSync(join(item.root, ".harness"));
    const statePath = join(item.root, ".harness", "version-state.json");
    writeFileSync(statePath, '{"cycle":1}\n', "utf8");
    item.git("add", ".harness/version-state.json");
    item.git("commit", "--quiet", "-m", "test: track version state");
    writeFileSync(statePath, '{"cycle":2}\n', "utf8");

    const result = item.commit(item.inspect(), ".harness/version-state.json");
    assert.equal(result.status, 0, result.stderr);
    assert.equal(item.git("show", "HEAD:.harness/version-state.json").stdout, '{"cycle":2}\n');
    assert.equal(item.git("status", "--porcelain").stdout, "");
  } finally { item.cleanup(); }
});

test("changed_snapshot_and_branch_switch_are_rejected_before_staging", () => {
  const item = fixture();
  try {
    const target = join(item.root, "source.txt");
    writeFileSync(target, "one\n", "utf8");
    const snapshot = item.inspect();
    writeFileSync(target, "two\n", "utf8");
    let result = item.commit(snapshot, "source.txt");
    assert.equal(result.status, 1);
    assert.match(result.stderr, /changed after review/);
    assert.equal(item.git("diff", "--cached", "--name-only").stdout, "");
    const branchSnapshot = item.inspect();
    item.git("switch", "--quiet", "-c", "another-branch");
    result = item.commit(branchSnapshot, "source.txt");
    assert.equal(result.status, 1);
    assert.match(result.stderr, /changed after review/);
  } finally { item.cleanup(); }
});

test("git_add_window_race_cannot_commit_unreviewed_bytes", { skip: process.platform === "win32" }, () => {
  const item = fixture();
  try {
    const target = join(item.root, "source.txt");
    writeFileSync(target, "reviewed\n", "utf8");
    const snapshot = item.inspect();
    const realGit = run("sh", ["-c", "command -v git"]).stdout.trim();
    const wrapperDirectory = join(item.temporary, "git-wrapper");
    mkdirSync(wrapperDirectory);
    const wrapper = join(wrapperDirectory, "git");
    writeFileSync(wrapper, [
      "#!/bin/sh",
      'if [ -z "${GIT_INDEX_FILE:-}" ] && [ "${1:-}" = -C ] && [ "${3:-}" = add ]; then',
      "  printf '%s\\n' raced > \"$AFH_RACE_TARGET\"",
      "fi",
      'exec "$AFH_REAL_GIT" "$@"',
      "",
    ].join("\n"), "utf8");
    chmodSync(wrapper, 0o755);
    item.env.AFH_RACE_TARGET = target;
    item.env.AFH_REAL_GIT = realGit;
    item.env.PATH = `${wrapperDirectory}${delimiter}${item.env.PATH ?? ""}`;
    const result = item.commit(snapshot, "source.txt");
    assert.equal(result.status, 1);
    assert.match(result.stderr, /staged content changed after review/);
    assert.equal(readFileSync(target, "utf8"), "raced\n");
  } finally { item.cleanup(); }
});

test("frozen_patch_covers_tracked_untracked_mode_delete_and_rename", { skip: process.platform === "win32" }, () => {
  const item = fixture();
  try {
    for (const name of ["mode.txt", "deleted.txt", "renamed-from.txt"]) {
      writeFileSync(join(item.root, name), `${name}\n`, "utf8");
    }
    item.git("add", "mode.txt", "deleted.txt", "renamed-from.txt");
    item.git("commit", "--quiet", "-m", "test: add patch fixtures");
    writeFileSync(join(item.root, "README.md"), "tracked update\n", "utf8");
    writeFileSync(join(item.root, "untracked.txt"), "untracked\n", "utf8");
    chmodSync(join(item.root, "mode.txt"), 0o755);
    item.git("update-index", "--chmod=+x", "mode.txt");
    unlinkSync(join(item.root, "deleted.txt"));
    renameSync(join(item.root, "renamed-from.txt"), join(item.root, "renamed-to.txt"));
    const result = item.commit(
      item.inspect(), "README.md", "untracked.txt", "mode.txt", "deleted.txt", "renamed-from.txt", "renamed-to.txt",
    );
    assert.equal(result.status, 0, result.stderr);
    const summary = item.git("diff", "--summary", "HEAD^", "HEAD").stdout;
    assert.match(summary, /create mode 100644 untracked\.txt/);
    assert.match(summary, /mode change 100644 => 100755 mode\.txt/);
    assert.match(summary, /delete mode 100644 deleted\.txt/);
    assert.match(summary, /rename renamed-from\.txt => renamed-to\.txt \(100%\)/);
  } finally { item.cleanup(); }
});

test("partial_or_preexisting_unreviewed_scope_is_rejected", () => {
  const item = fixture();
  try {
    writeFileSync(join(item.root, "source.txt"), "done\n", "utf8");
    writeFileSync(join(item.root, "other.txt"), "must review\n", "utf8");
    let result = item.commit(item.inspect(), "source.txt");
    assert.equal(result.status, 1);
    assert.match(result.stderr, /complete working tree/);
    item.git("reset");
    item.git("add", "other.txt");
    result = item.commit(item.inspect(), "source.txt");
    assert.equal(result.status, 1);
    assert.match(result.stderr, /outside the reviewed scope/);
  } finally { item.cleanup(); }
});

test("hooks_cannot_fail_or_smuggle_unreviewed_content", { skip: process.platform === "win32" }, () => {
  for (const [body, expected] of [
    ["#!/bin/sh\nexit 17\n", /hooks were not bypassed/],
    ["#!/bin/sh\nprintf '%s\\n' injected > injected.txt\ngit add injected.txt\n", /hook changed the reviewed staged content/],
  ]) {
    const item = fixture();
    try {
      writeFileSync(join(item.root, "source.txt"), "done\n", "utf8");
      const hook = join(item.root, ".git/hooks/pre-commit");
      writeFileSync(hook, body, "utf8");
      chmodSync(hook, 0o755);
      const result = item.commit(item.inspect(), "source.txt");
      assert.equal(result.status, 1);
      assert.match(result.stderr, expected);
    } finally { item.cleanup(); }
  }
});

test("high_confidence_secret_stops_without_advancing_head", () => {
  const item = fixture();
  try {
    const secret = "sk-abcdefghijklmnopqrstuvwxyz123456";
    writeFileSync(join(item.root, "secret.env"), `API_TOKEN=${secret}\n`, "utf8");
    const before = item.git("rev-parse", "HEAD").stdout.trim();
    const result = item.commit(item.inspect(), "secret.env");
    assert.equal(result.status, 1);
    assert.match(result.stderr, /potential secret detected/);
    assert.ok(!result.stderr.includes(secret));
    assert.equal(item.git("rev-parse", "HEAD").stdout.trim(), before);
  } finally { item.cleanup(); }
});

test("unsafe_or_empty_commit_scope_is_rejected", () => {
  const item = fixture();
  try {
    writeFileSync(join(item.root, "source.txt"), "done\n", "utf8");
    const snapshot = item.inspect();
    for (const unsafe of ["../source.txt", ".git/config", ".harness", ".harness/other.json", "release/candidate.zip", ":(glob)*"]) {
      assert.equal(item.commit(snapshot, unsafe).status, 1, unsafe);
    }
  } finally { item.cleanup(); }
});
