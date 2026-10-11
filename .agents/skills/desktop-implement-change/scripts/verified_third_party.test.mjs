import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, test } from "node:test";

import { parseThirdPartyManifest, verifyRegisteredThirdParty } from "./verified_third_party.mjs";

const source = fileURLToPath(new URL("../../../../vendor/accesskit_macos/", import.meta.url));
const directory = "vendor/accesskit_macos";
const manifest = `${directory}/source-content.sha256`;
const pinnedDigest = "f4bc40427a5db1b6acf7262cf8d4bcdfe25c19b6a16499302252a3942e86ffda";
let root;

function installSource() {
  const payload = readFileSync(path.join(source, "source-content.sha256"));
  assert.equal(createHash("sha256").update(payload).digest("hex"), pinnedDigest);
  const entries = parseThirdPartyManifest(payload);
  for (const relative of ["source-content.sha256", ...entries.map((entry) => entry.relative)]) {
    const target = path.join(root, directory, relative);
    mkdirSync(path.dirname(target), { recursive: true });
    copyFileSync(path.join(source, relative), target);
  }
  return entries.map((entry) => `${directory}/${entry.relative}`).sort();
}

function assertRejected(pattern) {
  const result = verifyRegisteredThirdParty(root);
  assert.deepEqual(result.files, []);
  assert.ok(result.errors.some((error) => pattern.test(error)), JSON.stringify(result));
}

/** GPUI 清单相对 immutable upstream；shim 和补丁绝不进入排除集合。 */
function installGpuiSource(component = "vendor/gpui-pre") {
  const sourceRoot = fileURLToPath(new URL(`../../../../${component}/`, import.meta.url));
  const payload = readFileSync(path.join(sourceRoot, "source-content.sha256"));
  const entries = parseThirdPartyManifest(payload);
  const targetRoot = path.join(root, component);
  mkdirSync(targetRoot, { recursive: true });
  copyFileSync(path.join(sourceRoot, "source-content.sha256"), path.join(targetRoot, "source-content.sha256"));
  for (const entry of entries) {
    const relative = `upstream/${entry.relative}`;
    const target = path.join(targetRoot, relative);
    mkdirSync(path.dirname(target), { recursive: true });
    copyFileSync(path.join(sourceRoot, relative), target);
  }
  return entries.map((entry) => `${component}/upstream/${entry.relative}`).sort();
}

beforeEach(() => { root = mkdtempSync(path.join(tmpdir(), "afh-third-party-")); });
afterEach(() => rmSync(root, { recursive: true, force: true }));

test("an absent registered component yields no exclusions or errors", () => {
  mkdirSync(path.join(root, "vendor/another_component"), { recursive: true });
  assert.deepEqual(verifyRegisteredThirdParty(root), { files: [], errors: [] });
});

test("only the exact pinned manifest and all 18 original files are verified", () => {
  const expected = installSource();
  assert.equal(expected.length, 18);
  assert.deepEqual(verifyRegisteredThirdParty(root), { files: expected, errors: [] });
});

test("GPUI registration verifies its nested source root and excludes no maintained shim", () => {
  const expected = installGpuiSource();
  mkdirSync(path.join(root, "vendor/gpui-pre/src"));
  writeFileSync(path.join(root, "vendor/gpui-pre/src/lib.rs"), "// maintained shim\n");
  assert.equal(expected.length, 160);
  assert.deepEqual(verifyRegisteredThirdParty(root), { files: expected, errors: [] });
});

test("macOS font registration verifies all 17 originals and excludes no helper or fixture", () => {
  const expected = installGpuiSource("vendor/gpui-pre-macos");
  for (const relative of ["src/variable_font.rs", "test-fixtures/font.ttf"]) {
    const target = path.join(root, "vendor/gpui-pre-macos", relative);
    mkdirSync(path.dirname(target), { recursive: true });
    writeFileSync(target, "maintained content\n");
  }
  assert.equal(expected.length, 17);
  assert.deepEqual(verifyRegisteredThirdParty(root), { files: expected, errors: [] });
});

test("macOS font source drift rejects all its exclusions while keeping other registrations", () => {
  const expected = installSource();
  installGpuiSource("vendor/gpui-pre-macos");
  writeFileSync(path.join(root, "vendor/gpui-pre-macos/upstream/src/text_system.rs"), "// changed\n");
  const result = verifyRegisteredThirdParty(root);
  assert.deepEqual(result.files, expected);
  assert.ok(result.errors.some((error) => /原样文件 SHA256 不符.*text_system\.rs/u.test(error)));
});

test("a modified GPUI source rejects its whole registration without affecting AccessKit", () => {
  const expected = installSource();
  installGpuiSource();
  writeFileSync(path.join(root, "vendor/gpui-pre/upstream/src/window.rs"), "// changed\n");
  const result = verifyRegisteredThirdParty(root);
  assert.deepEqual(result.files, expected);
  assert.ok(result.errors.some((error) => /原样文件 SHA256 不符.*window\.rs/u.test(error)));
});

test("a symlink replacing GPUI upstream is rejected before source exclusion", () => {
  installGpuiSource();
  const target = path.join(root, "vendor/gpui-pre/upstream");
  const original = path.join(root, "original-gpui-upstream");
  renameSync(target, original);
  symlinkSync(original, target, "dir");
  assertRejected(/父目录不得是符号链接.*upstream/);
});

test("rewriting a manifest hash cannot authorize modified upstream bytes", () => {
  installSource();
  const relative = "upstream/src/node.rs";
  const payload = Buffer.from("// changed upstream\n");
  writeFileSync(path.join(root, directory, relative), payload);
  const rewritten = readFileSync(path.join(root, manifest), "utf8")
    .replace(/^[a-f0-9]{64}  upstream\/src\/node\.rs$/mu, `${createHash("sha256").update(payload).digest("hex")}  ${relative}`);
  writeFileSync(path.join(root, manifest), rewritten);
  assertRejected(/清单 SHA256/);
});

test("modification of one original file rejects the complete registration", () => {
  installSource();
  writeFileSync(path.join(root, directory, "upstream/src/node.rs"), "// modified\n");
  assertRejected(/原样文件 SHA256 不符.*node\.rs/);
});

test("a missing original file rejects the complete registration", () => {
  installSource();
  unlinkSync(path.join(root, directory, "licenses/LICENSE-MIT"));
  assertRejected(/缺失.*LICENSE-MIT/);
});

test("a present component without its registered manifest fails closed", () => {
  installSource();
  unlinkSync(path.join(root, manifest));
  assertRejected(/缺失.*source-content\.sha256/);
});

test("an original file replaced by a directory is rejected before reading", () => {
  installSource();
  const target = path.join(root, directory, "upstream/src/node.rs");
  unlinkSync(target);
  mkdirSync(target);
  assertRejected(/不是普通文件.*node\.rs/);
});

test("file symlinks are rejected even when their target contains original bytes", () => {
  installSource();
  const target = path.join(root, directory, "upstream/src/node.rs");
  const original = path.join(root, "original-node.rs");
  renameSync(target, original);
  symlinkSync(original, target);
  assertRejected(/符号链接.*node\.rs/);
});

test("a symlink in an original file parent directory is rejected", () => {
  installSource();
  const target = path.join(root, directory, "upstream");
  const original = path.join(root, "original-upstream");
  renameSync(target, original);
  symlinkSync(original, target, "dir");
  assertRejected(/父目录不得是符号链接.*upstream/);
});

test("a symlink in the registered component parent is rejected", () => {
  installSource();
  const target = path.join(root, "vendor");
  const original = path.join(root, "original-vendor");
  renameSync(target, original);
  symlinkSync(original, target, "dir");
  assertRejected(/父目录不得是符号链接/);
});

test("the manifest itself cannot be a symlink", () => {
  installSource();
  const target = path.join(root, manifest);
  const original = path.join(root, "original-manifest");
  renameSync(target, original);
  symlinkSync(original, target);
  assertRejected(/符号链接.*source-content\.sha256/);
});

test("manifest parsing rejects duplicate entries and all path escape forms", () => {
  const hash = "a".repeat(64);
  assert.throws(() => parseThirdPartyManifest(Buffer.from(`${hash}  upstream/src/node.rs\n`.repeat(2))), /重复路径/);
  for (const relative of ["../outside.rs", "/outside.rs", "upstream/../../outside.rs", "upstream/./node.rs", "upstream//node.rs", "C:/outside.rs", "upstream\\node.rs"]) {
    assert.throws(() => parseThirdPartyManifest(Buffer.from(`${hash}  ${relative}\n`)), /路径越界或不规范/);
  }
});

test("a manifest appended with a duplicate or escaped path cannot be approved", () => {
  installSource();
  const target = path.join(root, manifest);
  const original = readFileSync(target);
  for (const relative of ["upstream/src/node.rs", "../outside.rs"]) {
    writeFileSync(target, Buffer.concat([original, Buffer.from(`${"a".repeat(64)}  ${relative}\n`)]));
    assertRejected(/清单 SHA256/);
  }
});
