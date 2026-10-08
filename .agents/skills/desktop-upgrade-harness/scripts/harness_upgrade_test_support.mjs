/** Harness 升级测试的隔离 Git 夹具与共享断言。 */

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { REQUIRED_MANAGED_SOURCE_PATHS } from "./harness_upgrade_policy.mjs";

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
export const SCRIPT = path.join(SCRIPT_DIRECTORY, "harness_upgrade.mjs");
export const PRODUCTION_OWNERSHIP = path.join(SCRIPT_DIRECTORY, "..", "references", "ownership-manifest.json");
export const MANAGED = ".agents/skills/desktop-define-product/SKILL.md";
export const MANAGED_SECOND = ".agents/skills/desktop-plan-change/SKILL.md";
export const MANAGED_SELF = ".agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.mjs";
export const CORE_FIRST_CHECKER = ".agents/skills/desktop-implement-change/scripts/check_core_first.mjs";
export const REQUIRED_MANAGED_CHECKERS = REQUIRED_MANAGED_SOURCE_PATHS;
export const MIXED = "AGENTS.md";
export const PROTECTED = "docs/AGENT_POLICY.md";
export const TOMBSTONE = "Version.md";

function runGit(root, ...args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result;
}

export class HarnessUpgradeFixture {
  constructor(t) {
    this.root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "harness_upgrade_")));
    t.after(() => fs.rmSync(this.root, { recursive: true, force: true }));
    this.source = path.join(this.root, "source");
    this.candidate = path.join(this.root, "candidate");
    this.target = path.join(this.root, "target");
    fs.mkdirSync(this.source); fs.mkdirSync(this.candidate); fs.mkdirSync(this.target);
    for (const repository of [this.source, this.target]) runGit(repository, "init", "--initial-branch=main");
    this.sourceVersion = "202607310001";
    this.write(this.source, "Version.md", `# Harness 版本\n\n- 当前版本：\`${this.sourceVersion}\`\n`);
    runGit(this.source, "add", "Version.md");
    for (const repository of [this.source, this.target]) runGit(repository, "-c", "user.name=Harness Fixture", "-c", "user.email=harness-fixture@example.invalid", "commit", "--allow-empty", "-m", "fixture baseline");
    this.sourceCommit = runGit(this.source, "rev-parse", "HEAD").stdout.trim();
    this.ownership = path.join(this.target, ".agents", "skills", "desktop-upgrade-harness", "references", "ownership-manifest.json");
    const candidateOwnership = path.join(this.candidate, path.relative(this.target, this.ownership));
    fs.mkdirSync(path.dirname(this.ownership), { recursive: true }); fs.mkdirSync(path.dirname(candidateOwnership), { recursive: true });
    fs.copyFileSync(PRODUCTION_OWNERSHIP, this.ownership); fs.copyFileSync(PRODUCTION_OWNERSHIP, candidateOwnership);
    this.lock = path.join(this.target, ".harness", "upstream-lock.json");
    this.planCounter = 0;
  }

  write(root, relative, content) {
    const file = path.join(root, ...relative.split("/"));
    fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, content);
  }

  runTool(args, expected = 0) {
    const result = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8" });
    assert.equal(result.status, expected, result.stderr || result.stdout);
    return JSON.parse(result.stdout.trim() ? result.stdout : result.stderr);
  }

  sharedArguments(lock = this.lock) {
    return ["--source-root", this.source, "--source-version", this.sourceVersion, "--source-commit", this.sourceCommit, "--candidate-root", this.candidate, "--target-root", this.target, "--ownership", this.ownership, "--lock", lock];
  }

  createPlan(expected = 0, name = "plan") {
    this.planCounter += 1;
    const planPath = path.join(this.root, `${name}-${this.planCounter}.json`);
    return { plan: this.runTool(["plan", ...this.sharedArguments(), "--output", planPath], expected), planPath };
  }

  bootstrap() {
    const { planPath } = this.createPlan(0, "bootstrap");
    this.runTool(["record", "--plan", planPath, "--source-version", this.sourceVersion, "--source-commit", this.sourceCommit, "--bootstrap", "--approval", "bootstrap-verified-baseline"]);
  }

  record(planPath, resolvedManual = []) {
    const args = ["record", "--plan", planPath, "--source-version", this.sourceVersion, "--source-commit", this.sourceCommit, "--approval", "record-verified-baseline"];
    for (const relative of resolvedManual) args.push("--resolved-manual", relative);
    return this.runTool(args);
  }

  plan(expected = 0) { return this.runTool(["plan", ...this.sharedArguments()], expected); }

  classification(plan, relative) {
    const matches = plan.actions.filter((item) => item.path === relative);
    assert.equal(matches.length, 1);
    return matches[0].classification;
  }

  git(root, ...args) { return runGit(root, ...args); }
}

/** 创建测试符号链接；Windows 未授予该权限时跳过用例，其余错误照常失败。 */
export function symlinkOrSkip(context, target, linkPath, type) {
  try {
    fs.symlinkSync(target, linkPath, type);
    return true;
  } catch (error) {
    if (process.platform === "win32" && error.code === "EPERM") {
      context.skip("当前 Windows 主机未授予创建符号链接的权限");
      return false;
    }
    throw error;
  }
}
