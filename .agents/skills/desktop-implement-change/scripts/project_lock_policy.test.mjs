import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import { assertDependencyLocks, readDependencyLockPolicy } from "./project_lock_policy.mjs";

function fixture(callback) {
  const root = mkdtempSync(path.join(tmpdir(), "afh-lock-policy-"));
  try {
    writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n");
    callback(root);
  } finally { rmSync(root, { recursive: true, force: true }); }
}

function git(root, ...args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, `${args.join(" ")}: ${result.stderr}`);
}

function trackedFixture(root, { gui = true, nested = false } = {}) {
  git(root, "init", "-q");
  mkdirSync(path.join(root, "app"));
  writeFileSync(path.join(root, "app", "Cargo.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[lib]\npath = \"lib.rs\"\n");
  writeFileSync(path.join(root, "app", "lib.rs"), "pub fn fixture() {}\n");
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\nmembers = [\"app\"]\n[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n");
  writeFileSync(path.join(root, "Cargo.lock"), "# root lock\n");
  if (gui) writeFileSync(path.join(root, "app", "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  if (nested) {
    mkdirSync(path.join(root, "nested"));
    writeFileSync(path.join(root, "nested", "Cargo.toml"), "[workspace]\nmembers = [\"member\"]\n");
    writeFileSync(path.join(root, "nested", "Cargo.lock"), "# nested lock\n");
    mkdirSync(path.join(root, "nested", "member"));
    writeFileSync(path.join(root, "nested", "member", "Cargo.toml"), "[package]\nname = \"member\"\nversion = \"0.1.0\"\n[lib]\npath = \"lib.rs\"\n");
    writeFileSync(path.join(root, "nested", "member", "lib.rs"), "pub fn fixture() {}\n");
  }
  git(root, "add", "--", "Cargo.toml", "Cargo.lock", "app/Cargo.toml", "app/lib.rs", ...(gui ? ["app/pnpm-lock.yaml"] : []),
    ...(nested ? ["nested/Cargo.toml", "nested/Cargo.lock", "nested/member/Cargo.toml", "nested/member/lib.rs"] : []));
}

test("defaults to ignored and accepts explicit ignored or tracked", () => fixture((root) => {
  assert.equal(readDependencyLockPolicy(root), "ignored");
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace.metadata.agent-first-harness]\ndependency-lock-policy = 'ignored' # local default\n");
  assert.equal(readDependencyLockPolicy(root), "ignored");
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\" # opt in\n");
  assert.equal(readDependencyLockPolicy(root), "tracked");
}));

test("recognizes TOML-equivalent quoted tables and dotted policy keys", () => fixture((root) => {
  const manifest = path.join(root, "Cargo.toml");
  for (const source of [
    "[workspace.metadata.\"agent-first-harness\"]\ndependency-lock-policy = \"tracked\"\n",
    "[workspace.metadata.'agent-first-harness']\n'dependency-lock-policy' = \"tracked\"\n",
    "[\"workspace\".\"metadata\".\"agent-first-harness\"]\n\"dependency-lock-policy\" = \"tracked\"\n",
    "[workspace.metadata]\n\"agent-first-harness\".dependency-lock-policy = \"tracked\"\n",
    "workspace.metadata.\"agent-first-harness\".dependency-lock-policy = \"tracked\"\n",
    "[workspace.metadata.\"agent-first-harness\"]\n\"dependency-\\U0000006cock-policy\" = \"tracked\"\n",
    "[\"work\\U00000073pace\".metadata.\"agent-first-harness\"]\ndependency-lock-policy = \"tracked\"\n",
  ]) {
    writeFileSync(manifest, source);
    assert.equal(readDependencyLockPolicy(root), "tracked", source);
  }
}));

test("ambiguous inline or misplaced policy assignments fail closed", () => fixture((root) => {
  const manifest = path.join(root, "Cargo.toml");
  for (const source of [
    "[workspace.metadata]\n\"agent-first-harness\" = { dependency-lock-policy = \"tracked\" }\n",
    "[workspace.metadata]\n\"agent-first-harness\" = { \"dependency-\\U0000006cock-policy\" = \"tracked\" }\n",
    "workspace = { metadata = { \"agent-first-harness\" = { \"dependency-lock-policy\" = \"tracked\" } } }\n",
    "workspace = { metadata = { \"agent-first-harness\" = { \"dependency-\\U0000006cock-policy\" = \"tracked\" } } }\n",
    "[package]\ndependency-lock-policy = \"tracked\"\n",
  ]) {
    writeFileSync(manifest, source);
    assert.throws(() => readDependencyLockPolicy(root), /dependency-lock-policy/u, source);
  }
  writeFileSync(manifest, "[package]\ndescription = \"dependency-lock-policy = tracked\"\n");
  assert.equal(readDependencyLockPolicy(root), "ignored");
}));

test("rejects unknown, nonstring, duplicate, and malformed policy values", () => fixture((root) => {
  const manifest = path.join(root, "Cargo.toml");
  for (const value of ["true", "0", "[\"tracked\"]", "\"freeform\"", "\"tracked\" suffix", ""]) {
    writeFileSync(manifest, `[workspace.metadata.agent-first-harness]\ndependency-lock-policy = ${value}\n`);
    assert.throws(() => readDependencyLockPolicy(root), /dependency-lock-policy/u, value);
  }
  writeFileSync(manifest, "[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\ndependency-lock-policy = \"ignored\"\n");
  assert.throws(() => readDependencyLockPolicy(root), /重复/u);
  writeFileSync(manifest, "[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n[workspace.metadata.agent-first-harness]\n");
  assert.throws(() => readDependencyLockPolicy(root), /重复/u);
}));

test("ignores apparent policy tables inside TOML comments and multiline strings", () => fixture((root) => {
  writeFileSync(path.join(root, "Cargo.toml"), "# [workspace.metadata.agent-first-harness]\n[package]\ndescription = '''\n[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n'''\n");
  assert.equal(readDependencyLockPolicy(root), "ignored");
}));

test("ignored mode never requires Git or lock files", () => fixture((root) => {
  assert.deepEqual(assertDependencyLocks(root, { guiRoot: "app" }), { policy: "ignored", locks: [] });
}));

test("tracked mode accepts member manifests using the root workspace lock", () => fixture((root) => {
  trackedFixture(root);
  assert.deepEqual(assertDependencyLocks(root, { rustTestManifests: ["app/Cargo.toml"], guiRoot: "app" }), {
    policy: "tracked", locks: ["Cargo.lock", "app/pnpm-lock.yaml"],
  });
}));

test("tracked mode accepts a GUI whose package root is the project root", () => fixture((root) => {
  trackedFixture(root, { gui: false });
  writeFileSync(path.join(root, "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  git(root, "add", "--", "pnpm-lock.yaml");
  assert.deepEqual(assertDependencyLocks(root, { guiRoot: "." }), {
    policy: "tracked", locks: ["Cargo.lock", "pnpm-lock.yaml"],
  });
}));

test("tracked mode requires each independent workspace lock and omits GUI lock without a GUI", () => fixture((root) => {
  trackedFixture(root, { gui: false, nested: true });
  assert.deepEqual(assertDependencyLocks(root, { rustTestManifests: ["app/Cargo.toml", "nested/member/Cargo.toml"] }), {
    policy: "tracked", locks: ["Cargo.lock", "nested/Cargo.lock"],
  });
}));

test("tracked mode requires a separate lock for a quoted independent workspace", () => fixture((root) => {
  trackedFixture(root, { gui: false, nested: true });
  const nestedManifest = path.join(root, "nested", "Cargo.toml");
  const manifests = ["app/Cargo.toml", "nested/member/Cargo.toml"];
  for (const header of ['["workspace"]', "['workspace']", '["work\\U00000073pace"]']) {
    writeFileSync(nestedManifest, `${header} # independent workspace\nmembers = ["member"]\n`);
    assert.deepEqual(assertDependencyLocks(root, { rustTestManifests: manifests }), {
      policy: "tracked", locks: ["Cargo.lock", "nested/Cargo.lock"],
    }, header);
  }
  writeFileSync(nestedManifest, '["workspace"]\nmembers = ["member"]\n');
  rmSync(path.join(root, "nested", "Cargo.lock"));
  assert.throws(() => assertDependencyLocks(root, { rustTestManifests: manifests }), /nested\/Cargo.lock/u);
}));

for (const [label, declaration] of [
  ["dotted key", 'workspace.members = ["member"]\n'],
  ["quoted dotted key", '"work\\U00000073pace".members = ["member"]\n'],
  ["inline table", 'workspace = { members = ["member"] }\n'],
]) {
  test(`tracked mode requires a separate lock for an independent workspace ${label}`, () => fixture((root) => {
    trackedFixture(root, { gui: false, nested: true });
    writeFileSync(path.join(root, "nested", "Cargo.toml"), declaration);
    const manifests = ["app/Cargo.toml", "nested/member/Cargo.toml"];
    assert.deepEqual(assertDependencyLocks(root, { rustTestManifests: manifests }), {
      policy: "tracked", locks: ["Cargo.lock", "nested/Cargo.lock"],
    });
    rmSync(path.join(root, "nested", "Cargo.lock"));
    assert.throws(() => assertDependencyLocks(root, { rustTestManifests: manifests }), /nested\/Cargo.lock/u);
  }));
}

test("tracked mode requires an implicit nested package workspace lock", () => fixture((root) => {
  trackedFixture(root, { gui: false });
  writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\nmembers = [\"app\"]\nexclude = [\"nested\"]\n[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n");
  mkdirSync(path.join(root, "nested"));
  writeFileSync(path.join(root, "nested", "Cargo.toml"), 'package = { name = "nested", version = "0.1.0" }\n[lib]\npath = "lib.rs"\n');
  writeFileSync(path.join(root, "nested", "lib.rs"), "pub fn fixture() {}\n");
  writeFileSync(path.join(root, "nested", "Cargo.lock"), "# implicit workspace lock\n");
  git(root, "add", "--", "Cargo.toml", "nested/Cargo.toml", "nested/lib.rs", "nested/Cargo.lock");
  assert.deepEqual(assertDependencyLocks(root, { rustTestManifests: ["nested/Cargo.toml"] }), {
    policy: "tracked", locks: ["nested/Cargo.lock"],
  });
  rmSync(path.join(root, "nested", "Cargo.lock"));
  assert.throws(() => assertDependencyLocks(root, { rustTestManifests: ["nested/Cargo.toml"] }), /nested\/Cargo.lock/u);
}));

for (const [label, packageDeclaration] of [
  ["package table", "[package]\nname = \"app\"\nversion = \"0.1.0\"\nworkspace = \"../other\"\n"],
  ["inline package", 'package = { name = "app", version = "0.1.0", workspace = "../other" }\n'],
]) {
  test(`tracked mode follows ${label} workspace to a sibling lock`, () => fixture((root) => {
    trackedFixture(root, { gui: false });
    writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\nmembers = []\nexclude = [\"app\", \"other\"]\n[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n");
    mkdirSync(path.join(root, "other"));
    writeFileSync(path.join(root, "other", "Cargo.toml"), "[workspace]\nmembers = [\"../app\"]\n");
    writeFileSync(path.join(root, "other", "Cargo.lock"), "# sibling workspace lock\n");
    writeFileSync(path.join(root, "app", "Cargo.toml"), `${packageDeclaration}[lib]\npath = "lib.rs"\n`);
    writeFileSync(path.join(root, "app", "lib.rs"), "pub fn fixture() {}\n");
    git(root, "add", "--", "Cargo.toml", "app/Cargo.toml", "other/Cargo.toml", "other/Cargo.lock");
    assert.deepEqual(assertDependencyLocks(root, { rustTestManifests: ["app/Cargo.toml"] }), {
      policy: "tracked", locks: ["other/Cargo.lock"],
    });
    rmSync(path.join(root, "other", "Cargo.lock"));
    assert.throws(() => assertDependencyLocks(root, { rustTestManifests: ["app/Cargo.toml"] }), /other\/Cargo.lock/u);
  }));
}

test("tracked mode rejects package.workspace paths outside the project", () => fixture((root) => {
  trackedFixture(root, { gui: false });
  const outside = mkdtempSync(path.join(tmpdir(), "afh-outside-workspace-"));
  try {
    const workspacePath = path.relative(path.join(root, "app"), outside).split(path.sep).join("/");
    const memberPath = path.relative(outside, path.join(root, "app")).split(path.sep).join("/");
    writeFileSync(path.join(root, "Cargo.toml"), "[workspace]\nmembers = []\nexclude = [\"app\"]\n[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n");
    writeFileSync(path.join(outside, "Cargo.toml"), `[workspace]\nmembers = ["${memberPath}"]\n`);
    writeFileSync(path.join(root, "app", "Cargo.toml"), `[package]\nname = "app"\nversion = "0.1.0"\nworkspace = "${workspacePath}"\n[lib]\npath = "lib.rs"\n`);
    assert.throws(() => assertDependencyLocks(root, { rustTestManifests: ["app/Cargo.toml"] }), /Cargo workspace 根不得离开项目/u);
  } finally { rmSync(outside, { recursive: true, force: true }); }
}));

test("tracked mode rejects missing, untracked, ignored, and symbolic-link locks", { skip: process.platform === "win32" }, () => fixture((root) => {
  trackedFixture(root);
  rmSync(path.join(root, "Cargo.lock"));
  assert.throws(() => assertDependencyLocks(root, { guiRoot: "app" }), /Cargo.lock/u);
  writeFileSync(path.join(root, "Cargo.lock"), "# replacement\n");
  rmSync(path.join(root, "app", "pnpm-lock.yaml"));
  writeFileSync(path.join(root, "app", "pnpm-lock.yaml"), "# replacement\n");
  git(root, "rm", "--cached", "-f", "-q", "--", "app/pnpm-lock.yaml");
  assert.throws(() => assertDependencyLocks(root, { guiRoot: "app" }), /必须受 Git 跟踪.*pnpm-lock.yaml/u);
  git(root, "add", "--", "app/pnpm-lock.yaml");
  writeFileSync(path.join(root, ".gitignore"), "Cargo.lock\n");
  assert.throws(() => assertDependencyLocks(root, { guiRoot: "app" }), /不得被 Git 忽略.*Cargo.lock/u);
  rmSync(path.join(root, ".gitignore"));
  rmSync(path.join(root, "Cargo.lock"));
  symlinkSync(path.join(root, "app", "pnpm-lock.yaml"), path.join(root, "Cargo.lock"));
  assert.throws(() => assertDependencyLocks(root, { guiRoot: "app" }), /不得是符号链接.*Cargo.lock/u);
}));

test("tracked mode rejects a symlink staged in Git after the worktree file becomes regular", { skip: process.platform === "win32" }, () => fixture((root) => {
  trackedFixture(root, { gui: false });
  rmSync(path.join(root, "Cargo.lock"));
  symlinkSync("app/Cargo.toml", path.join(root, "Cargo.lock"));
  git(root, "add", "--", "Cargo.lock");
  rmSync(path.join(root, "Cargo.lock"));
  writeFileSync(path.join(root, "Cargo.lock"), "# ordinary worktree file\n");
  assert.throws(() => assertDependencyLocks(root), /Git 索引必须是唯一的 stage-0 普通文件.*Cargo.lock/u);
}));

test("tracked mode rejects a symlink index mode with ordinary worktree bytes on every platform", () => fixture((root) => {
  trackedFixture(root, { gui: false });
  const linkTarget = path.join(root, "link-target.txt");
  writeFileSync(linkTarget, "app/Cargo.toml");
  const objectId = spawnSync("git", ["-C", root, "hash-object", "-w", linkTarget], { encoding: "utf8" });
  assert.equal(objectId.status, 0, objectId.stderr);
  git(root, "update-index", "--cacheinfo", `120000,${objectId.stdout.trim()},Cargo.lock`);
  assert.throws(() => assertDependencyLocks(root), /Git 索引必须是唯一的 stage-0 普通文件.*Cargo.lock/u);
}));

test("tracked mode refuses a nested checkout as the project root", () => fixture((root) => {
  trackedFixture(root);
  writeFileSync(path.join(root, "app", "Cargo.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[workspace.metadata.agent-first-harness]\ndependency-lock-policy = \"tracked\"\n");
  assert.throws(() => assertDependencyLocks(path.join(root, "app"), { rustTestManifests: ["Cargo.toml"] }), /Git 顶层/u);
}));
