import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import {
  fakeExistingTools,
  fakeFrontendTools,
  fakeGitPackageManager,
  fakeNpmInstaller,
  makeNodeDist,
  makeRustDist,
  runGate,
} from "./environment_gates_fixture.mjs";

/** 以下用例执行 `development-environment-gates.sh`；Windows 侧由 environment_gates_windows.test.mjs 覆盖。 */
const posixOnly = { skip: process.platform === "win32" };

function withTemporaryRoot(callback) {
  const root = mkdtempSync(path.join(tmpdir(), "afh-env-install-"));
  try { callback(root); } finally { rmSync(root, { recursive: true, force: true }); }
}

function isolatedInstallOverrides(extra = {}) {
  return {
    AFH_ALLOW_FILE_URLS: "1",
    AFH_SKIP_PERSIST_PATH: "1",
    ...extra,
  };
}

test("missing Git is installed and reprobed", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { includeGit: false });
  fakeGitPackageManager(probe);
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: isolatedInstallOverrides(),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /gate\.git\.status=passed/);
  assert.match(result.stdout, /gate\.git\.change=installed/);
  assert.match(result.stdout, /gate\.node\.status=passed/);
}));

test("Node is installed for a non-GUI project while pnpm remains optional", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { includeNode: false });
  const nodeDist = makeNodeDist(path.join(root, "node-dist"));
  const result = runGate(root, ["--install-missing", "--interfaces", "CLI"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_NODE_DIST_BASE: nodeDist }),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /gate\.node\.status=passed/);
  assert.match(result.stdout, /gate\.node\.change=installed/);
  assert.match(result.stdout, /gate\.pnpm\.status=not-required/);
  assert.equal(existsSync(path.join(root, "home", ".local", "bin", "pnpm")), false);
}));

test("lower Rust is upgraded and reprobed", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { rust: "1.98.0" });
  const rustDist = makeRustDist(path.join(root, "rust-dist"));
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_RUSTUP_DIST_BASE: rustDist }),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /gate\.rust\.status=passed/);
  assert.match(result.stdout, /gate\.rust\.change=upgraded/);
  assert.match(result.stdout, /gate\.rust\.version=rustc 1\.98\.1/);
}));

test("Rust upgrade that remains below MSRV fails closed", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { rust: "1.98.0" });
  const rustDist = makeRustDist(path.join(root, "rust-dist"), { installedVersion: "1.98.0" });
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_RUSTUP_DIST_BASE: rustDist }),
  });
  assert.equal(result.status, 22, result.stdout);
  assert.match(result.stderr, /仍低于 MSRV/);
}));

test("Rust installer failure blocks the gate", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { rust: "1.98.0" });
  const rustDist = makeRustDist(path.join(root, "rust-dist"), { succeeds: false });
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_RUSTUP_DIST_BASE: rustDist }),
  });
  assert.equal(result.status, 22);
  assert.match(result.stderr, /Rust 安装失败/);
}));

test("Rust checksum mismatch blocks before executing the installer", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { rust: "1.98.0" });
  const rustDist = makeRustDist(path.join(root, "rust-dist"), { validChecksum: false });
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_RUSTUP_DIST_BASE: rustDist }),
  });
  assert.equal(result.status, 22);
  assert.match(result.stderr, /SHA-256 校验失败/);
  assert.equal(existsSync(path.join(root, "home", ".cargo", "bin", "rustc")), false);
}));

test("Node checksum mismatch blocks before installation", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe, { includeNode: false });
  const nodeDist = makeNodeDist(path.join(root, "node-dist"), { validChecksum: false });
  const result = runGate(root, ["--install-missing"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_NODE_DIST_BASE: nodeDist }),
  });
  assert.equal(result.status, 26);
  assert.match(result.stderr, /SHA-256 校验失败/);
  assert.equal(existsSync(path.join(root, "home", ".local", "bin", "node")), false);
}));

test("lower GUI toolchain is upgraded and reprobed", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { node: "24.20.9", pnpm: "12.4.0" });
  const nodeDist = makeNodeDist(path.join(root, "node-dist"));
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_NODE_DIST_BASE: nodeDist }),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /gate\.node\.change=upgraded/);
  assert.match(result.stdout, /gate\.pnpm\.change=upgraded/);
  assert.match(result.stdout, /gate\.node\.version=v24\.21\.0/);
  assert.match(result.stdout, /gate\.pnpm\.version=12\.4\.1/);
}));

test("pnpm upgrade failure blocks the GUI gate", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { pnpm: "12.4.0" });
  fakeNpmInstaller(probe, { succeeds: false });
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
    probe,
    extra: isolatedInstallOverrides(),
  });
  assert.equal(result.status, 28);
  assert.match(result.stderr, /pnpm 安装失败/);
}));

test("Node installation selects the highest LTS independently of index order", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { node: "24.20.9" });
  const nodeDist = makeNodeDist(path.join(root, "node-dist"), { version: "v26.1.0" });
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
    probe,
    extra: isolatedInstallOverrides({ AFH_NODE_DIST_BASE: nodeDist }),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /gate\.node\.version=v26\.1\.0/);
  assert.match(result.stdout, /gate\.node\.change=upgraded/);
}));

test("Linux x64 musl selects the official musl Node archive", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { node: "24.20.9" });
  const nodeDist = makeNodeDist(path.join(root, "node-dist"), { forcedTuple: ["linux", "x64-musl"] });
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
    probe,
    extra: isolatedInstallOverrides({
      AFH_NODE_DIST_BASE: nodeDist,
      AFH_TEST_HOST_OS: "Linux",
      AFH_TEST_HOST_ARCH: "x86_64",
      AFH_TEST_LINUX_LIBC: "musl",
    }),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /gate\.node\.version=v24\.21\.0/);
  assert.equal(existsSync(path.join(root, "home", ".local", "lib", "nodejs", "v24.21.0", "bin", "node")), true);
}));

test("Linux arm64 musl fails before requesting a nonexistent Node archive", posixOnly, () => withTemporaryRoot((root) => {
  const probe = path.join(root, "probe");
  fakeExistingTools(probe);
  fakeFrontendTools(probe, { node: "24.20.9" });
  const result = runGate(root, ["--install-missing", "--interfaces", "GUI"], {
    probe,
    extra: isolatedInstallOverrides({
      AFH_NODE_DIST_BASE: new URL(`file://${path.join(root, "unreachable-node-dist")}`).href,
      AFH_TEST_HOST_OS: "Linux",
      AFH_TEST_HOST_ARCH: "aarch64",
      AFH_TEST_LINUX_LIBC: "musl",
    }),
  });
  assert.equal(result.status, 26, result.stderr);
  assert.match(result.stderr, /没有官方 Linux arm64 musl 制品/);
  assert.equal(existsSync(path.join(root, "home", ".local", "lib", "nodejs")), false);
}));
