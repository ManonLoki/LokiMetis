/** GUI 框架选择、候选传播与受保护事实的升级回归。 */

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { inapplicableGuiCandidate, parseInterfaceSelection } from "./harness_upgrade_interfaces.mjs";
import { loadOwnership } from "./harness_upgrade_ownership.mjs";
import { HarnessUpgradeFixture, MANAGED } from "./harness_upgrade_test_support.mjs";

const METADATA = "[workspace.metadata.agent-first-harness]\n";
const GPUI = ".agents/skills/desktop-add-gpui-adapter/SKILL.md";
const GPUI_BUILD = ".agents/skills/desktop-build-gpui-release/SKILL.md";
const TAURI = ".agents/skills/desktop-add-gui-adapter/SKILL.md";
const GPUI_INIT_ONLY = [
  ".agents/skills/desktop-add-gpui-adapter/scripts/create_gpui_workspace.mjs",
  ".agents/skills/desktop-add-gpui-adapter/scripts/create_gpui_workspace.test.mjs",
  ".agents/skills/desktop-add-gpui-adapter/assets/core/src/lib.rs",
];

/** 老 GUI 缺省 Tauri，显式 GPUI 不受示例正文或其他表干扰。 */
test("framework_selection_defaults_legacy_gui_to_tauri_and_parses_explicit_gpui", () => {
  assert.deepEqual(parseInterfaceSelection(`${METADATA}interfaces = ["gui"]\n`), { interfaces: ["gui"], guiFramework: "tauri" });
  assert.deepEqual(parseInterfaceSelection(`description = """\n${METADATA}gui-framework = "invalid"\n"""\n[workspace.metadata."agent-first-harness"]\ninterfaces = [\n 'cli', # interface\n "gui",\n]\n"gui-framework" = 'gpui' # framework\n`), { interfaces: ["cli", "gui"], guiFramework: "gpui" });
  assert.deepEqual(parseInterfaceSelection('[workspace.metadata]\nagent-first-harness.interfaces = ["gui"]\nagent-first-harness.gui-framework = "gpui"\n'), { interfaces: ["gui"], guiFramework: "gpui" });
});

/** 未知值、错类型、重复键和非法接口不能降级到默认值。 */
test("framework_selection_rejects_unknown_duplicate_and_malformed_metadata", () => {
  for (const suffix of [
    'gui-framework = "unknown"', 'gui-framework = false', 'gui-framework = 1',
    'gui-framework = "gpui"\ngui-framework = "tauri"',
    'interfaces = ["gui"]', '[workspace.metadata.agent-first-harness]\ngui-framework = "gpui"',
    '[workspace.metadata.agent-first-harness.gui-framework]',
  ]) assert.throws(() => parseInterfaceSelection(`${METADATA}interfaces = ["gui"]\n${suffix}\n`), /Cargo/u);
  for (const interfaces of ['"gui"', '["gui", "gui"]', '["unknown"]', '[false]']) {
    assert.throws(() => parseInterfaceSelection(`${METADATA}interfaces = ${interfaces}\n`), /Cargo/u);
  }
  assert.throws(() => parseInterfaceSelection(`${METADATA}interfaces = ["cli"]\ngui-framework = "gpui"\n`), /只适用于已选择 GUI/u);
  assert.throws(() => parseInterfaceSelection('workspace.metadata.agent-first-harness = { interfaces = ["gui"], gui-framework = "gpui" }\n'), /内联表/u);
  assert.throws(() => parseInterfaceSelection('gui-framework = "gpui"\n'), /必须位于 workspace.metadata/u);
});

/** GUI 工程目录按持久框架选择，不从候选中的技能反推接口。 */
test("gui_candidate_selection_excludes_the_other_framework_and_non_gui_projects", () => {
  for (const selection of [{ interfaces: ["cli"], guiFramework: null }, { interfaces: ["gui"], guiFramework: "tauri" }]) {
    assert.equal(inapplicableGuiCandidate(GPUI, selection), true);
    assert.equal(inapplicableGuiCandidate(GPUI_BUILD, selection), true);
  }
  const gpui = { interfaces: ["gui"], guiFramework: "gpui" };
  assert.equal(inapplicableGuiCandidate(GPUI, gpui), false);
  assert.equal(inapplicableGuiCandidate(GPUI_BUILD, gpui), false);
  for (const name of ["desktop-add-gui-adapter", "mantine-list-view", "desktop-add-gui-dialog", "desktop-prepare-gui-support-surfaces", "desktop-build-tauri-release"]) {
    assert.equal(inapplicableGuiCandidate(`.agents/skills/${name}/SKILL.md`, gpui), true, name);
  }
  assert.equal(inapplicableGuiCandidate(".agents/skills/desktop-prepare-gui-app-identity/SKILL.md", gpui), false);
});

/** GPUI 工程可规划，Tauri/Mantine 候选阻断，产品 profile 和源码仍受保护。 */
test("gpui_upgrade_plan_isolates_framework_assets_and_preserves_product_files", (t) => {
  const f = new HarnessUpgradeFixture(t);
  f.write(f.target, "Cargo.toml", `${METADATA}interfaces = ["gui"]\ngui-framework = "gpui"\n`);
  f.write(f.target, "docs/GUI_APP_PROFILE.md", "local approved profile\n");
  f.write(f.target, "crates/example-gui/src/main.rs", "local product source\n");
  f.write(f.candidate, GPUI, "neutral gpui engineering skill\n");
  f.write(f.candidate, GPUI_BUILD, "native GPUI packaging skill\n");
  const gpuiRender = ".agents/skills/desktop-add-gpui-adapter/scripts/gpui_adapter_files.mjs";
  const gpuiAdd = ".agents/skills/desktop-add-gpui-adapter/scripts/add_gpui_adapter.mjs";
  f.write(f.candidate, gpuiRender, "pure render library\n");
  f.write(f.candidate, gpuiAdd, "existing core add-only adapter\n");
  const before = fs.readFileSync(path.join(f.target, "Cargo.toml"), "utf8");
  const plan = f.plan();
  assert.equal(plan.gui_framework, "gpui");
  assert.deepEqual(plan.target_interfaces, ["gui"]);
  assert.equal(plan.actions.find((item) => item.path === GPUI).mode, "conditional");
  assert.equal(f.classification(plan, GPUI), "manual_add");
  assert.equal(plan.actions.find((item) => item.path === GPUI_BUILD).mode, "conditional");
  assert.equal(f.classification(plan, GPUI_BUILD), "manual_add");
  assert.equal(f.classification(plan, gpuiRender), "manual_add");
  assert.equal(f.classification(plan, gpuiAdd), "manual_add");
  assert.equal(plan.actions.some((item) => item.path.endsWith("main.rs")), false);
  for (const relative of [TAURI, ".agents/skills/mantine-list-view/SKILL.md"]) {
    f.write(f.candidate, relative, "wrong framework\n");
    assert.ok(f.plan(2).problems.some((problem) => problem.includes("不适用于目标 GUI 框架")));
    fs.rmSync(path.join(f.candidate, relative));
  }
  f.write(f.candidate, "docs/GUI_APP_PROFILE.md", "upstream profile\n");
  assert.equal(f.classification(f.plan(2), "docs/GUI_APP_PROFILE.md"), "protected_candidate");
  assert.equal(fs.readFileSync(path.join(f.target, "Cargo.toml"), "utf8"), before);
  assert.equal(fs.readFileSync(path.join(f.target, "docs/GUI_APP_PROFILE.md"), "utf8"), "local approved profile\n");
});

/** 缺省旧 GUI 可接收 Tauri，纯 CLI 不可接收任一种 GUI 技能。 */
test("legacy_tauri_and_non_gui_plans_enforce_gui_applicability", (t) => {
  const f = new HarnessUpgradeFixture(t);
  f.write(f.target, "Cargo.toml", `${METADATA}interfaces = ["gui"]\n`);
  f.write(f.candidate, TAURI, "tauri engineering skill\n");
  assert.equal(f.plan().gui_framework, "tauri");
  for (const relative of [GPUI, GPUI_BUILD]) {
    f.write(f.candidate, relative, "gpui engineering skill\n");
    assert.ok(f.plan(2).problems.some((problem) => problem.includes(relative)));
    fs.rmSync(path.join(f.candidate, relative));
  }
  f.write(f.target, "Cargo.toml", `${METADATA}interfaces = ["cli"]\n`);
  assert.ok(f.plan(2).problems.some((problem) => problem.includes(TAURI)));
});

/** 框架事实在计划后改变，即使 Git 脏状态相同也禁止应用其他受管更新。 */
test("framework_metadata_drift_blocks_reviewed_managed_application", (t) => {
  const f = new HarnessUpgradeFixture(t);
  f.write(f.target, "Cargo.toml", `${METADATA}interfaces = ["gui"]\ngui-framework = "gpui"\n`);
  f.write(f.candidate, MANAGED, "v1");
  f.write(f.target, MANAGED, "v1");
  f.bootstrap();
  f.write(f.candidate, MANAGED, "v2");
  const { planPath } = f.createPlan();
  f.write(f.target, "Cargo.toml", `${METADATA}interfaces = ["gui"]\ngui-framework = "tauri"\n`);
  const result = f.runTool(["apply", "--plan", planPath, "--approval", "apply-managed-changes", "--path", MANAGED], 2);
  assert.match(result.error, /已复核 plan 不再匹配/u);
  assert.equal(fs.readFileSync(path.join(f.target, MANAGED), "utf8"), "v1");
});

/** Conditional 规则不能被更早的通用 managed 规则吞掉。 */
test("gpui_conditional_rule_must_precede_generic_managed_ownership", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const manifest = JSON.parse(fs.readFileSync(f.ownership, "utf8"));
  const gpui = manifest.rules.find((item) => item.pattern === ".agents/skills/desktop-add-gpui-adapter/**");
  manifest.rules = manifest.rules.filter((item) => item !== gpui);
  manifest.rules.push(gpui);
  fs.writeFileSync(f.ownership, JSON.stringify(manifest));
  assert.throws(() => loadOwnership(f.ownership), /conditional 所有权规则必须位于通用 managed/u);
});

/** 新打包 Skill 的条件所有权也必须显式存在并位于通用 managed 之前。 */
test("gpui_build_conditional_ownership_cannot_be_omitted_or_shadowed", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const manifest = JSON.parse(fs.readFileSync(f.ownership, "utf8"));
  const rule = manifest.rules.find((item) => item.pattern === ".agents/skills/desktop-build-gpui-release/**");
  assert.equal(rule.mode, "conditional");
  manifest.rules = manifest.rules.filter((item) => item !== rule);
  fs.writeFileSync(f.ownership, JSON.stringify(manifest));
  assert.throws(() => loadOwnership(f.ownership), /所有权 manifest 削弱了必需保护/u);
  manifest.rules.push(rule);
  fs.writeFileSync(f.ownership, JSON.stringify(manifest));
  assert.throws(() => loadOwnership(f.ownership), /conditional 所有权规则必须位于通用 managed/u);
});

/** GPUI 终端升级拒绝重新传播首次创建入口和 core 模板。 */
test("gpui_initialization_only_paths_block_candidate_and_target_trees", (t) => {
  for (const relative of GPUI_INIT_ONLY) {
    const f = new HarnessUpgradeFixture(t);
    f.write(f.target, "Cargo.toml", `${METADATA}interfaces = ["gui"]\ngui-framework = "gpui"\n`);
    f.write(f.candidate, relative, "initialization-only asset\n");
    assert.equal(f.classification(f.plan(2), relative), "tombstone_candidate");
    fs.rmSync(path.join(f.candidate, ".agents/skills/desktop-add-gpui-adapter"), { recursive: true });
    f.write(f.target, relative, "stale initialization-only asset\n");
    assert.equal(f.classification(f.plan(2), relative), "tombstone_present");
  }
});

/** 精确 tombstone 不能放在较宽 GPUI conditional 规则之后。 */
test("gpui_initialization_tombstones_must_precede_the_conditional_tree", (t) => {
  const f = new HarnessUpgradeFixture(t);
  const manifest = JSON.parse(fs.readFileSync(f.ownership, "utf8"));
  const rule = manifest.rules.find((item) => item.pattern === GPUI_INIT_ONLY[0]);
  manifest.rules = manifest.rules.filter((item) => item !== rule);
  manifest.rules.push(rule);
  fs.writeFileSync(f.ownership, JSON.stringify(manifest));
  assert.throws(() => loadOwnership(f.ownership), /GPUI 初始化专用 tombstone 规则必须位于 GPUI conditional/u);
});
