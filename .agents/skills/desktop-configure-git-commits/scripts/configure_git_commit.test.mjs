#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const SCRIPT = fileURLToPath(new URL("./configure_git_commit.mjs", import.meta.url));
const SOURCE_TEMPLATE = fileURLToPath(new URL("../assets/commit-template.txt", import.meta.url));

function fixture(t) {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "configure-git-commit-"));
  t.after(() => fs.rmSync(temporary, { recursive: true, force: true }));
  const root = path.join(temporary, "project");
  fs.mkdirSync(root);
  const globalConfig = path.join(temporary, "global.gitconfig");
  const env = { ...process.env, GIT_CONFIG_GLOBAL: globalConfig, GIT_CONFIG_NOSYSTEM: "1" };
  const initialized = spawnSync("git", ["-C", root, "init", "--quiet", "--initial-branch=main"], { encoding: "utf8", env });
  assert.equal(initialized.status, 0, initialized.stderr);
  return { root, globalConfig, env };
}

function git(ctx, ...args) { return spawnSync("git", ["-C", ctx.root, ...args], { encoding: "utf8", env: ctx.env }); }
function run(ctx, ...args) { return spawnSync(process.execPath, [SCRIPT, ...args], { encoding: "utf8", env: ctx.env }); }
function command(ctx, name, ...extra) { return run(ctx, name, "--project-root", ctx.root, ...extra); }
function gitValues(ctx, key) {
  const result = git(ctx, "config", "--local", "--get-all", key);
  if (result.status === 1) return [];
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trimEnd() ? result.stdout.trimEnd().split(/\r?\n/) : [];
}
function setLocal(ctx, key, value) { const result = git(ctx, "config", "--local", key, value); assert.equal(result.status, 0, result.stderr); }

test("install_configures_local_template_and_check_passes", (t) => {
  const ctx = fixture(t);
  const result = command(ctx, "install");
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.deepEqual(fs.readFileSync(payload.templatePath), fs.readFileSync(SOURCE_TEMPLATE));
  assert.deepEqual(gitValues(ctx, "commit.template"), [payload.templatePath]);
  assert.deepEqual(gitValues(ctx, "commit.cleanup"), ["strip"]);
  assert.deepEqual(gitValues(ctx, "commit.verbose"), ["true"]);
  assert.deepEqual(gitValues(ctx, "core.commentChar"), ["#"]);
  const checked = command(ctx, "check");
  assert.equal(checked.status, 0, checked.stderr);
  assert.equal(JSON.parse(checked.stdout).status, "ok");
});

test("repeated_install_is_idempotent", (t) => {
  const ctx = fixture(t);
  assert.equal(command(ctx, "install").status, 0);
  const second = command(ctx, "install");
  assert.equal(second.status, 0, second.stderr);
  assert.equal(JSON.parse(second.stdout).changed, false);
  assert.equal(gitValues(ctx, "commit.template").length, 1);
});

test("conflicting_setting_fails_without_mutation", (t) => {
  const ctx = fixture(t);
  setLocal(ctx, "commit.template", "/existing/template.txt");
  const result = command(ctx, "install");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /conflicting local Git settings/);
  assert.deepEqual(gitValues(ctx, "commit.template"), ["/existing/template.txt"]);
  const common = git(ctx, "rev-parse", "--git-common-dir").stdout.trim();
  assert.equal(fs.existsSync(path.join(ctx.root, common, "harness")), false);
});

test("replace_requires_flag_and_normalizes_all_settings", (t) => {
  const ctx = fixture(t);
  setLocal(ctx, "commit.template", "/existing/template.txt"); setLocal(ctx, "commit.cleanup", "verbatim");
  setLocal(ctx, "commit.verbose", "false"); setLocal(ctx, "core.commentChar", ";");
  const result = command(ctx, "install", "--replace");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).replacedConflicts, true);
  assert.deepEqual(gitValues(ctx, "commit.cleanup"), ["strip"]);
  assert.deepEqual(gitValues(ctx, "commit.verbose"), ["true"]);
  assert.deepEqual(gitValues(ctx, "core.commentChar"), ["#"]);
});

test("check_rejects_tampered_installed_template", (t) => {
  const ctx = fixture(t);
  const installed = command(ctx, "install");
  assert.equal(installed.status, 0, installed.stderr);
  fs.writeFileSync(JSON.parse(installed.stdout).templatePath, "# tampered\n");
  const checked = command(ctx, "check");
  assert.equal(checked.status, 1);
  assert.match(checked.stderr, /differs from the tracked source/);
});

test("nested_directory_is_not_accepted_as_project_root", (t) => {
  const ctx = fixture(t);
  const nested = path.join(ctx.root, "nested"); fs.mkdirSync(nested);
  const result = run(ctx, "install", "--project-root", nested);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /must equal the independent Git top level/);
});

test("identity_bootstrap_writes_only_repository_local_missing_fields", (t) => {
  const ctx = fixture(t);
  const result = command(ctx, "identity-bootstrap", "--fallback-username", "DeviceUser");
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.changed, true);
  assert.deepEqual(gitValues(ctx, "user.name"), ["DeviceUser"]);
  assert.deepEqual(gitValues(ctx, "user.email"), ["DeviceUser@gmail.com"]);
  assert.deepEqual(payload.identity.writtenLocalKeys, ["user.email", "user.name"]);
  assert.equal(payload.identity.name.scope, "local");
  assert.equal(fs.existsSync(ctx.globalConfig), false);
});

test("identity_bootstrap_preserves_complete_effective_global_identity", (t) => {
  const ctx = fixture(t);
  assert.equal(git(ctx, "config", "--global", "user.name", "Existing User").status, 0);
  assert.equal(git(ctx, "config", "--global", "user.email", "existing@example.com").status, 0);
  const before = fs.readFileSync(ctx.globalConfig);
  const result = command(ctx, "identity-bootstrap");
  assert.equal(result.status, 0, result.stderr);
  const payload = JSON.parse(result.stdout);
  assert.equal(payload.changed, false);
  assert.deepEqual(gitValues(ctx, "user.name"), []);
  assert.equal(payload.identity.name.scope, "global");
  assert.deepEqual(fs.readFileSync(ctx.globalConfig), before);
});

test("identity_bootstrap_fills_only_missing_email", (t) => {
  const ctx = fixture(t);
  assert.equal(git(ctx, "config", "--global", "user.name", "Existing User").status, 0);
  const result = command(ctx, "identity-bootstrap", "--fallback-username", "DeviceUser");
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(gitValues(ctx, "user.name"), []);
  assert.deepEqual(gitValues(ctx, "user.email"), ["DeviceUser@gmail.com"]);
});

test("identity_bootstrap_rejects_untranslated_or_non_slug_username", (t) => {
  const ctx = fixture(t);
  for (const username of ["设备用户", "Device User"]) {
    const result = command(ctx, "identity-bootstrap", "--fallback-username", username);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /ASCII English device username/);
    assert.deepEqual(gitValues(ctx, "user.name"), []);
  }
});

test("identity_report_and_check_are_read_only", (t) => {
  const ctx = fixture(t);
  const report = command(ctx, "identity-report");
  assert.equal(report.status, 0, report.stderr);
  assert.equal(JSON.parse(report.stdout).status, "missing");
  const missing = command(ctx, "identity-check");
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /missing effective Git identity/);
  assert.equal(command(ctx, "identity-bootstrap", "--fallback-username", "DeviceUser").status, 0);
  const checked = command(ctx, "identity-check");
  assert.equal(checked.status, 0, checked.stderr);
  assert.equal(JSON.parse(checked.stdout).identity.email.value, "DeviceUser@gmail.com");
});
