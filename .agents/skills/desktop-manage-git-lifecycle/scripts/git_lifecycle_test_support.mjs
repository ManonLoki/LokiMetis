/** Git 生命周期黑盒回归的隔离仓库夹具。 */

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const SCRIPT_DIRECTORY = dirname(fileURLToPath(import.meta.url));
export const SCRIPT = join(SCRIPT_DIRECTORY, "git_lifecycle.mjs");
export const RELEASE_CONTEXT_HELPER = resolve(
  SCRIPT_DIRECTORY,
  "../../desktop-prepare-release/scripts/release_context.mjs",
);
const HARNESS_VERSION_CLOCK_HELPER = resolve(
  SCRIPT_DIRECTORY,
  "../../desktop-prepare-release/scripts/harness_version_clock.mjs",
);
const LIFECYCLE_CORE_HELPER = join(SCRIPT_DIRECTORY, "git_lifecycle_core.mjs");

/** 以测试专用非交互环境执行命令。 */
export function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    env: options.env ?? process.env,
    encoding: "utf8",
    input: "",
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (options.check !== false && result.status !== 0) {
    assert.fail(`${command} ${args.join(" ")} failed: ${result.stderr}`);
  }
  return result;
}

/** 建立可自动回收的测试根及共享操作。 */
export function lifecycleFixture() {
  const temporary = mkdtempSync(join(tmpdir(), "git-lifecycle-"));
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: join(temporary, "global.gitconfig"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_TERMINAL_PROMPT: "0",
    GCM_INTERACTIVE: "Never",
    SSH_ASKPASS_REQUIRE: "never",
  };
  const git = (cwd, ...args) => run("git", ["-C", cwd, ...args], { env });
  const gitUnchecked = (cwd, ...args) => run("git", ["-C", cwd, ...args], { env, check: false });
  const helper = (repository, args, { success = true, cwd } = {}) => {
    const result = run(process.execPath, [SCRIPT, ...args, "--project-root", repository], {
      env,
      cwd,
      check: false,
    });
    assert.equal(result.stderr, "");
    assert.equal(result.stdout.trimEnd().split("\n").length, 1, result.stdout);
    const payload = JSON.parse(result.stdout);
    if (success) {
      assert.equal(result.status, 0, JSON.stringify(payload));
      assert.notEqual(payload.status, "error");
    } else {
      assert.notEqual(result.status, 0, JSON.stringify(payload));
      assert.equal(payload.status, "error");
    }
    return { payload, result };
  };
  const initializeRepository = ({ remote }) => {
    const repository = join(temporary, "repository");
    mkdirSync(repository);
    git(repository, "init", "--quiet", "-b", "main");
    git(repository, "config", "user.name", "Lifecycle Test");
    git(repository, "config", "user.email", "lifecycle@example.invalid");
    const helperPath = join(repository, ".agents/skills/desktop-prepare-release/scripts/release_context.mjs");
    const clockPath = join(repository, ".agents/skills/desktop-prepare-release/scripts/harness_version_clock.mjs");
    const corePath = join(repository, ".agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle_core.mjs");
    mkdirSync(dirname(helperPath), { recursive: true });
    mkdirSync(dirname(corePath), { recursive: true });
    copyFileSync(RELEASE_CONTEXT_HELPER, helperPath);
    copyFileSync(HARNESS_VERSION_CLOCK_HELPER, clockPath);
    copyFileSync(LIFECYCLE_CORE_HELPER, corePath);
    writeFileSync(join(repository, "base.txt"), "base\n", "utf8");
    git(repository, "add", "base.txt", ".agents/skills/desktop-prepare-release/scripts/release_context.mjs", ".agents/skills/desktop-prepare-release/scripts/harness_version_clock.mjs", ".agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle_core.mjs");
    git(repository, "commit", "--quiet", "-m", "initial");
    if (!remote) return { repository, bare: null };
    const bare = join(temporary, "remote.git");
    mkdirSync(bare);
    git(bare, "init", "--quiet", "--bare");
    git(bare, "symbolic-ref", "HEAD", "refs/heads/main");
    git(repository, "remote", "add", "origin", bare);
    git(repository, "push", "--quiet", "-u", "origin", "main");
    return { repository, bare };
  };
  const addBareRemote = (repository, name, defaultBranch) => {
    const bare = join(temporary, `${name}.git`);
    mkdirSync(bare);
    git(bare, "init", "--quiet", "--bare");
    git(bare, "symbolic-ref", "HEAD", `refs/heads/${defaultBranch}`);
    git(repository, "remote", "add", name, bare);
    git(repository, "push", "--quiet", name, `refs/heads/main:refs/heads/${defaultBranch}`);
    return bare;
  };
  const commitFile = (repository, name, content) => {
    writeFileSync(join(repository, name), content, "utf8");
    git(repository, "add", name);
    git(repository, "commit", "--quiet", "-m", `add ${name}`);
    return git(repository, "rev-parse", "HEAD").stdout.trim();
  };
  const state = (repository) => {
    const common = git(repository, "rev-parse", "--path-format=absolute", "--git-common-dir").stdout.trim();
    return JSON.parse(readFileSync(join(common, "agent-first-harness/git-lifecycle.json"), "utf8"));
  };
  const prepareReleaseContext = (
    repository,
    { version, date, defaultBranch = "main", summary = "发布上下文测试", reviewSelection = "disabled" },
  ) => {
    const sourceHead = git(repository, "rev-parse", "HEAD").stdout.trim();
    const scopeBase = git(repository, "rev-list", "--max-parents=0", "HEAD").stdout.trim();
    const scopeDiff = spawnSync("git", ["-C", repository, "-c", "core.quotePath=true", "diff",
      "--binary", "--full-index", "--no-renames", "--no-ext-diff", "--no-textconv", "--no-color",
      "--no-relative", "--src-prefix=a/", "--dst-prefix=b/", "--diff-algorithm=myers",
      "--no-indent-heuristic", "--unified=3", "--inter-hunk-context=0", "--submodule=short",
      "--ignore-submodules=none", scopeBase, sourceHead, "--"], { env, encoding: null });
    assert.equal(scopeDiff.status, 0, scopeDiff.stderr?.toString("utf8"));
    const scopeDiffSha256 = createHash("sha256").update(scopeDiff.stdout).digest("hex");
    writeFileSync(join(repository, "release-notes.json"), `${JSON.stringify({ schemaVersion: 2, releases: [{
      releaseDate: `${date.slice(0, 4)}-${date.slice(4, 6)}-${date.slice(6)}`, version: `v${version}`,
      featureOptimizations: [{ "zh-CN": "测试发布。", "en-US": "Test release." }], bugFixes: [],
    }] }, null, 2)}\n`, "utf8");
    const command = [
      join(repository, ".agents/skills/desktop-prepare-release/scripts/release_context.mjs"),
      "write", "--project-root", repository,
      "--source-head", sourceHead,
      "--version", version,
      "--release-date", `${date.slice(0, 4)}-${date.slice(4, 6)}-${date.slice(6)}`,
      "--default-branch", defaultBranch,
      "--review-selection", reviewSelection,
      "--scope-base", scopeBase,
      "--scope-diff-sha256", scopeDiffSha256,
      ...(reviewSelection === "enabled"
        ? ["--review-evidence-summary", summary]
        : ["--review-reason", summary, "--review-remaining-risk", "测试不执行正式发布语义审查"]),
    ];
    const written = run(process.execPath, command, { env, check: false });
    assert.equal(written.status, 0, written.stderr);
    const payload = JSON.parse(written.stdout);
    git(repository, "add", ".harness/release-context.json", "release-notes.json");
    git(repository, "commit", "--quiet", "-m", "chore(release): bind release context");
    return {
      digest: payload.releaseContextSha256,
      head: git(repository, "rev-parse", "HEAD").stdout.trim(),
    };
  };
  const localBranchExists = (repository, branch) =>
    gitUnchecked(repository, "show-ref", "--verify", "--quiet", `refs/heads/${branch}`).status === 0;
  const remoteBranchExists = (repository, remote, branch) =>
    git(repository, "ls-remote", "--heads", remote, `refs/heads/${branch}`).stdout.trim() !== "";
  const installHook = (bare, body) => {
    const hook = join(bare, "hooks/pre-receive");
    writeFileSync(hook, `#!/bin/sh\nset -eu\n${body}`, "utf8");
    chmodSync(hook, 0o755);
  };
  const cleanup = () => rmSync(temporary, { recursive: true, force: true });
  return {
    temporary, env, git, gitUnchecked, helper, initializeRepository, addBareRemote,
    commitFile, state, prepareReleaseContext, localBranchExists, remoteBranchExists,
    installHook, cleanup,
  };
}

/** 异步启动一个 lifecycle CLI，供锁并发测试。 */
export function spawnHelper(repository, args, env) {
  return spawn(process.execPath, [SCRIPT, ...args, "--project-root", repository], {
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

/** 收集异步子进程并返回输出。 */
export function collectProcess(child) {
  return new Promise((accept, reject) => {
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.once("error", reject);
    child.once("close", (status) => accept({ status, stdout, stderr }));
  });
}
