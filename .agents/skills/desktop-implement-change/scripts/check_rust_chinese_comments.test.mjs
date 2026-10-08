import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, test } from "node:test";

import { inspectProject, main } from "./check_rust_chinese_comments.mjs";

let root;

function write(relative, payload) {
  const target = path.join(root, relative);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, payload);
  return target;
}

function packageFixture(relative = ".", source = "") {
  const prefix = relative === "." ? "" : `${relative}/`;
  const name = relative === "." ? "root_package" : relative.replaceAll("/", "_");
  write(`${prefix}Cargo.toml`, `[package]\nname = "${name}"\nversion = "0.1.0"\nedition = "2024"\n`);
  write(`${prefix}src/lib.rs`, source);
}

function captureOutput(callback) {
  let stdout = "";
  let stderr = "";
  const stdoutWrite = process.stdout.write;
  const stderrWrite = process.stderr.write;
  process.stdout.write = (chunk) => { stdout += String(chunk); return true; };
  process.stderr.write = (chunk) => { stderr += String(chunk); return true; };
  try { return { result: callback(), stdout, stderr }; }
  finally { process.stdout.write = stdoutWrite; process.stderr.write = stderrWrite; }
}

beforeEach(() => { root = mkdtempSync(path.join(tmpdir(), "afh-rust-comments-")); });
afterEach(() => rmSync(root, { recursive: true, force: true }));

test("accepts workspace and all governed declarations", () => {
  write("Cargo.toml", '[workspace]\nmembers = ["core", "cli"]\nresolver = "3"\n');
  packageFixture("core", `
/// 保存任务状态。
pub struct Task;
/// 表示任务阶段。
pub enum Phase { Ready }
/// 表示共享内存值。
pub union Shared { value: u64 }
/// 定义执行器约束。
pub trait Runner {
    /// 执行任务。
    fn run(&self);
}
/// 表示任务标识。
pub type TaskId = u64;
/// 查询当前任务。
pub async fn load<'a>(_value: &'a str) {}
`);
  packageFixture("cli", "\n/// 执行 CLI。\nfn main() {}\n");
  write("cli/build.rs", "/// 执行构建准备。\nfn main() {}\n");
  write("cli/tests/cli.rs", "/// 验证 CLI 状态。\n#[test]\nfn reports_status() {}\n");
  const report = inspectProject(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.equal(report.checkedPackages, 2);
  assert.equal(report.checkedRustFiles, 4);
  assert.equal(report.checkedDeclarations, 10);
});

test("accepts outer attributes and rejects non-item evidence", () => {
  packageFixture(".", `
//! 中文模块说明不能替代条目说明。
#[doc = "保存有效结构。"]
#[derive(Clone)]
struct Valid;
#[cfg_attr(all(), doc = "保存条件结构。")]
struct Conditional;
/// English only.
struct English;
#[allow(doc = "这不是文档属性")]
struct NestedButInvalid;
// 普通中文注释不属于 Rust 文档。
fn ordinary() {}
/// 中文实现说明只属于 impl。
impl Valid { fn method(&self) {} }
`);
  const report = inspectProject(root);
  assert.deepEqual(report.violations.map((item) => [item.kind, item.name]), [
    ["struct", "English"],
    ["struct", "NestedButInvalid"],
    ["fn", "ordinary"],
    ["fn", "method"],
  ]);
});

test("ignores macro bodies, strings, comments, and function-pointer tokens", () => {
  packageFixture(".", `
// struct CommentOnly; fn comment_only() {}
const TEXT: &str = "trait StringOnly { fn string_only(); }";
/// 表示回调类型。
type Callback = fn(u64) -> u64;
macro_rules! make_function { ($name:ident) => { fn $name() {} }; }
const TOKENS: &str = stringify!(fn generated() {});
/// 返回原始输入。
fn identity(value: u64) -> u64 { value }
`);
  const report = inspectProject(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.equal(report.checkedDeclarations, 2);
});

test("reports unbalanced source, invalid UTF-8, NUL, and symlinks", () => {
  packageFixture(".", "/// 中文函数。\nfn broken(\n");
  write("src/invalid.rs", Buffer.from([0xff, 0xfe]));
  write("src/nul.rs", Buffer.concat([Buffer.from("/// 中文结构。\nstruct Nul;"), Buffer.from([0])]));
  const target = write("outside.rs", "/// 中文结构。\nstruct Outside;\n");
  try { symlinkSync(target, path.join(root, "src", "linked.rs")); }
  catch (error) { if (error?.code === "EPERM" || error?.code === "ENOSYS") return; throw error; }
  const report = inspectProject(root);
  assert.equal(report.ok, false);
  for (const fragment of ["未闭合", "不是 UTF-8", "NUL", "符号链接"]) {
    assert.ok(report.errors.some((item) => item.includes(fragment)), JSON.stringify(report));
  }
});

test("rejects workspace escape, missing member, and empty package", () => {
  write("Cargo.toml", '[workspace]\nmembers = ["../outside", "missing-*", "empty"]\n');
  write("empty/Cargo.toml", '[package]\nname = "empty"\nversion = "0.1.0"\nedition = "2024"\n');
  const report = inspectProject(root);
  assert.equal(report.ok, false);
  for (const fragment of ["不得越界", "未匹配", "未找到 Rust 源码"]) {
    assert.ok(report.errors.some((item) => item.includes(fragment)), JSON.stringify(report));
  }
});

test("rejects missing manifest and zero governed declarations", () => {
  write("src/lib.rs", "const VALUE: u64 = 1;\n");
  const missingManifest = inspectProject(root);
  assert.equal(missingManifest.ok, false);
  assert.ok(missingManifest.errors.some((item) => item.includes("缺少 Cargo.toml")));
  write("Cargo.toml", '[package]\nname = "empty_declarations"\nversion = "0.1.0"\nedition = "2024"\n');
  const noDeclarations = inspectProject(root);
  assert.equal(noDeclarations.ok, false);
  assert.ok(noDeclarations.errors.some((item) => item.includes("未发现受中文注释门禁管理")));
});

test("JSON command uses zero-one-two exit contract", () => {
  const reports = [
    [{ ok: true, errors: [], violations: [] }, 0],
    [{ ok: false, errors: [], violations: [{}] }, 1],
    [{ ok: false, errors: ["failure"], violations: [] }, 2],
  ];
  for (const [report, expected] of reports) {
    const output = captureOutput(() => main(["--json"], () => report));
    assert.equal(output.result, expected);
    assert.deepEqual(JSON.parse(output.stdout), report);
  }
});
