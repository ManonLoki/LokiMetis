import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { localPackageSupported, parseAgentPolicyDocument, parseReleaseGuiFramework, parseReleaseMetadataArray } from "./post_release_action.mjs";

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), "post_release_action.mjs");
const V3 = [
  "---",
  "schema_version: 3",
  "confirmed_by: user",
  "confirmed_at: 2026-09-28",
  "decision_mode: reuse_then_infer_then_ask",
  "superpowers: disabled",
  "user_owned_tasks: disabled",
  "parallel_worktree_subagents: disabled",
  "acceptance_smoke: enabled",
  "e2e_hint: disabled",
  "---",
  "# 本地策略",
  "保留本地正文。",
  "- `post_release_action`：`local_package` 或 `push_release_branch`。",
  "发布后动作直接读取 `post_release_action`。",
  "完成初始化的下游冻结已确认的 `post_release_action`。",
  "使用 `push-release --remote <name>`。",
  "远端默认主分支、`release` 分支和 tag 同步到同一已发布 HEAD。",
  "",
].join("\n");

function fixture(t, contents = V3) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "post-release-action-")));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const initialized = spawnSync("git", ["-C", root, "init", "-q", "--initial-branch=main"], { encoding: "utf8" });
  assert.equal(initialized.status, 0, initialized.stderr);
  const docs = path.join(root, "docs");
  fs.mkdirSync(docs);
  fs.writeFileSync(path.join(root, "Cargo.toml"), "[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"cli\"]\n");
  const policy = path.join(docs, "AGENT_POLICY.md");
  fs.writeFileSync(policy, contents);
  const run = (command, options = [], expectedStatus = 0) => {
    const result = spawnSync(process.execPath, [SCRIPT, command, "--project-root", root, ...options], { encoding: "utf8" });
    assert.equal(result.status, expectedStatus, result.stderr || result.stdout);
    return JSON.parse(expectedStatus === 0 ? result.stdout : result.stderr);
  };
  return { root, docs, policy, run };
}

function setOptions(action, expected = "missing") {
  return ["--action", action, "--expected-action", expected, "--confirmed-user-choice"];
}

/** 只将测试场景明确指定的清单放入 Git 索引。 */
function track(f, ...files) {
  const added = spawnSync("git", ["-C", f.root, "add", "--", ...files], { encoding: "utf8" });
  assert.equal(added.status, 0, added.stderr);
}

/** GPUI 原生 macOS/Windows 路线独立可选，仅 Linux GUI 仍没有本地包路线。 */
test("gpui_local_package_supports_native_gui_platforms_without_cli", (t) => {
  assert.equal(localPackageSupported(["gui"], ["macos"], "gpui"), true);
  assert.equal(localPackageSupported(["gui"], ["windows"], "gpui"), true);
  assert.equal(localPackageSupported(["gui"], ["linux"], "gpui"), false);
  assert.equal(localPackageSupported(["cli", "gui"], ["macos"], "gpui"), true);
  assert.equal(localPackageSupported(["cli", "gui"], ["linux"], "gpui"), true);
  assert.equal(localPackageSupported(["gui"], ["macos"]), true);
  assert.throws(() => localPackageSupported(["gui"], ["macos"], "unknown"), /gui-framework/u);
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), '[workspace.metadata.agent-first-harness]\ntarget-platforms = ["linux"]\ninterfaces = ["gui"]\ngui-framework = "gpui"\n');
  assert.match(f.run("set", setOptions("local_package"), 2).error, /没有现有本地打包 Skill/u);
  assert.equal(fs.readFileSync(f.policy, "utf8"), V3);
  assert.equal(f.run("set", setOptions("push_release_branch")).post_release_action, "push_release_branch");
});

/** GPUI 清单与测试范围受跟踪即可选择原生包，不借用 Tauri 或 pnpm 清单。 */
test("gpui_local_package_checks_rust_manifests_without_frontend_files", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), '[workspace]\nmembers = ["app"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = ["macos", "windows"]\ninterfaces = ["gui"]\ngui-framework = "gpui"\ngui-root = "app"\nrust-test-manifests = ["Cargo.toml"]\n');
  assert.match(f.run("set", setOptions("local_package"), 2).error, /GPUI GUI 根目录 不存在/u);
  fs.mkdirSync(path.join(f.root, "app", "src"), { recursive: true });
  fs.writeFileSync(path.join(f.root, "app", "Cargo.toml"), '[package]\nname = "example"\nversion = "0.1.0"\n');
  fs.writeFileSync(path.join(f.root, "app", "src", "main.rs"), "fn main() {}\n");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /必须受 Git 跟踪/u);
  track(f, "Cargo.toml", "app/Cargo.toml", "app/src/main.rs");
  assert.equal(f.run("set", setOptions("local_package")).post_release_action, "local_package");
  const cargo = path.join(f.root, "Cargo.toml");
  const initial = fs.readFileSync(cargo, "utf8");
  fs.writeFileSync(cargo, initial.replace('rust-test-manifests = ["Cargo.toml"]', 'rust-test-manifests = []'));
  assert.match(f.run("check", [], 2).error, /rust-test-manifests/u);
  fs.writeFileSync(cargo, initial.replace('members = ["app"]', "members = []"));
  assert.match(f.run("check", [], 2).error, /空 workspace/u);
  fs.writeFileSync(cargo, initial + 'dependency-lock-policy = "tracked"\n');
  assert.match(f.run("check", [], 2).error, /Cargo.lock/u);
  fs.writeFileSync(path.join(f.root, "Cargo.lock"), "# root lock\n");
  track(f, "Cargo.lock");
  assert.equal(f.run("check").status, "configured");
  assert.equal(fs.existsSync(path.join(f.root, "app", "package.json")), false);
  assert.equal(fs.existsSync(path.join(f.root, "app", "pnpm-lock.yaml")), false);
});

/** GPUI 独立 GUI workspace 的锁策略同样检查，不能只检查根 Cargo.lock。 */
test("gpui_local_package_checks_independent_gui_workspace_lock", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), '[workspace]\nmembers = ["core"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = ["windows"]\ninterfaces = ["gui"]\ngui-framework = "gpui"\ngui-root = "app"\ndependency-lock-policy = "tracked"\n');
  for (const name of ["core", "app"]) {
    fs.mkdirSync(path.join(f.root, name, "src"), { recursive: true });
    fs.writeFileSync(path.join(f.root, name, "Cargo.toml"), `[package]\nname = "${name}"\nversion = "0.1.0"\n${name === "app" ? "[workspace]\n" : ""}`);
    fs.writeFileSync(path.join(f.root, name, "src", "lib.rs"), "pub fn marker() {}\n");
  }
  fs.writeFileSync(path.join(f.root, "Cargo.lock"), "# root lock\n");
  track(f, "Cargo.toml", "Cargo.lock", "core/Cargo.toml", "core/src/lib.rs", "app/Cargo.toml", "app/src/lib.rs");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /app\/Cargo.lock/u);
  fs.writeFileSync(path.join(f.root, "app", "Cargo.lock"), "# GUI lock\n");
  track(f, "app/Cargo.lock");
  assert.equal(f.run("set", setOptions("local_package")).status, "configured");
});

/** 缺省 GUI 目录优先使用项目标识，Worktree 根目录名称不改变已选包位置。 */
test("gpui_gui_root_defaults_to_project_identity_inside_renamed_checkouts", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), '[workspace]\nmembers = ["example_gui"]\n[workspace.metadata.agent-first-harness]\nproject-id = "example"\ntarget-platforms = ["macos"]\ninterfaces = ["gui"]\ngui-framework = "gpui"\n');
  fs.mkdirSync(path.join(f.root, "example_gui"));
  fs.writeFileSync(path.join(f.root, "example_gui", "Cargo.toml"), '[package]\nname = "example_gui"\nversion = "0.1.0"\n');
  track(f, "Cargo.toml", "example_gui/Cargo.toml");
  assert.equal(f.run("set", setOptions("local_package")).status, "configured");
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), fs.readFileSync(path.join(f.root, "Cargo.toml"), "utf8").replace('project-id = "example"', 'project-id = false'));
  assert.match(f.run("check", [], 2).error, /project-id/u);
});

/** 框架字段遵循唯一事实；兼容旧项目但不修正显式错误。 */
test("gui_framework_metadata_defaults_only_when_absent", () => {
  const source = '[workspace.metadata.agent-first-harness]\ninterfaces = ["gui"]\n';
  assert.equal(parseReleaseGuiFramework(source), "tauri");
  for (const declaration of [
    'gui-framework = "gpui"', '"gui-framework" = "gpui"', "'gui-framework' = 'gpui' # native GUI",
    '"gui-\\u0066ramework" = "gpui"',
  ]) assert.equal(parseReleaseGuiFramework(`${source}${declaration}\n`), "gpui");
  assert.equal(parseReleaseGuiFramework(`${source}"gui-framework" = 'tauri'\n`), "tauri");
  for (const suffix of ['gui-framework = "unknown"\n', 'gui-framework = null\n', 'gui-framework = "gpui"\ngui-framework = "tauri"\n']) {
    assert.throws(() => parseReleaseGuiFramework(source + suffix), /gui-framework/u);
  }
});

/** 表、错位与混用引号的重复键均为显式错误，不能退回旧 Tauri 路线。 */
test("gui_framework_parser_rejects_structural_errors_and_ignores_text", () => {
  const source = '[workspace.metadata.agent-first-harness]\ninterfaces = ["gui"]\n';
  for (const invalid of [
    `${source}[workspace.metadata.agent-first-harness.gui-framework]\n`,
    `${source}[[workspace.metadata.agent-first-harness.'gui-framework']]\n`,
    `${source}gui-framework.name = "gpui"\n`,
    `${source}[package.metadata]\n"gui-framework" = "gpui"\n`,
    `gui-framework = "gpui"\n${source}`,
    `${source}gui-framework = "gpui"\n"gui-framework" = "tauri"\n`,
    `${source}"gui-framework" = false\n`,
    `${source}"gui-framework" = "unknown"\n`,
    `workspace = { metadata = { agent-first-harness = { gui-framework = "gpui" } } }\n${source}`,
    `${source}"gui-framework\\U00000000" = "gpui"\n`,
    `${source}"gui-framework" = "gpui\n`,
  ]) assert.throws(() => parseReleaseGuiFramework(invalid), /gui-framework/u, invalid);
  const description = '[package]\ndescription = """\n[workspace.metadata.agent-first-harness]\ngui-framework = "unknown"\n"""\n';
  assert.equal(parseReleaseGuiFramework(`${description}${source}# gui-framework = "unknown"\n`), "tauri");
  assert.equal(parseReleaseGuiFramework(`${description}${source}"gui-framework" = 'gpui' # chosen\n`), "gpui");
  assert.equal(parseReleaseGuiFramework("[workspace.metadata.'agent-first-harness'] # metadata\n'gui-framework' = 'gpui'\n"), "gpui");
  assert.throws(() => parseReleaseGuiFramework(`${source}[workspace.metadata.'agent-first-harness']\n`), /缺失或重复/u);
});

/** 真实 set/check 按引号框架键选择原生 GPUI，并在框架错误时保持策略与目录字节不变。 */
test("quoted_gpui_framework_routes_set_check_and_errors_do_not_write", (t) => {
  const f = fixture(t);
  const cargo = path.join(f.root, "Cargo.toml");
  const manifest = '[workspace]\nmembers = ["app"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = ["macos"]\ninterfaces = ["gui"]\ngui-root = "app"\nrust-test-manifests = ["Cargo.toml"]\n';
  fs.mkdirSync(path.join(f.root, "app"));
  fs.writeFileSync(path.join(f.root, "app", "Cargo.toml"), '[package]\nname = "app"\nversion = "0.1.0"\n');
  fs.writeFileSync(cargo, `${manifest}"gui-framework" = 'gpui' # native\n`);
  track(f, "Cargo.toml", "app/Cargo.toml");
  assert.equal(f.run("set", setOptions("local_package")).post_release_action, "local_package");
  assert.equal(f.run("check").status, "configured");
  assert.equal(fs.existsSync(path.join(f.root, "app", "package.json")), false);
  const configured = fs.readFileSync(f.policy);
  const files = fs.readdirSync(f.docs);
  for (const suffix of [
    '[workspace.metadata.agent-first-harness."gui-framework"]\n',
    '[package.metadata]\n"gui-framework" = "gpui"\n',
    'gui-framework = "gpui"\n"gui-framework" = "tauri"\n',
    '"gui-framework" = "unknown"\n',
  ]) {
    fs.writeFileSync(cargo, manifest + suffix);
    assert.match(f.run("check", [], 2).error, /gui-framework/u);
    assert.ok(fs.readFileSync(f.policy).equals(configured));
    fs.writeFileSync(f.policy, V3);
    assert.match(f.run("set", setOptions("local_package"), 2).error, /gui-framework/u);
    assert.equal(fs.readFileSync(f.policy, "utf8"), V3);
    assert.deepEqual(fs.readdirSync(f.docs), files);
    fs.writeFileSync(f.policy, configured);
  }
});

test("shared policy parser preserves runtime field and newline rules", () => {
  assert.equal(parseAgentPolicyDocument(Buffer.from(V3)).schema, "3");
  assert.equal(parseAgentPolicyDocument(Buffer.from(V3.replaceAll("\n", "\r\n"))).newline, "\r\n");
  assert.throws(() => parseAgentPolicyDocument(V3.replace("schema_version: 3", "schema_version : 3")), /不支持的 Agent 策略字段行/u);
  assert.throws(() => parseAgentPolicyDocument(V3.replace("e2e_hint: disabled", "e2e_hint: disabled\ne2e_hint: disabled")), /重复的 Agent 策略字段/u);
  assert.throws(() => parseAgentPolicyDocument(V3.replace("schema_version: 3\n", "schema_version: 3\r\n")), /混合换行/u);
  assert.throws(() => parseAgentPolicyDocument(Buffer.concat([Buffer.from(V3), Buffer.from([0xff])])), /有效 UTF-8/u);
});

test("shared Cargo metadata parser preserves runtime array syntax", () => {
  const source = "[workspace.metadata.agent-first-harness]\ninterfaces = [\"cli\"]\ntarget-platforms = [\"macos\"]\n";
  assert.deepEqual(parseReleaseMetadataArray(source, "interfaces"), ["cli"]);
  assert.deepEqual(parseReleaseMetadataArray(source.replaceAll("\n", "\r\n"), "target-platforms"), ["macos"]);
  for (const invalid of [
    source.replace('["cli"]', "['cli']"),
    `${source}[workspace.metadata.agent-first-harness]\n`,
    `${source}interfaces = [\"cli\"]\n`,
    source.replace('["cli"]', "[]"),
    source.replace('["cli"]', '["unknown"]'),
  ]) {
    assert.throws(() => parseReleaseMetadataArray(invalid, "interfaces"), /Cargo .*元数据/u);
  }
});

test("old downstream requires selection and migrates only after confirmed choice", (t) => {
  const f = fixture(t);
  assert.deepEqual(f.run("inspect"), { schema_version: 3, status: "selection_required", post_release_action: null });
  assert.match(f.run("check", [], 2).error, /尚未选择/u);
  const denied = f.run("set", ["--action", "local_package", "--expected-action", "missing"], 2);
  assert.match(denied.error, /confirmed-user-choice/u);
  assert.equal(fs.readFileSync(f.policy, "utf8"), V3);
  assert.deepEqual(f.run("set", setOptions("local_package")), {
    schema_version: 4, status: "configured", post_release_action: "local_package", changed: true, previous_action: "missing",
  });
  const written = fs.readFileSync(f.policy, "utf8");
  const expected = V3
    .replace("schema_version: 3", "schema_version: 4")
    .replace("e2e_hint: disabled\n---", "e2e_hint: disabled\npost_release_action: local_package\n---");
  assert.equal(written, expected, "迁移只允许改变 schema 并追加发布后选择，必须保留旧五项与确认元数据");
  assert.match(written, /^schema_version: 4$/mu);
  assert.match(written, /^post_release_action: local_package$/mu);
  assert.ok(written.includes("# 本地策略\n保留本地正文。\n"));
  assert.equal(f.run("check").post_release_action, "local_package");
  assert.deepEqual(fs.readdirSync(f.docs).sort(), ["AGENT_POLICY.md"]);
});

test("same choice is byte-identical no-op; future switch changes only action", (t) => {
  const f = fixture(t);
  f.run("set", setOptions("local_package"));
  const before = fs.readFileSync(f.policy);
  assert.equal(f.run("set", setOptions("local_package", "local_package")).changed, false);
  assert.ok(fs.readFileSync(f.policy).equals(before));
  const switched = f.run("set", setOptions("push_release_branch", "local_package"));
  assert.equal(switched.previous_action, "local_package");
  assert.equal(switched.post_release_action, "push_release_branch");
  assert.equal(fs.readFileSync(f.policy, "utf8"), before.toString("utf8").replace("post_release_action: local_package", "post_release_action: push_release_branch"));
  assert.match(f.run("set", setOptions("local_package", "local_package"), 2).error, /已变化/u);
});

test("invalid or incomplete schema cannot pass the completion check or be silently repaired", (t) => {
  const f = fixture(t, V3.replace("schema_version: 3", "schema_version: 4"));
  const before = fs.readFileSync(f.policy);
  assert.match(f.run("check", [], 2).error, /schema 或字段集合/u);
  assert.match(f.run("set", setOptions("local_package"), 2).error, /schema 或字段集合/u);
  assert.ok(fs.readFileSync(f.policy).equals(before));
  fs.writeFileSync(f.policy, V3.replace("schema_version: 3", "schema_version: 4").replace("e2e_hint: disabled", "e2e_hint: disabled\npost_release_action: remote_push"));
  assert.match(f.run("check", [], 2).error, /post_release_action/u);
});

test("unconfirmed legacy metadata and preferences do not become configured", (t) => {
  const f = fixture(t, V3.replace("confirmed_at: 2026-09-28", "confirmed_at: not-a-date"));
  assert.match(f.run("set", setOptions("local_package"), 2).error, /confirmed_at/u);
  fs.writeFileSync(f.policy, V3.replace("acceptance_smoke: enabled", "acceptance_smoke: pending"));
  assert.match(f.run("set", setOptions("local_package"), 2).error, /acceptance_smoke/u);
  assert.match(f.run("check", [], 2).error, /acceptance_smoke/u);
});

test("duplicate field, stale expected choice and invalid value fail without writes", (t) => {
  const f = fixture(t, V3.replace("e2e_hint: disabled", "e2e_hint: disabled\ne2e_hint: disabled"));
  assert.match(f.run("set", setOptions("local_package"), 2).error, /重复/u);
  fs.writeFileSync(f.policy, V3);
  const before = fs.readFileSync(f.policy);
  assert.match(f.run("set", setOptions("unknown"), 2).error, /--action/u);
  assert.match(f.run("set", setOptions("push_release_branch", "local_package"), 2).error, /已变化/u);
  assert.ok(fs.readFileSync(f.policy).equals(before));
});

test("existing writer lock fails closed without changing the protected policy", (t) => {
  const f = fixture(t);
  const lock = path.join(f.docs, ".AGENT_POLICY.post-release.lock");
  fs.writeFileSync(lock, "another writer");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /正在被修改/u);
  assert.equal(fs.readFileSync(f.policy, "utf8"), V3);
  fs.rmSync(lock);
  assert.equal(f.run("set", setOptions("local_package")).post_release_action, "local_package");
});

test("CRLF policy remains CRLF and policy symlink is rejected", (t) => {
  const f = fixture(t, V3.replaceAll("\n", "\r\n"));
  f.run("set", setOptions("push_release_branch"));
  const written = fs.readFileSync(f.policy, "utf8");
  assert.equal(written.replaceAll("\r\n", "").includes("\n"), false);
  const real = path.join(f.docs, "actual.md");
  fs.renameSync(f.policy, real);
  try { fs.symlinkSync(real, f.policy); } catch (error) {
    if (process.platform === "win32" && error.code === "EPERM") { t.skip("symlink permission unavailable"); return; }
    throw error;
  }
  assert.match(f.run("check", [], 2).error, /普通非符号链接/u);
  assert.match(f.run("set", setOptions("local_package", "push_release_branch"), 2).error, /普通非符号链接/u);
  assert.ok(fs.readFileSync(real, "utf8").includes("post_release_action: push_release_branch"));
});

test("Harness source marker blocks setting a downstream choice", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Version.md"), "template");
  const derived = path.join(f.root, ".agents", "skills", "desktop-instantiate-project");
  fs.mkdirSync(derived, { recursive: true });
  fs.writeFileSync(path.join(derived, "SKILL.md"), "template");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /Harness 模板源/u);
  assert.equal(fs.readFileSync(f.policy, "utf8"), V3);
});

test("legacy policy body must be reconciled before a choice can be persisted", (t) => {
  const stale = V3.replace("发布后动作直接读取 `post_release_action`。", "推送与打包分别由发布后的用户请求决定。");
  const f = fixture(t, stale);
  assert.equal(f.run("inspect").status, "selection_required");
  const rejected = f.run("set", setOptions("local_package"), 2).error;
  assert.match(rejected, /正文尚未合并/u);
  assert.match(rejected, /缺少：发布后动作直接读取 `post_release_action`/u);
  assert.match(rejected, /过时：推送与打包分别由发布后的用户请求决定/u);
  assert.equal(fs.readFileSync(f.policy, "utf8"), stale);
  fs.writeFileSync(f.policy, V3);
  assert.equal(f.run("set", setOptions("local_package")).status, "configured");
  fs.writeFileSync(f.policy, fs.readFileSync(f.policy, "utf8").replace("发布后动作直接读取 `post_release_action`。", "发布后是否推送、是否打包只由用户各自的明确请求决定。"));
  assert.match(f.run("check", [], 2).error, /正文尚未合并/u);
});

test("legacy policy excluding default branch synchronization fails without writes", (t) => {
  const f = fixture(t);
  f.run("set", setOptions("push_release_branch"));
  const current = fs.readFileSync(f.policy, "utf8");
  for (const legacy of [
    current.replace("远端默认主分支、`release` 分支和 tag 同步到同一已发布 HEAD。", "只推 release 和 tag。"),
    ...["远端默认主分支不得因此改变", "不重新合并或改动远端默认主分支", "远端 advertised 默认分支不因该路径移动"].map((line) => `${current}\n${line}\n`),
  ]) {
    fs.writeFileSync(f.policy, legacy);
    assert.match(f.run("check", [], 2).error, /正文尚未合并/u);
    assert.equal(fs.readFileSync(f.policy, "utf8"), legacy);
  }
});

test("local package choice requires an existing CLI or supported GUI package route", (t) => {
  const f = fixture(t);
  const cargo = path.join(f.root, "Cargo.toml");
  fs.writeFileSync(cargo, "[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"tui\", \"mcp\"]\n");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /没有现有本地打包 Skill/u);
  assert.equal(f.run("set", setOptions("push_release_branch")).status, "configured");
  assert.match(f.run("set", setOptions("local_package", "push_release_branch"), 2).error, /没有现有本地打包 Skill/u);
  fs.writeFileSync(cargo, "[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \".\"\nrust-test-manifests = [\"src-tauri/Cargo.toml\"]\n");
  assert.match(f.run("set", setOptions("local_package", "push_release_branch"), 2).error, /GUI package.json/u);
  fs.mkdirSync(path.join(f.root, "src-tauri"));
  fs.writeFileSync(path.join(f.root, "package.json"), '{"devDependencies":{"@tauri-apps/cli":"2.0.0"}}\n');
  fs.writeFileSync(path.join(f.root, "src-tauri", "tauri.conf.json"), '{}\n');
  fs.writeFileSync(path.join(f.root, "src-tauri", "Cargo.toml"), "[package]\nname = \"example\"\nversion = \"0.1.0\"\n");
  assert.match(f.run("set", setOptions("local_package", "push_release_branch"), 2).error, /必须受 Git 跟踪/u);
  track(f, "Cargo.toml", "package.json", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json");
  assert.equal(f.run("set", setOptions("local_package", "push_release_branch")).status, "configured");
  fs.writeFileSync(path.join(f.root, "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  fs.writeFileSync(path.join(f.root, "src-tauri", "Cargo.lock"), "# local lock\n");
  assert.equal(f.run("check").status, "configured", "本地未跟踪锁文件不应改变动作选择");
  fs.writeFileSync(cargo, "[workspace]\nmembers = []\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \".\"\n");
  assert.match(f.run("check", [], 2).error, /空 workspace/u);
  fs.writeFileSync(cargo, "[workspace]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \".\"\n");
  assert.match(f.run("check", [], 2).error, /空 workspace/u);
  fs.writeFileSync(cargo, "[workspace]\nmembers = [\"src-tauri\"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \".\"\n");
  assert.equal(f.run("check").status, "configured");
});

test("mixed CLI and GUI local package choice still checks GUI readiness", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"cli\", \"gui\"]\ngui-root = \"app\"\ndependency-lock-policy = \"tracked\"\n");
  track(f, "Cargo.toml");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /GUI 根目录 不存在/u);
});

// GUI 根目录内的 Cargo 与 Tauri 配置必须成对，并拒绝同时存在两份配置。
test("gui_root_cargo_layout_is_supported_and_ambiguous_config_fails_closed", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \"app\"\nrust-test-manifests = [\"app/Cargo.toml\"]\n");
  fs.mkdirSync(path.join(f.root, "app"));
  fs.writeFileSync(path.join(f.root, "app", "package.json"), '{"devDependencies":{"@tauri-apps/cli":"2.0.0"}}\n');
  fs.writeFileSync(path.join(f.root, "app", "Cargo.toml"), "[package]\nname = \"example\"\nversion = \"0.1.0\"\n");
  fs.writeFileSync(path.join(f.root, "app", "tauri.conf.json"), '{}\n');
  track(f, "Cargo.toml", "app/package.json", "app/Cargo.toml", "app/tauri.conf.json");
  assert.equal(f.run("set", setOptions("local_package")).post_release_action, "local_package");
  assert.equal(f.run("check").status, "configured");
  fs.mkdirSync(path.join(f.root, "app", "src-tauri"));
  fs.writeFileSync(path.join(f.root, "app", "src-tauri", "Cargo.toml"), "[package]\nname = \"duplicate\"\n");
  fs.writeFileSync(path.join(f.root, "app", "src-tauri", "tauri.conf.json"), '{}\n');
  assert.match(f.run("check", [], 2).error, /只能存在一份/u);
  fs.rmSync(path.join(f.root, "app", "src-tauri", "tauri.conf.json"));
  fs.rmSync(path.join(f.root, "app", "Cargo.toml"));
  assert.match(f.run("check", [], 2).error, /GUI Rust Cargo 清单 不存在/u, "根配置不能与 src-tauri Cargo 交叉配对");
});

// 前端清单与选定的 Rust 清单必须逐路径受跟踪，不能由目录名通配命中替代。
test("gui_manifests_must_be_exactly_git_tracked", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), "[workspace]\nmembers = [\"gui[1]\"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \"gui[1]\"\nrust-test-manifests = [\"gui[1]/Cargo.toml\"]\n");
  fs.mkdirSync(path.join(f.root, "gui[1]"));
  fs.writeFileSync(path.join(f.root, "gui[1]", "package.json"), '{"devDependencies":{"@tauri-apps/cli":"2.0.0"}}\n');
  fs.writeFileSync(path.join(f.root, "gui[1]", "Cargo.toml"), "[package]\nname = \"example\"\n");
  fs.writeFileSync(path.join(f.root, "gui[1]", "tauri.conf.json"), '{}\n');
  fs.mkdirSync(path.join(f.root, "gui1"));
  fs.writeFileSync(path.join(f.root, "gui1", "package.json"), "{}\n");
  track(f, "Cargo.toml", "gui1/package.json");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /GUI package.json 必须受 Git 跟踪/u);
  track(f, "gui[1]/package.json");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /GUI Rust Cargo 清单 必须受 Git 跟踪/u);
  track(f, "gui[1]/Cargo.toml");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /Tauri 配置 必须受 Git 跟踪/u);
  track(f, "gui[1]/tauri.conf.json");
  assert.equal(f.run("set", setOptions("local_package")).status, "configured");
});

// 选择了根布局时，配置链接仍必须经过路径守卫，不能绕过到外部文件。
test("gui_config_symlink_is_rejected", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \"app\"\nrust-test-manifests = [\"app/Cargo.toml\"]\n");
  fs.mkdirSync(path.join(f.root, "app"));
  fs.writeFileSync(path.join(f.root, "app", "package.json"), '{"devDependencies":{"@tauri-apps/cli":"2.0.0"}}\n');
  fs.writeFileSync(path.join(f.root, "app", "Cargo.toml"), "[package]\nname = \"example\"\n");
  try { fs.symlinkSync(path.join(f.root, "Cargo.toml"), path.join(f.root, "app", "tauri.conf.json")); }
  catch (error) {
    if (process.platform === "win32" && error.code === "EPERM") { t.skip("symlink permission unavailable"); return; }
    throw error;
  }
  track(f, "Cargo.toml", "app/package.json", "app/Cargo.toml", "app/tauri.conf.json");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /Tauri 配置 不得是符号链接/u);
});

// 显式跟踪锁策略必须落实到索引中的实际 Rust 与前端锁文件。
test("tracked_lock_policy_requires_gui_workspace_and_frontend_locks", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \"app\"\nrust-test-manifests = [\"Cargo.toml\"]\ndependency-lock-policy = \"tracked\"\n");
  fs.mkdirSync(path.join(f.root, "app"));
  fs.writeFileSync(path.join(f.root, "app", "package.json"), '{"devDependencies":{"@tauri-apps/cli":"2.0.0"}}\n');
  fs.writeFileSync(path.join(f.root, "app", "Cargo.toml"), "[package]\nname = \"example\"\nversion = \"0.1.0\"\n");
  fs.mkdirSync(path.join(f.root, "app", "src"));
  fs.writeFileSync(path.join(f.root, "app", "src", "lib.rs"), "pub fn marker() {}\n");
  fs.writeFileSync(path.join(f.root, "app", "tauri.conf.json"), '{}\n');
  track(f, "Cargo.toml", "app/package.json", "app/Cargo.toml", "app/tauri.conf.json");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /Cargo.lock/u);
  fs.writeFileSync(path.join(f.root, "Cargo.lock"), "# test lock\n");
  fs.writeFileSync(path.join(f.root, "app", "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /Cargo.lock/u);
  track(f, "Cargo.lock");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /pnpm-lock.yaml/u);
  track(f, "app/pnpm-lock.yaml");
  assert.equal(f.run("set", setOptions("local_package")).status, "configured");
  fs.rmSync(path.join(f.root, "app", "pnpm-lock.yaml"));
  assert.match(f.run("check", [], 2).error, /pnpm-lock.yaml/u);
});

test("tracked GUI package checks its independent Cargo workspace even when absent from test manifests", (t) => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, "Cargo.toml"), "[workspace]\nmembers = [\"core\"]\n[workspace.metadata.agent-first-harness]\ntarget-platforms = [\"macos\"]\ninterfaces = [\"gui\"]\ngui-root = \"app\"\nrust-test-manifests = [\"Cargo.toml\"]\ndependency-lock-policy = \"tracked\"\n");
  fs.mkdirSync(path.join(f.root, "core", "src"), { recursive: true });
  fs.writeFileSync(path.join(f.root, "core", "Cargo.toml"), "[package]\nname = \"core\"\nversion = \"0.1.0\"\n");
  fs.writeFileSync(path.join(f.root, "core", "src", "lib.rs"), "pub fn marker() {}\n");
  fs.mkdirSync(path.join(f.root, "app"));
  fs.writeFileSync(path.join(f.root, "app", "package.json"), '{"devDependencies":{"@tauri-apps/cli":"2.0.0"}}\n');
  fs.writeFileSync(path.join(f.root, "app", "Cargo.toml"), "[package]\nname = \"example\"\nversion = \"0.1.0\"\n[workspace]\nmembers = []\n");
  fs.mkdirSync(path.join(f.root, "app", "src"));
  fs.writeFileSync(path.join(f.root, "app", "src", "lib.rs"), "pub fn marker() {}\n");
  fs.writeFileSync(path.join(f.root, "app", "tauri.conf.json"), "{}\n");
  fs.writeFileSync(path.join(f.root, "Cargo.lock"), "# root lock\n");
  fs.writeFileSync(path.join(f.root, "app", "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  track(f, "Cargo.toml", "Cargo.lock", "core/Cargo.toml", "core/src/lib.rs", "app/package.json", "app/Cargo.toml", "app/src/lib.rs", "app/tauri.conf.json", "app/pnpm-lock.yaml");
  assert.match(f.run("set", setOptions("local_package"), 2).error, /app\/Cargo.lock/u);
  fs.writeFileSync(path.join(f.root, "app", "Cargo.lock"), "# GUI lock\n");
  track(f, "app/Cargo.lock");
  assert.equal(f.run("set", setOptions("local_package")).status, "configured");
});

// CLI 本地打包也必须遵守显式跟踪锁策略，默认未选择时维持旧行为。
test("tracked_lock_policy_requires_cli_workspace_lock", (t) => {
  const f = fixture(t);
  const cargo = path.join(f.root, "Cargo.toml");
  fs.writeFileSync(cargo, fs.readFileSync(cargo, "utf8") + 'dependency-lock-policy = "tracked"\n');
  assert.match(f.run("set", setOptions("local_package"), 2).error, /Cargo.lock/u);
  fs.writeFileSync(path.join(f.root, "Cargo.lock"), "# test lock\n");
  track(f, "Cargo.lock");
  assert.equal(f.run("set", setOptions("local_package")).status, "configured");
});
