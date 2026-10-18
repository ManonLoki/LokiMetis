import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { afterEach, beforeEach, test } from "node:test";
import { fileURLToPath } from "node:url";
import { parseThirdPartyManifest } from "./verified_third_party.mjs";

import {
  DEFAULT_HARD_LINE_LIMIT,
  DEFAULT_REVIEW_THRESHOLD,
  FRONTEND_CODE_SUFFIXES,
  FRONTEND_HARD_LINE_LIMIT,
  FRONTEND_REVIEW_THRESHOLD,
  RUST_HARD_LINE_LIMIT,
  RUST_REVIEW_THRESHOLD,
  inspectRepository,
  main,
} from "./check_file_line_limits.mjs";

let root;

function run(command, args, options = {}) { return spawnSync(command, args, { encoding: "utf8", ...options }); }

function write(relative, payload) {
  const target = path.join(root, relative);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, payload);
  return target;
}

function track(...relative) {
  assert.equal(run("git", ["add", "--", ...relative], { cwd: root }).status, 0);
}

function installThirdPartySource() {
  const source = fileURLToPath(new URL("../../../../vendor/accesskit_macos/", import.meta.url));
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
  const source = fileURLToPath(new URL("../../../../.archify/architecture-lokimetris-20261011-2240/", import.meta.url));
  for (const name of ["lokimetris.html", "lokimetris.finalize.json"]) {
    const target = path.join(root, directory, name);
    mkdirSync(path.dirname(target), { recursive: true });
    copyFileSync(path.join(source, name), target);
  }
  return `${directory}/lokimetris.html`;
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

beforeEach(() => {
  root = mkdtempSync(path.join(tmpdir(), "afh-line-limits-"));
  assert.equal(run("git", ["init", "--quiet", "--initial-branch=main", "."], { cwd: root }).status, 0);
});

afterEach(() => rmSync(root, { recursive: true, force: true }));

test("generic text accepts 500 lines with or without final newline", () => {
  write("tracked.txt", Array(DEFAULT_REVIEW_THRESHOLD).fill("x").join("\n"));
  write("untracked.txt", "y\n".repeat(DEFAULT_REVIEW_THRESHOLD));
  track("tracked.txt");
  const report = inspectRepository(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.equal(report.checkedTextFiles, 2);
  assert.deepEqual(report.reviewCandidates, []);
});

test("generic text reports 501 through 2000 without hard failure", () => {
  write("tracked.md", "x\n".repeat(DEFAULT_REVIEW_THRESHOLD + 1));
  write("nested/untracked.txt", "x\n".repeat(DEFAULT_HARD_LINE_LIMIT));
  track("tracked.md");
  const report = inspectRepository(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.deepEqual(report.reviewCandidates.map((item) => item.path), ["nested/untracked.txt", "tracked.md"]);
  assert.ok(report.reviewCandidates.every((item) => item.profile === "maintained_text"));
  assert.deepEqual(report.violations, []);
});

test("generic text rejects 2001 tracked and untracked lines", () => {
  const payload = Array(DEFAULT_HARD_LINE_LIMIT + 1).fill("x").join("\n");
  write("tracked.md", payload);
  write("nested/untracked.txt", payload);
  track("tracked.md");
  const report = inspectRepository(root);
  assert.equal(report.ok, false);
  assert.deepEqual(report.violations.map((item) => item.path), ["nested/untracked.txt", "tracked.md"]);
});

test("Rust uses 400-line review and 800-line hard limits", () => {
  write("src/within.rs", "x\n".repeat(RUST_REVIEW_THRESHOLD));
  write("src/review.rs", "x\n".repeat(RUST_REVIEW_THRESHOLD + 1));
  write("src/domain/mod.rs", "x\n".repeat(RUST_HARD_LINE_LIMIT));
  write("src/violation.rs", "x\n".repeat(RUST_HARD_LINE_LIMIT + 1));
  const report = inspectRepository(root);
  assert.equal(report.ok, false);
  assert.deepEqual(report.reviewCandidates.map(({ path: itemPath, profile, threshold, limit }) => [itemPath, profile, threshold, limit]), [
    ["src/domain/mod.rs", "rust", 400, 800],
    ["src/review.rs", "rust", 400, 800],
  ]);
  assert.deepEqual(report.violations.map(({ path: itemPath, profile, limit }) => [itemPath, profile, limit]), [
    ["src/violation.rs", "rust", 800],
  ]);
});

test("frontend uses 500-line review and 1000-line hard limits", () => {
  write("src/Within.tsx", "x\n".repeat(FRONTEND_REVIEW_THRESHOLD));
  write("src/review.ts", "x\n".repeat(FRONTEND_REVIEW_THRESHOLD + 1));
  write("src/page.jsx", "x\n".repeat(FRONTEND_HARD_LINE_LIMIT));
  write("src/violation.css", "x\n".repeat(FRONTEND_HARD_LINE_LIMIT + 1));
  const report = inspectRepository(root);
  assert.equal(report.ok, false);
  assert.deepEqual(report.reviewCandidates.map(({ path: itemPath, profile, threshold, limit }) => [itemPath, profile, threshold, limit]), [
    ["src/page.jsx", "frontend", 500, 1000],
    ["src/review.ts", "frontend", 500, 1000],
  ]);
  assert.deepEqual(report.violations.map(({ path: itemPath, profile, limit }) => [itemPath, profile, limit]), [
    ["src/violation.css", "frontend", 1000],
  ]);
});

test("all declared frontend suffixes select frontend profile", () => {
  [...FRONTEND_CODE_SUFFIXES].sort().forEach((suffix, index) => {
    write(`frontend/example_${index}${suffix}`, "x\n".repeat(FRONTEND_REVIEW_THRESHOLD + 1));
  });
  const report = inspectRepository(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.equal(report.reviewCandidates.length, FRONTEND_CODE_SUFFIXES.size);
  assert.ok(report.reviewCandidates.every((item) => item.profile === "frontend" && item.threshold === 500 && item.limit === 1000));
});

test("handles hidden and spaced paths", () => {
  write(".hidden file.md", "x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1));
  track(".hidden file.md");
  assert.deepEqual(inspectRepository(root).violations.map((item) => item.path), [".hidden file.md"]);
});

test("handles newlines in paths", { skip: process.platform === "win32" }, () => {
  const name = "line\nbreak.txt";
  try { write(name, "x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1)); }
  catch (error) { if (error?.code === "EINVAL") return; throw error; }
  track(name);
  assert.deepEqual(inspectRepository(root).violations.map((item) => item.path), [name]);
});

test("ignored binary and symlink targets do not bypass or expand scope", () => {
  write(".gitignore", "ignored.txt\ntarget.txt\n");
  write("ignored.txt", "x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1));
  write("binary.bin", Buffer.concat([Buffer.from("header\0"), Buffer.from("x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1))]));
  write("target.txt", "x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1));
  track(".gitignore", "binary.bin");
  try { symlinkSync(path.join(root, "target.txt"), path.join(root, "linked.txt")); }
  catch (error) { if (error?.code === "EPERM" || error?.code === "ENOSYS") return; throw error; }
  track("linked.txt");
  const report = inspectRepository(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.equal(report.skippedBinaryFiles, 1);
  assert.equal(report.skippedSymlinks, 1);
});

test("deleted tracked file is skipped as absent worktree content", () => {
  const deleted = write("deleted.txt", "x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1));
  track("deleted.txt");
  unlinkSync(deleted);
  const report = inspectRepository(root);
  assert.equal(report.ok, true, JSON.stringify(report));
  assert.equal(report.skippedMissingFiles, 1);
});

test("excludes only known generated lockfile names", () => {
  const payload = "x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1);
  write("Cargo.lock", payload);
  write("Cargo.lock.notes", payload);
  track("Cargo.lock", "Cargo.lock.notes");
  const report = inspectRepository(root);
  assert.deepEqual(report.excludedGeneratedFiles, ["Cargo.lock"]);
  assert.deepEqual(report.violations.map((item) => item.path), ["Cargo.lock.notes"]);
});

test("excludes only root Harness generated lock path", () => {
  const payload = "x\n".repeat(DEFAULT_HARD_LINE_LIMIT + 1);
  const paths = [".harness/upstream-lock.json", "docs/upstream-lock.json", "nested/.harness/upstream-lock.json"];
  paths.forEach((relative) => write(relative, payload));
  track(...paths);
  const report = inspectRepository(root);
  assert.deepEqual(report.excludedGeneratedFiles, [".harness/upstream-lock.json"]);
  assert.deepEqual(report.violations.map((item) => item.path), ["docs/upstream-lock.json", "nested/.harness/upstream-lock.json"]);
});

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
  const output = captureOutput(() => main(["--json"], () => report));
  assert.equal(output.result, 2);
  assert.deepEqual(JSON.parse(output.stdout).excludedThirdPartyFiles, []);
});

test("invalid UTF-8 text candidate fails closed", () => {
  write("unknown.dat", Buffer.from([0xff, 0xfe, 0xfd]));
  track("unknown.dat");
  const report = inspectRepository(root);
  assert.equal(report.ok, false);
  assert.ok(report.errors.some((item) => item.includes("不是 UTF-8")), JSON.stringify(report));
});

test("text CLI explains Rust directory mod.rs structure", () => {
  const report = {
    ok: false,
    errors: [],
    reviewCandidates: [],
    violations: [{ path: "src/domain.rs", lines: 801, profile: "rust", threshold: 400, limit: 800 }],
  };
  const output = captureOutput(() => main([], () => report));
  assert.equal(output.result, 1);
  assert.match(output.stderr, /<module>\/mod\.rs/);
});

test("non-Git directory is operational failure", () => {
  const temporary = mkdtempSync(path.join(tmpdir(), "afh-non-git-"));
  try {
    const report = inspectRepository(temporary);
    assert.equal(report.ok, false);
    assert.ok(report.errors.some((item) => item.includes("Git 文件枚举失败")), JSON.stringify(report));
  } finally { rmSync(temporary, { recursive: true, force: true }); }
});

test("Git execution error fails closed", () => {
  const previousPath = process.env.PATH;
  process.env.PATH = "";
  try {
    const report = inspectRepository(root);
    assert.ok(report.errors.some((item) => item.includes("无法执行 Git")), JSON.stringify(report));
  } finally { process.env.PATH = previousPath; }
});

test("JSON CLI uses zero-one-two exit contract", () => {
  const reports = [
    [{ ok: true, errors: [], reviewCandidates: [], violations: [] }, 0],
    [{ ok: false, errors: [], reviewCandidates: [], violations: [{}] }, 1],
    [{ ok: false, errors: ["failure"], reviewCandidates: [], violations: [] }, 2],
  ];
  for (const [report, expected] of reports) {
    const output = captureOutput(() => main(["--json"], () => report));
    assert.equal(output.result, expected);
    assert.deepEqual(JSON.parse(output.stdout), report);
  }
});
