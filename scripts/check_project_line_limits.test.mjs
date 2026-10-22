/** 验证项目固定封存物与第三方登记，公共扫描规则由原样上游测试覆盖。 */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { afterEach, beforeEach, test } from "node:test";
import { fileURLToPath } from "node:url";
import { parseThirdPartyManifest } from "../.agents/skills/desktop-implement-change/scripts/verified_third_party.mjs";

import {
  DEFAULT_HARD_LINE_LIMIT,
  FRONTEND_HARD_LINE_LIMIT,
  RUST_HARD_LINE_LIMIT,
} from "../.agents/skills/desktop-implement-change/scripts/check_file_line_limits.mjs";
import { inspectRepository, main } from "./check_project_line_limits.mjs";

let root;

/** 在隔离临时仓库执行夹具命令并收集实际状态，不访问用户 Git 配置。 */
function run(command, args, options = {}) { return spawnSync(command, args, { encoding: "utf8", ...options }); }

/** 只向当前夹具写入指定路径及必要父目录，返回真实文件路径。 */
function write(relative, payload) {
  const target = path.join(root, relative);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, payload);
  return target;
}

/** 复制仓库已有原样归档及固定清单，不制造新的信任摘要。 */
function installThirdPartySource() {
  const source = fileURLToPath(new URL("../vendor/accesskit_macos/", import.meta.url));
  const directory = "vendor/accesskit_macos";
  const manifest = readFileSync(path.join(source, "source-content.sha256"));
  const files = parseThirdPartyManifest(manifest).map((entry) => entry.relative);
  for (const relative of ["source-content.sha256", ...files]) {
    const target = path.join(root, directory, relative);
    mkdirSync(path.dirname(target), { recursive: true });
    copyFileSync(path.join(source, relative), target);
  }
  return files.map((relative) => `${directory}/${relative}`).sort();
}

/** 安装仓库已封存的真实生成物与完成回执，不能用新造的摘要冒充登记。 */
function installGeneratedArchitecture(directory = ".archify/architecture-lokimetris-20261011-2240") {
  const source = fileURLToPath(new URL("../.archify/architecture-lokimetris-20261011-2240/", import.meta.url));
  for (const name of ["lokimetris.html", "lokimetris.finalize.json"]) {
    const target = path.join(root, directory, name);
    mkdirSync(path.dirname(target), { recursive: true });
    copyFileSync(path.join(source, name), target);
  }
  return `${directory}/lokimetris.html`;
}

/** 临时收集同进程 CLI 输出并始终恢复流方法，保留返回状态供断言。 */
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

beforeEach(() => {
  root = mkdtempSync(path.join(tmpdir(), "afh-line-limits-"));
  assert.equal(run("git", ["init", "--quiet", "--initial-branch=main", "."], { cwd: root }).status, 0);
});

afterEach(() => rmSync(root, { recursive: true, force: true }));

test("sealed architecture bytes and finalize receipt exempt only the exact registered HTML", () => {
  const artifact = installGeneratedArchitecture();
  const neighbor = ".archify/architecture-lokimetris-20261011-2240/handwritten.html";
  write(neighbor, "x\n".repeat(FRONTEND_HARD_LINE_LIMIT + 1));
  const report = inspectRepository(root);
  assert.deepEqual(report.errors, []);
  assert.deepEqual(report.excludedGeneratedFiles, [artifact]);
  assert.deepEqual(report.violations.map((item) => item.path), [neighbor]);
});

test("registered architecture remains excluded while its finalize receipt is maintained text", () => {
  const artifact = installGeneratedArchitecture();
  const report = inspectRepository(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.deepEqual(report.excludedGeneratedFiles, [artifact]);
  assert.equal(report.checkedTextFiles, 1);
});

/** 当前架构封存只信任已完成回执绑定的原字节，等长篡改不能继承生成物排除。 */
test("current architecture registration accepts its finalized bytes and rejects corruption", () => {
  const directory = ".archify/architecture-desktop-20261022-144530";
  const files = ["desktop.html", "review-2/desktop.finalize.json"];
  for (const relative of files) {
    const source = fileURLToPath(new URL(`../${directory}/${relative}`, import.meta.url));
    const target = path.join(root, directory, relative);
    mkdirSync(path.dirname(target), { recursive: true });
    copyFileSync(source, target);
  }
  const artifact = `${directory}/desktop.html`;
  const accepted = inspectRepository(root);
  assert.equal(accepted.ok, true, JSON.stringify(accepted));
  assert.deepEqual(accepted.excludedGeneratedFiles, [artifact]);
  const payload = readFileSync(path.join(root, artifact));
  payload[100] ^= 1;
  write(artifact, payload);
  const rejected = inspectRepository(root);
  assert.deepEqual(rejected.excludedGeneratedFiles, []);
  assert.ok(rejected.errors.some(error => error.includes("生成物 SHA256")));
  assert.deepEqual(rejected.violations.map(item => item.path), [artifact]);
});

test("copying a genuine generated pair to another directory does not grant exclusion", () => {
  const artifact = installGeneratedArchitecture(".archify/unregistered");
  const report = inspectRepository(root);
  assert.deepEqual(report.errors, []);
  assert.deepEqual(report.excludedGeneratedFiles, []);
  assert.deepEqual(report.violations.map((item) => item.path), [artifact]);
});

test("generator markers alone do not exempt handwritten HTML", () => {
  write(".archify/unregistered.html", '<meta name="generator" content="archify 3.0.1">\n'.repeat(FRONTEND_HARD_LINE_LIMIT + 1));
  const report = inspectRepository(root);
  assert.deepEqual(report.excludedGeneratedFiles, []);
  assert.deepEqual(report.violations.map((item) => item.path), [".archify/unregistered.html"]);
});

test("equal-size HTML corruption fails closed and still reports its line violation", () => {
  const artifact = installGeneratedArchitecture();
  const payload = readFileSync(path.join(root, artifact));
  payload[100] ^= 1;
  write(artifact, payload);
  const report = inspectRepository(root);
  assert.deepEqual(report.excludedGeneratedFiles, []);
  assert.ok(report.errors.some((error) => error.includes("生成物 SHA256")), JSON.stringify(report));
  assert.deepEqual(report.violations.map((item) => item.path), [artifact]);
});

test("changing HTML and all matching receipt hashes cannot forge a new trusted registration", () => {
  const artifact = installGeneratedArchitecture();
  const payload = readFileSync(path.join(root, artifact));
  payload[100] ^= 1;
  write(artifact, payload);
  const receipt = artifact.replace(".html", ".finalize.json");
  const content = readFileSync(path.join(root, receipt), "utf8");
  const original = JSON.parse(content).artifact.sha256;
  const forged = createHash("sha256").update(payload).digest("hex");
  write(receipt, content.replaceAll(original, forged));
  const report = inspectRepository(root);
  assert.deepEqual(report.excludedGeneratedFiles, []);
  assert.ok(report.errors.some((error) => error.includes("完成回执 SHA256")), JSON.stringify(report));
  assert.deepEqual(report.violations.map((item) => item.path), [artifact]);
});

for (const missing of ["lokimetris.html", "lokimetris.finalize.json"]) {
  test(`partially missing sealed pair fails closed: ${missing}`, () => {
    const artifact = installGeneratedArchitecture();
    unlinkSync(path.join(root, path.posix.dirname(artifact), missing));
    const report = inspectRepository(root);
    assert.deepEqual(report.excludedGeneratedFiles, []);
    assert.ok(report.errors.some((error) => error.includes("缺失")), JSON.stringify(report));
  });
}

for (const linked of ["lokimetris.html", "lokimetris.finalize.json"]) {
  test(`sealed artifact and receipt reject file links: ${linked}`, (context) => {
    const artifact = installGeneratedArchitecture();
    const target = path.join(root, path.posix.dirname(artifact), linked);
    const original = path.join(root, "original-payload");
    renameSync(target, original);
    try { symlinkSync(original, target); }
    catch (error) { if (error?.code === "EPERM" || error?.code === "ENOSYS") { context.skip("host disallows symbolic links"); return; } throw error; }
    const report = inspectRepository(root);
    assert.deepEqual(report.excludedGeneratedFiles, []);
    assert.ok(report.errors.some((error) => error.includes("符号链接")), JSON.stringify(report));
  });
}

test("a symlink in the registered parent chain fails closed", (context) => {
  const artifact = installGeneratedArchitecture();
  const directory = path.join(root, path.posix.dirname(artifact));
  const original = path.join(root, "original-directory");
  renameSync(directory, original);
  try { symlinkSync(original, directory, "dir"); }
  catch (error) { if (error?.code === "EPERM" || error?.code === "ENOSYS") { context.skip("host disallows symbolic links"); return; } throw error; }
  const report = inspectRepository(root);
  assert.deepEqual(report.excludedGeneratedFiles, []);
  assert.ok(report.errors.some((error) => error.includes("符号链接")), JSON.stringify(report));
});

test("registered receipt must be a regular file", () => {
  const artifact = installGeneratedArchitecture();
  const receipt = path.join(root, artifact.replace(".html", ".finalize.json"));
  unlinkSync(receipt);
  mkdirSync(receipt);
  const report = inspectRepository(root);
  assert.deepEqual(report.excludedGeneratedFiles, []);
  assert.ok(report.errors.some((error) => error.includes("不是普通文件")), JSON.stringify(report));
});

test("verified original third-party files are explicitly reported and excluded", () => {
  const expected = installThirdPartySource();
  write("Cargo.lock", "generated\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1));
  const report = inspectRepository(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.deepEqual(report.excludedThirdPartyFiles, expected);
  assert.equal(report.excludedThirdPartyFiles.length, 18);
  assert.deepEqual(report.excludedGeneratedFiles, ["Cargo.lock"]);
  assert.equal(report.checkedTextFiles, 1); // 来源清单仍属于维护范围。
  assert.deepEqual(report.violations, []);
});

test("additional handwritten files beside registered originals retain their own limits", () => {
  const expected = installThirdPartySource();
  const extras = ["build.rs", "patches/handwritten.rs", "upstream/src/extra.rs"];
  extras.forEach((relative) => write(`vendor/accesskit_macos/${relative}`, "x\n".repeat(RUST_HARD_LINE_LIMIT + 1)));
  write("vendor/other/src/node.rs", "x\n".repeat(RUST_HARD_LINE_LIMIT + 1));
  const report = inspectRepository(root);
  assert.equal(report.ok, false);
  assert.deepEqual(report.errors, []);
  assert.deepEqual(report.excludedThirdPartyFiles, expected);
  assert.deepEqual(report.violations.map((item) => item.path), [
    "vendor/accesskit_macos/build.rs",
    "vendor/accesskit_macos/patches/handwritten.rs",
    "vendor/accesskit_macos/upstream/src/extra.rs",
    "vendor/other/src/node.rs",
  ]);
});

test("a corrupted registration fails closed and does not suppress source violations", () => {
  installThirdPartySource();
  write("vendor/accesskit_macos/upstream/src/node.rs", "x\n".repeat(RUST_HARD_LINE_LIMIT + 1));
  const report = inspectRepository(root);
  assert.equal(report.ok, false);
  assert.deepEqual(report.excludedThirdPartyFiles, []);
  assert.ok(report.errors.some((error) => error.includes("原样文件 SHA256 不符")), JSON.stringify(report));
  assert.ok(report.violations.some((item) => item.path === "vendor/accesskit_macos/upstream/src/node.rs"));
  const output = captureOutput(() => main(["--root", root, "--json"]));
  assert.equal(output.result, 2);
  assert.deepEqual(JSON.parse(output.stdout).excludedThirdPartyFiles, []);
});

/** 登记成功也不得吞掉同次扫描的未知文本错误。 */
test("registered originals preserve unrelated upstream decoding errors", () => {
  installThirdPartySource();
  write("unknown.dat", Buffer.from([0xff, 0xfe]));
  const report = inspectRepository(root);
  assert.equal(report.ok, false);
  assert.equal(report.excludedThirdPartyFiles.length, 18);
  assert.ok(report.errors.some(error => error.includes("UTF-8")));
});
