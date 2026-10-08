#!/usr/bin/env node

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const SCRIPT = fileURLToPath(new URL("./rename_project_identity.mjs", import.meta.url));

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rename-project-identity-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return root;
}

function runScript(root, ...extra) {
  return spawnSync(process.execPath, [SCRIPT, "--root", root,
    "--old-display-name-zh", "旧产品", "--new-display-name-zh", "新产品",
    "--old-display-name-en", "Old Product", "--new-display-name-en", "New Product",
    "--old-id", "old_product", "--new-id", "new_product",
    "--old-kebab", "old-product", "--new-kebab", "new-product", ...extra],
  { encoding: "utf8" });
}

test("preview_then_apply_renames_content_paths_and_licenses", (t) => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, "LICENSE.zh-CN.md"), "旧产品 old_product");
  fs.writeFileSync(path.join(root, "LICENSE.en.md"), "Old Product old_product");
  const skill = path.join(root, ".agents", "skills", "old-product-tool");
  fs.mkdirSync(skill, { recursive: true });
  const source = path.join(skill, "old_product.toml");
  fs.writeFileSync(source, "name = 'old_product' # Old Product", { mode: 0o744 });
  const preview = runScript(root);
  assert.equal(preview.status, 0, preview.stderr);
  assert.equal(JSON.parse(preview.stdout).mode, "preview");
  assert.match(fs.readFileSync(source, "utf8"), /Old Product/);
  const applied = runScript(root, "--apply");
  assert.equal(applied.status, 0, applied.stderr);
  assert.deepEqual(JSON.parse(applied.stdout).residuals, []);
  const renamed = path.join(root, ".agents", "skills", "new-product-tool", "new_product.toml");
  // Windows 不保留 POSIX 执行位；只有具备该位的平台才能复核权限随重命名保留。
  if (process.platform !== "win32") assert.ok(fs.statSync(renamed).mode & 0o100);
  assert.equal(fs.readFileSync(renamed, "utf8"), "name = 'new_product' # New Product");
  assert.equal(fs.readFileSync(path.join(root, "LICENSE.zh-CN.md"), "utf8"), "新产品 new_product");
  assert.equal(fs.readFileSync(path.join(root, "LICENSE.en.md"), "utf8"), "New Product new_product");
});

test("existing_destination_blocks_without_overwrite", (t) => {
  const root = fixture(t);
  const oldPath = path.join(root, "old_product.txt");
  const newPath = path.join(root, "new_product.txt");
  fs.writeFileSync(oldPath, "old");
  fs.writeFileSync(newPath, "new");
  const result = runScript(root, "--apply");
  assert.equal(result.status, 1);
  assert.equal(fs.readFileSync(oldPath, "utf8"), "old");
  assert.equal(fs.readFileSync(newPath, "utf8"), "new");
});

test("symbolic_link_blocks_before_write", (t) => {
  const root = fixture(t);
  const source = path.join(root, "source.txt");
  fs.writeFileSync(source, "Old Product");
  try { fs.symlinkSync(source, path.join(root, "linked.txt")); } catch (error) { return t.skip(error.message); }
  const result = runScript(root, "--apply");
  assert.equal(result.status, 1);
  assert.equal(fs.readFileSync(source, "utf8"), "Old Product");
});

test("explicit_root_rename_moves_project_without_overwrite", (t) => {
  const parent = fixture(t);
  const root = path.join(parent, "old_product");
  fs.mkdirSync(root);
  fs.writeFileSync(path.join(root, "README.md"), "旧产品 / Old Product");
  const result = runScript(root, "--rename-root", "--apply");
  assert.equal(result.status, 0, result.stderr);
  const destination = path.join(parent, "new_product");
  assert.equal(fs.existsSync(root), false);
  assert.equal(fs.readFileSync(path.join(destination, "README.md"), "utf8"), "新产品 / New Product");
});

test("dotdot_prefixed_in_root_name_is_not_mistaken_for_parent_escape", (t) => {
  const root = fixture(t);
  fs.writeFileSync(path.join(root, "..old_product.txt"), "Old Product");
  const result = runScript(root, "--apply");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(fs.readFileSync(path.join(root, "..new_product.txt"), "utf8"), "New Product");
});
