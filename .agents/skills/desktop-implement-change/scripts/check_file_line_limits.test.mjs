import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { afterEach, beforeEach, test } from "node:test";

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
