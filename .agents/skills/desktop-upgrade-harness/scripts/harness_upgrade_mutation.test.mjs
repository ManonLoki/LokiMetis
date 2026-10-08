#!/usr/bin/env node
/** Harness 升级写入、漂移检测和记录回归。 */

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { HarnessUpgradeFixture, MANAGED, MANAGED_SECOND, MANAGED_SELF, MIXED, PROTECTED, symlinkOrSkip } from "./harness_upgrade_test_support.mjs";
import { snapshotFile } from "./harness_upgrade_safety.mjs";

const OWNERSHIP_MANIFEST = ".agents/skills/desktop-upgrade-harness/references/ownership-manifest.json";
const SWITCH_RULE = ".agents/skills/desktop-switch-post-release-action/**";

/** 模拟旧下游已记录的清单，保持来源锁和目标字节一致。 */
function setLegacyOwnershipBaseline(f) {
  const manifest = JSON.parse(fs.readFileSync(f.ownership, "utf8"));
  manifest.rules = manifest.rules.filter((item) => item.pattern !== SWITCH_RULE);
  fs.writeFileSync(f.ownership, `${JSON.stringify(manifest, null, 2)}\n`);
  const previous = snapshotFile(f.ownership);
  const lock = JSON.parse(fs.readFileSync(f.lock, "utf8"));
  lock.entries[OWNERSHIP_MANIFEST] = { mode: "managed-self", candidate: previous, target: previous };
  fs.writeFileSync(f.lock, `${JSON.stringify(lock)}\n`);
}

/** POSIX 权限位在 Windows 上不存在，相关用例只在 POSIX 主机运行。 */
const posixOnly = { skip: process.platform === "win32" };

test("tampered_plan_cannot_overwrite_protected_file", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.write(f.target, PROTECTED, "keep"); f.bootstrap(); f.write(f.candidate, MANAGED, "v2"); f.write(f.candidate, PROTECTED, "malicious");
  const { planPath } = f.createPlan(2); const payload = JSON.parse(fs.readFileSync(planPath, "utf8")); payload.actions.find((item) => item.path === MANAGED).path = PROTECTED; fs.writeFileSync(planPath, JSON.stringify(payload));
  f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", PROTECTED], 2); assert.equal(fs.readFileSync(path.join(f.target, PROTECTED), "utf8"), "keep");
});

test("target_parent_symlink_after_plan_cannot_escape", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); f.write(f.candidate, MANAGED, "v2"); const { planPath } = f.createPlan();
  const managedParent = path.dirname(path.join(f.target, MANAGED)); fs.rmSync(managedParent, { recursive: true }); const outsideParent = path.join(f.root, "outside-managed"); fs.mkdirSync(outsideParent); const outsideFile = path.join(outsideParent, path.basename(MANAGED)); fs.writeFileSync(outsideFile, "v1"); if (!symlinkOrSkip(t, outsideParent, managedParent, "dir")) return;
  f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED], 2); assert.equal(fs.readFileSync(outsideFile, "utf8"), "v1");
});

test("plan_output_cannot_overwrite_target_or_existing_file", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.target, PROTECTED, "keep"); f.runTool(["plan", ...f.sharedArguments(), "--output", path.join(f.target, PROTECTED)], 2); assert.equal(fs.readFileSync(path.join(f.target, PROTECTED), "utf8"), "keep");
  const existing = path.join(f.root, "existing.json"); fs.writeFileSync(existing, "keep"); f.runTool(["plan", ...f.sharedArguments(), "--output", existing], 2); assert.equal(fs.readFileSync(existing, "utf8"), "keep");
});

test("lock_path_must_be_exact_control_path", (t) => {
  const f = new HarnessUpgradeFixture(t); f.runTool(["plan", ...f.sharedArguments(path.join(f.root, "forged-lock.json"))], 2); f.runTool(["plan", ...f.sharedArguments(path.join(f.target, "src", "provenance.json"))], 2);
});

test("ownership_mode_drift_blocks", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); const lock = JSON.parse(fs.readFileSync(f.lock, "utf8")); lock.entries[MANAGED].mode = "merge-sections"; fs.writeFileSync(f.lock, JSON.stringify(lock)); assert.ok(f.plan(2).problems.some((item) => item.includes("所有权 mode 已变化")));
});

test("weakened_ownership_manifest_is_rejected", (t) => {
  const f = new HarnessUpgradeFixture(t); const manifest = JSON.parse(fs.readFileSync(f.ownership, "utf8")); manifest.rules = manifest.rules.filter((item) => item.pattern !== "Version.md"); fs.writeFileSync(f.ownership, JSON.stringify(manifest)); assert.match(f.runTool(["plan", ...f.sharedArguments()], 2).error, /削弱了必需保护/);
});

test("legacy_managed_rule_is_planned_repaired_and_required_before_record", (t) => {
  const f = new HarnessUpgradeFixture(t);
  f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap();
  setLegacyOwnershipBaseline(f);
  f.write(f.candidate, MANAGED, "v2");
  const { plan, planPath } = f.createPlan();
  assert.deepEqual(plan.ownership_repairs, [SWITCH_RULE]);
  assert.equal(f.classification(plan, OWNERSHIP_MANIFEST), "update");
  assert.equal(f.classification(plan, MANAGED), "update");
  assert.match(f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--approval", "record-verified-baseline"], 2).error, /削弱了必需保护/);
  assert.match(f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", OWNERSHIP_MANIFEST], 2).error, /managed-self 更新必须等待/);
  f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED]);
  const afterManaged = f.createPlan();
  f.runTool(["apply", "--plan", afterManaged.planPath, "--approval", "apply-managed-changes", "--path", OWNERSHIP_MANIFEST]);
  const repaired = f.createPlan();
  assert.deepEqual(repaired.plan.ownership_repairs, []);
  assert.equal(f.classification(repaired.plan, OWNERSHIP_MANIFEST), "converged");
  f.record(repaired.planPath);
});

test("source_provenance_is_bound_to_clean_git_head", (t) => {
  const f = new HarnessUpgradeFixture(t); f.bootstrap(); const { planPath } = f.createPlan(); let result = f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", "0".repeat(40), "--approval", "record-verified-baseline"], 2); assert.match(result.error, /必须与已复核 plan 精确匹配/);
  f.write(f.source, "untracked.txt", "drift"); result = f.runTool(["plan", ...f.sharedArguments()], 2); assert.match(result.error, /必须保持干净/);
});

test("control_file_drift_blocks_apply_before_write", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); f.bootstrap(); f.write(f.candidate, MANAGED, "v2"); const { planPath } = f.createPlan(); const manifest = JSON.parse(fs.readFileSync(f.ownership, "utf8")); fs.writeFileSync(f.ownership, `${JSON.stringify(manifest, null, 4)}\n`);
  f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED], 2); assert.equal(fs.readFileSync(path.join(f.target, MANAGED), "utf8"), "v1");
});

test("permission_mode_changes_follow_three_way_rules", posixOnly, (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); f.write(f.target, MANAGED, "v1"); fs.chmodSync(path.join(f.candidate, MANAGED), 0o644); fs.chmodSync(path.join(f.target, MANAGED), 0o644); f.bootstrap(); fs.chmodSync(path.join(f.candidate, MANAGED), 0o755);
  const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MANAGED), "update"); f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED]); assert.equal(fs.statSync(path.join(f.target, MANAGED)).mode & 0o777, 0o755); f.record(f.createPlan(0, "mode-converged").planPath); fs.chmodSync(path.join(f.target, MANAGED), 0o700); assert.equal(f.classification(f.plan(), MANAGED), "preserve_local");
});

test("special_permission_bits_are_rejected", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MANAGED, "v1"); fs.chmodSync(path.join(f.candidate, MANAGED), 0o1755); const observed = fs.statSync(path.join(f.candidate, MANAGED)).mode & 0o7777; if (!(observed & ~0o777)) return t.skip("文件系统会清除特殊权限位"); assert.match(f.runTool(["plan", ...f.sharedArguments()], 2).error, /不受支持的特殊权限位/);
});

test("full_preflight_prevents_partial_apply", (t) => {
  const f = new HarnessUpgradeFixture(t); for (const relative of [MANAGED, MANAGED_SECOND]) { f.write(f.candidate, relative, "v1"); f.write(f.target, relative, "v1"); } f.bootstrap(); for (const relative of [MANAGED, MANAGED_SECOND]) f.write(f.candidate, relative, "v2"); const { planPath } = f.createPlan(); f.write(f.target, MANAGED_SECOND, "changed-after-plan"); f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED], 2); assert.equal(fs.readFileSync(path.join(f.target, MANAGED), "utf8"), "v1");
});

test("manual_merge_must_be_named_before_record", (t) => {
  const f = new HarnessUpgradeFixture(t); f.write(f.candidate, MIXED, "v1"); f.write(f.target, MIXED, "v1"); f.bootstrap(); f.write(f.candidate, MIXED, "upstream"); f.write(f.target, MIXED, "reviewed-local-merge"); const { plan, planPath } = f.createPlan(); assert.equal(f.classification(plan, MIXED), "manual_merge"); f.runTool(["record", "--plan", planPath, "--source-version", f.sourceVersion, "--source-commit", f.sourceCommit, "--approval", "record-verified-baseline"], 2); assert.ok(f.record(planPath, [MIXED]).entries > 0);
});

test("managed_self_updates_after_other_managed_files", (t) => {
  const f = new HarnessUpgradeFixture(t); for (const relative of [MANAGED, MANAGED_SELF]) { f.write(f.candidate, relative, "v1"); f.write(f.target, relative, "v1"); } f.bootstrap(); for (const relative of [MANAGED, MANAGED_SELF]) f.write(f.candidate, relative, "v2");
  const { planPath } = f.createPlan(); f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED_SELF], 2); assert.deepEqual(f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED]).applied, [MANAGED]);
  const selfPlan = f.createPlan(0, "managed-self").planPath; assert.deepEqual(f.runTool(["apply", "--plan", selfPlan, "--approval", "apply-managed-changes", "--path", MANAGED_SELF]).applied, [MANAGED_SELF]);
});
