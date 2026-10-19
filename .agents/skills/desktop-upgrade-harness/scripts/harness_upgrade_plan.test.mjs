#!/usr/bin/env node
/** Harness 升级计划、所有权和 bootstrap 回归。 */

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { HarnessUpgradeFixture, MANAGED, MIXED, PROTECTED, REQUIRED_MANAGED_CHECKERS, TOMBSTONE, symlinkOrSkip } from "./harness_upgrade_test_support.mjs";
import { loadOwnership, matchesPattern, scanTree } from "./harness_upgrade_ownership.mjs";
import { REQUIRED_MANAGED_SOURCE_PATHS } from "./harness_upgrade_policy.mjs";
import { safeRelativePath, snapshotFile } from "./harness_upgrade_safety.mjs";

/** 保留入口的清单在下游独立校验，不依赖 Harness 源专用目录。 */
test("required_managed_paths_are_unique_and_canonical", () => {
  assert.equal(new Set(REQUIRED_MANAGED_SOURCE_PATHS).size, REQUIRED_MANAGED_SOURCE_PATHS.length);
  for (const relative of REQUIRED_MANAGED_SOURCE_PATHS) assert.equal(safeRelativePath(relative), relative);
});

test("ownership_glob_preserves_fnmatch_character_classes", () => {
  assert.equal(matchesPattern("docs/a.txt", "docs/[ab].txt"), true);
  assert.equal(matchesPattern("docs/c.txt", "docs/[ab].txt"), false);
  assert.equal(matchesPattern("docs/c.txt", "docs/[!ab].txt"), true);
  assert.equal(matchesPattern("docs/a.txt", "docs/[!ab].txt"), false);
  assert.equal(matchesPattern("docs/a.txt", "docs/[z-a].txt"), false);
});

test("first bootstrap identifies identical managed overlap as converged", (t) => {
  const f = new HarnessUpgradeFixture(t);
  f.write(f.candidate, MANAGED, "same bytes");
  f.write(f.target, MANAGED, "same bytes");
  const { plan, planPath } = f.createPlan();
  assert.equal(f.classification(plan, MANAGED), "converged");
  assert.equal(plan.blocked, false);
  f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--bootstrap", "--approval", "bootstrap-verified-baseline"]);
});

test("target scan prunes ignored generated files unless the plan requires them", (t) => {
  const f = new HarnessUpgradeFixture(t);
  f.write(f.target, ".gitignore", "node_modules/\n");
  f.write(f.target, "node_modules/pkg/needed.txt", "tracked by plan");
  assert.equal(Object.hasOwn(scanTree(f.target, { targetTree: true }).files, "node_modules/pkg/needed.txt"), false);
  assert.equal(Object.hasOwn(scanTree(f.target, { targetTree: true, retainPaths: ["node_modules/pkg/needed.txt"] }).files, "node_modules/pkg/needed.txt"), true);
});

test("source-only checker is rejected from candidate and retired from an old target", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const checker = ".agents/skills/desktop-implement-change/scripts/check_no_python.mjs";
  f.write(f.source, checker, "source-only checker");
  f.git(f.source, "add", checker);
  f.git(f.source, "-c", "user.name=Harness Fixture", "-c", "user.email=harness-fixture@example.invalid", "commit", "-m", "add source checker");
  f.sourceCommit = f.git(f.source, "rev-parse", "HEAD").stdout.trim();
  f.write(f.candidate, checker, "source-only checker");
  assert.ok(f.plan(2).problems.some((problem) => problem.includes("候选不得传播上游专用检查器")));
  fs.rmSync(path.join(f.candidate, checker));
  f.write(f.target, ["scripts/tool", "py"].join("."), "print('project script')\n");
  assert.equal(f.plan().blocked, false, "下游产品脚本不受上游检查器限制");
  f.bootstrap();
  f.write(f.target, checker, "old checker");
  const lock = JSON.parse(fs.readFileSync(f.lock, "utf8"));
  const oldSnapshot = snapshotFile(path.join(f.target, checker));
  lock.entries[checker] = { mode: "managed", candidate: oldSnapshot, target: oldSnapshot };
  fs.writeFileSync(f.lock, `${JSON.stringify(lock)}\n`);
  assert.ok(f.plan(2).problems.some((problem) => problem.includes("目标须先移除旧版上游专用检查器")));
  fs.rmSync(path.join(f.target, checker));
  const { plan, planPath } = f.createPlan();
  assert.equal(f.classification(plan, checker), "converged");
  f.record(planPath);
  assert.equal(Object.hasOwn(JSON.parse(fs.readFileSync(f.lock, "utf8")).entries, checker), false);
});

test("upstream_only_change_can_apply_and_record", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); f.write(f.candidate, MANAGED, "v2");
  const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MANAGED), "update");
  const applied = f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED]); assert.deepEqual(applied.applied, [MANAGED]); assert.equal(fs.readFileSync(path.join(f.target, MANAGED), "utf8"), "v2");
  f.record(f.createPlan(0, "converged").planPath);
});

test("local_change_remains_protected_across_later_upstream_change", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); f.write(f.target, MANAGED, "local");
  const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MANAGED), "preserve_local"); f.record(planPath); f.write(f.candidate, MANAGED, "v2"); assert.equal(f.classification(f.plan(2), MANAGED), "conflict");
});

test("local_delete_remains_protected_across_later_upstream_change", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); fs.unlinkSync(path.join(f.target, MANAGED));
  const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MANAGED), "preserve_local"); f.record(planPath); f.write(f.candidate, MANAGED, "v2"); assert.equal(f.classification(f.plan(2), MANAGED), "conflict");
});

test("both_sides_change_blocks", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); f.write(f.candidate, MANAGED, "upstream"); f.write(f.target, MANAGED, "local"); assert.equal(f.classification(f.plan(2), MANAGED), "conflict");
});

test("delete_requires_manual_resolution_before_record", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); fs.unlinkSync(path.join(f.candidate, MANAGED));
  const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MANAGED), "delete");
  assert.equal(f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED], 2).ok, false); assert.equal(fs.existsSync(path.join(f.target, MANAGED)), true);
  f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--approval", "record-verified-baseline"], 2);
  fs.unlinkSync(path.join(f.target, MANAGED)); f.record(f.createPlan(0, "delete-resolved").planPath);
});

test("new_managed_path_requires_manual_convergence", (t) => {
  const f = new HarnessUpgradeFixture(t); f.bootstrap(); f.write(f.candidate, MANAGED, "new"); assert.equal(f.classification(f.plan(), MANAGED), "add"); f.write(f.target, MANAGED, "new"); const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MANAGED), "converged"); f.record(planPath);
});

test("source_required_checkers_must_enter_rendered_candidate", (t) => {
  const f = new HarnessUpgradeFixture(t); f.bootstrap();
  for (const relative of REQUIRED_MANAGED_CHECKERS) f.write(f.source, relative, `source:${relative}`);
  f.git(f.source, "add", ...REQUIRED_MANAGED_CHECKERS); f.git(f.source, "-c", "user.name=Harness Fixture", "-c", "user.email=harness-fixture@example.invalid", "commit", "-m", "add required checker"); f.sourceCommit = f.git(f.source, "rev-parse", "HEAD").stdout.trim();
  const missing = f.plan(2);
  for (const relative of REQUIRED_MANAGED_CHECKERS) { assert.ok(missing.problems.some((item) => item.includes(relative) && item.includes("候选缺少源 Harness 必需 managed 路径"))); f.write(f.candidate, relative, `stale:${relative}`); }
  const stale = f.plan(2);
  for (const relative of REQUIRED_MANAGED_CHECKERS) { assert.ok(stale.problems.some((item) => item.includes(relative) && item.includes("必需 managed 路径内容不匹配"))); f.write(f.candidate, relative, `source:${relative}`); }
  const present = f.plan(); for (const relative of REQUIRED_MANAGED_CHECKERS) assert.equal(f.classification(present, relative), "add");
  assert.equal(present.actions.some((item) => item.path === ".harness/release-context.json"), false);
});

test("release_context_is_a_protected_target_fact", (t) => {
  const f = new HarnessUpgradeFixture(t); const releaseContext = ".harness/release-context.json"; f.bootstrap(); f.write(f.target, releaseContext, '{"release": "target"}\n');
  assert.equal(f.plan().actions.some((item) => item.path === releaseContext), false); assert.equal(fs.readFileSync(path.join(f.target, releaseContext), "utf8"), '{"release": "target"}\n');
  f.write(f.candidate, releaseContext, '{"release": "source"}\n'); assert.equal(f.classification(f.plan(2), releaseContext), "protected_candidate");
});

/** 项目本地第三方 Skill 不进入来源锁，候选试图覆盖时阻断且保留目标字节。 */
test("project_design_skill_is_preserved_and_excluded_from_upgrade_baseline", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const designSkill = ".agents/skills/design-taste-frontend/SKILL.md";
  f.write(f.target, designSkill, "project design notes\n");
  f.bootstrap();
  assert.equal(Object.hasOwn(JSON.parse(fs.readFileSync(f.lock, "utf8")).entries, designSkill), false);
  assert.equal(f.plan().actions.some((item) => item.path === designSkill), false);
  f.write(f.candidate, designSkill, "upstream replacement\n");
  assert.equal(f.classification(f.plan(2), designSkill), "protected_candidate");
  assert.equal(fs.readFileSync(path.join(f.target, designSkill), "utf8"), "project design notes\n");
});

/** 自定义通配或内部路径不能在 protected 条目前遮蔽设计树；移到其后才合法。 */
test("earlier_custom_ownership_rules_cannot_shadow_any_part_of_design_skill_tree", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const original = JSON.parse(fs.readFileSync(f.ownership, "utf8"));
  for (const pattern of [".agents/skills/design-*/**", ".agents/skills/design-taste-frontend/private/**", ".agents/skills/design-taste-frontend/SKILL.md"]) {
    const rule = { pattern, mode: "managed" };
    fs.writeFileSync(f.ownership, JSON.stringify({ ...original, rules: [rule, ...original.rules] }));
    assert.throws(() => loadOwnership(f.ownership), /protected 规则被更早/u, pattern);
    fs.writeFileSync(f.ownership, JSON.stringify({ ...original, rules: [...original.rules, rule] }));
    assert.doesNotThrow(() => loadOwnership(f.ownership));
  }
});

test("post_release_choice_stays_protected_while_switch_skill_is_required_managed_input", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const switchSkill = ".agents/skills/desktop-switch-post-release-action/SKILL.md";
  assert.ok(REQUIRED_MANAGED_CHECKERS.includes(switchSkill));
  f.bootstrap();
  f.write(f.target, PROTECTED, "schema_version: 3\n");
  f.write(f.source, switchSkill, "managed switch skill\n");
  f.git(f.source, "add", switchSkill);
  f.git(f.source, "-c", "user.name=Harness Fixture", "-c", "user.email=harness-fixture@example.invalid", "commit", "-m", "add switch skill");
  f.sourceCommit = f.git(f.source, "rev-parse", "HEAD").stdout.trim();
  const missing = f.plan(2);
  assert.ok(missing.problems.some((item) => item.includes(switchSkill) && item.includes("候选缺少")));
  f.write(f.candidate, switchSkill, "managed switch skill\n");
  const ready = f.plan();
  assert.equal(f.classification(ready, switchSkill), "add");
  assert.equal(ready.actions.some((item) => item.path === PROTECTED), false);
  assert.equal(fs.readFileSync(path.join(f.target, PROTECTED), "utf8"), "schema_version: 3\n");
});

/** 关闭工作树偏好的既有下游也接收完整技能，策略字节与权限保持。 */
test("disabled_task_worktrees_still_receive_skill_without_policy_mutation", (t) => {
  const f = new HarnessUpgradeFixture(t);
  f.bootstrap();
  const policy = "---\nschema_version: 5\ntask_worktrees: disabled\n---\n原用户确认及正文\n";
  f.write(f.target, PROTECTED, policy);
  const file = path.join(f.target, PROTECTED);
  const stat = fs.statSync(file);
  const paths = [".agents/skills/desktop-manage-task-worktrees/SKILL.md", ".agents/skills/desktop-manage-task-worktrees/agents/openai.yaml"];
  for (const relative of paths) {
    assert.ok(REQUIRED_MANAGED_SOURCE_PATHS.includes(relative));
    f.write(f.source, relative, `完整工程文件:${relative}\n`);
    f.write(f.candidate, relative, `完整工程文件:${relative}\n`);
  }
  f.git(f.source, "add", ...paths);
  f.git(f.source, "-c", "user.name=Harness Fixture", "-c", "user.email=harness-fixture@example.invalid", "commit", "-m", "新增独立工作树技能");
  f.sourceCommit = f.git(f.source, "rev-parse", "HEAD").stdout.trim();
  const { plan } = f.createPlan();
  for (const relative of paths) assert.equal(f.classification(plan, relative), "add");
  assert.equal(plan.actions.some((item) => item.path === PROTECTED), false);
  // add 沿既有流程逐项复核后复制；apply 只处理已存在文件的 update。
  for (const relative of paths) f.write(f.target, relative, fs.readFileSync(path.join(f.candidate, relative)));
  const converged = f.createPlan();
  for (const relative of paths) assert.equal(f.classification(converged.plan, relative), "converged");
  f.record(converged.planPath);
  for (const relative of paths) assert.equal(fs.readFileSync(path.join(f.target, relative), "utf8"), `完整工程文件:${relative}\n`);
  assert.equal(fs.readFileSync(file, "utf8"), policy);
  assert.equal(fs.statSync(file).mtimeMs, stat.mtimeMs);
  assert.equal(fs.statSync(file).mode, stat.mode);
});

test("legacy_managed_omission_rejects_stale_candidate_and_overlapping_protection", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const switchRule = ".agents/skills/desktop-switch-post-release-action/**";
  const old = JSON.parse(fs.readFileSync(f.ownership, "utf8"));
  old.rules = old.rules.filter((item) => item.pattern !== switchRule);
  fs.writeFileSync(f.ownership, `${JSON.stringify(old)}\n`);
  const candidateOwnership = path.join(f.candidate, ".agents/skills/desktop-upgrade-harness/references/ownership-manifest.json");
  fs.writeFileSync(candidateOwnership, `${JSON.stringify(old)}\n`);
  assert.ok(f.plan(2).problems.some((problem) => problem.includes("候选所有权 manifest 无效")));
  fs.copyFileSync(path.join(import.meta.dirname, "..", "references", "ownership-manifest.json"), candidateOwnership);
  old.rules.splice(old.rules.findIndex((item) => item.pattern === ".agents/skills/**"), 0,
    { pattern: ".agents/skills/desktop-switch-post-release-action/private/**", mode: "protected" });
  fs.writeFileSync(f.ownership, `${JSON.stringify(old)}\n`);
  assert.match(f.runTool(["plan", ...f.sharedArguments()], 2).error, /削弱了必需保护/);
  old.rules.find((item) => item.pattern.endsWith("/private/**")).pattern = ".agents/skills/desktop-switch-post-release-actio?/private/**";
  fs.writeFileSync(f.ownership, `${JSON.stringify(old)}\n`);
  assert.match(f.runTool(["plan", ...f.sharedArguments()], 2).error, /削弱了必需保护/);
});

test("new_path_collision_blocks", (t) => {
  const f = new HarnessUpgradeFixture(t); f.bootstrap(); f.write(f.candidate, MANAGED, "upstream"); f.write(f.target, MANAGED, "local"); assert.equal(f.classification(f.plan(2), MANAGED), "collision");
});

test("protected_candidate_blocks_raw_source_tree", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, PROTECTED, "upstream"); assert.equal(f.classification(f.plan(2), PROTECTED), "protected_candidate");
});

test("tombstone_in_candidate_or_target_blocks", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, TOMBSTONE, "source"); assert.equal(f.classification(f.plan(2), TOMBSTONE), "tombstone_candidate"); fs.unlinkSync(path.join(f.candidate, TOMBSTONE)); f.write(f.target, TOMBSTONE, "target"); assert.equal(f.classification(f.plan(2), TOMBSTONE), "tombstone_present");
});

test("target_tombstone_symlink_and_empty_directory_block", (t) => {
  const f = new HarnessUpgradeFixture(t); const outside = path.join(f.root, "outside-version.txt"); fs.writeFileSync(outside, "outside"); if (!symlinkOrSkip(t, outside, path.join(f.target, TOMBSTONE))) return; let plan = f.plan(2); assert.ok(plan.problems.some((item) => item.includes("目标包含 tombstone 路径：Version.md")));
  fs.unlinkSync(path.join(f.target, TOMBSTONE)); fs.mkdirSync(path.join(f.target, ".agents", "skills", "desktop-instantiate-project"), { recursive: true }); plan = f.plan(2); assert.ok(plan.problems.some((item) => item.includes("desktop-instantiate-project")));
});

test("manual_add_cannot_be_recorded_without_creating_target", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MIXED, "candidate"); const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MIXED), "manual_add");
  f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--resolved-manual", MIXED, "--bootstrap", "--approval", "bootstrap-verified-baseline"], 2); assert.equal(fs.existsSync(f.lock), false);
});

test("candidate_symlink_blocks_plan_and_bootstrap_record", (t) => {
  const f = new HarnessUpgradeFixture(t); const outside = path.join(f.root, "outside.txt"); fs.writeFileSync(outside, "secret"); const linked = path.join(f.candidate, MANAGED); fs.mkdirSync(path.dirname(linked), { recursive: true }); if (!symlinkOrSkip(t, outside, linked)) return;
  const { planPath } = f.createPlan(2); f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--bootstrap", "--approval", "bootstrap-verified-baseline"], 2); assert.equal(fs.existsSync(f.lock), false);
});

test("bootstrap_rejects_divergent_managed_overlap", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "upstream"); f.write(f.target, MANAGED, "local"); const { planPath } = f.createPlan(2); f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--bootstrap", "--approval", "bootstrap-verified-baseline"], 2); assert.equal(fs.existsSync(f.lock), false);
});

test("bootstrap_rejects_tombstone", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.target, TOMBSTONE, "forbidden"); const { planPath } = f.createPlan(2); f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--bootstrap", "--approval", "bootstrap-verified-baseline"], 2); assert.equal(fs.existsSync(f.lock), false);
});

/** 共享消息校验与发布 helper 随升级传播，已确认项目策略逐字节保护。 */
test("Chinese commit dependency closure is managed and policy remains byte-identical", (t) => {
  for (const ending of ["\n", "\r\n"]) {
    const f = new HarnessUpgradeFixture(t);
    const paths = REQUIRED_MANAGED_SOURCE_PATHS.filter((relative) => relative.includes("desktop-configure-git-commits/") || relative.endsWith("/release_git.mjs") || relative.endsWith("/release_git.test.mjs"));
    assert.equal(paths.length, 8);
    f.bootstrap();
    const policy = Buffer.from(["---", "schema_version: 4", "confirmed_by: 用户已确认", "confirmed_at: 2026-10-01", "user_owned_tasks: disabled", "superpowers: disabled", "parallel_worktree_subagents: disabled", "acceptance_smoke: enabled", "e2e_hint: disabled", "post_release_action: local_package", "---", "", "原项目正文", ""].join(ending));
    f.write(f.target, PROTECTED, policy);
    for (const relative of paths) f.write(f.source, relative, `source:${relative}`);
    f.git(f.source, "add", ...paths);
    f.git(f.source, "-c", "user.name=Harness Fixture", "-c", "user.email=harness-fixture@example.invalid", "commit", "-m", "chore: 记录共享消息校验依赖");
    f.sourceCommit = f.git(f.source, "rev-parse", "HEAD").stdout.trim();
    const missing = f.plan(2);
    for (const relative of paths) assert.ok(missing.problems.some((problem) => problem.includes(relative) && problem.includes("候选缺少")));
    for (const relative of paths) f.write(f.candidate, relative, `source:${relative}`);
    const plan = f.plan();
    for (const relative of paths) assert.equal(f.classification(plan, relative), "add");
    assert.equal(plan.actions.some((action) => action.path === PROTECTED), false);
    assert.deepEqual(fs.readFileSync(path.join(f.target, PROTECTED)), policy);
    for (const relative of paths) f.write(f.target, relative, `source:${relative}`);
    f.record(f.createPlan(0, "converged").planPath);
    const helper = paths.find((relative) => relative.endsWith("/configure_git_commit.mjs"));
    f.write(f.source, helper, "updated shared validator");
    f.write(f.candidate, helper, "updated shared validator");
    f.git(f.source, "add", helper);
    f.git(f.source, "-c", "user.name=Harness Fixture", "-c", "user.email=harness-fixture@example.invalid", "commit", "-m", "chore: 更新共享校验器");
    f.sourceCommit = f.git(f.source, "rev-parse", "HEAD").stdout.trim();
    const update = f.createPlan();
    assert.equal(f.classification(update.plan, helper), "update");
    f.runTool(["apply", "--plan", update.planPath, "--approval", "apply-managed-changes", "--path", helper]);
    f.record(f.createPlan(0, "updated").planPath);
    assert.deepEqual(fs.readFileSync(path.join(f.target, PROTECTED)), policy);
  }
});
