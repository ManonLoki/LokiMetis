/** 覆盖发布更新日志的格式、保留上限与文件安全门禁。 */

import test from "node:test";
import assert from "node:assert/strict";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  MAX_RELEASE_NOTES_BYTES,
  ReleaseNotesError,
  loadDocument,
  main,
  normalizeDisplayVersion,
  parseDocumentBytes,
  renderDocument,
  upsertRelease,
} from "./release_notes.mjs";

/** 为单个测试建立隔离更新日志路径并保证回收。 */
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "release-notes-"));
  return { root, path: join(root, "release-notes.json"), cleanup: () => rmSync(root, { recursive: true, force: true }) };
}

/** 写入一个同时含优化与修复的有效版本。 */
function upsert(path, version, day) {
  return upsertRelease(path, {
    releaseDate: `2026-08-${String(day).padStart(2, "0")}`,
    version,
    featureOptimizationsZhCn: [`优化 ${version}`],
    featureOptimizationsEnUs: [`Improve ${version}`],
    bugFixesZhCn: [`修复 ${version}`],
    bugFixesEnUs: [`Fix ${version}`],
  });
}

test("upsert_normalizes_version_and_renders_exact_sections", () => {
  const item = fixture();
  try {
    upsert(item.path, "vv1.2.3", 26);
    const document = loadDocument(item.path);
    assert.equal(document.releases[0].version, "v1.2.3");
    assert.equal(
      renderDocument(document, "zh-CN"),
      "-----------更新日志 2026-08-26 v1.2.3----------\n\n###功能优化\n\n- 优化 vv1.2.3\n\n###问题修复\n\n- 修复 vv1.2.3",
    );
    assert.equal(
      renderDocument(document, "en-US"),
      "-----------Release notes 2026-08-26 v1.2.3----------\n\n###Feature optimizations\n\n- Improve vv1.2.3\n\n###Bug fixes\n\n- Fix vv1.2.3",
    );
  } finally { item.cleanup(); }
});

test("semver_reader_enforces_u64_major_and_base_100_minor_patch", () => {
  for (const valid of ["101.0.0", "18446744073709551615.0.0"]) {
    assert.equal(normalizeDisplayVersion(valid), `v${valid}`);
  }
  assert.throws(() => normalizeDisplayVersion("18446744073709551616.0.0"), /major/);
  assert.throws(() => normalizeDisplayVersion(`${"9".repeat(5000)}.0.0`), /major/);
  assert.throws(() => normalizeDisplayVersion("1.٢.3"), /version must/);
  for (const invalid of ["0.100.0", "0.0.100", "0.101.0", "0.0.101"]) {
    assert.throws(() => normalizeDisplayVersion(invalid), /minor and patch/);
  }
});

test("upsert_keeps_ten_actual_releases_and_replaces_the_current_version", () => {
  const item = fixture();
  try {
    const versions = [
      "1.0.0", "1.0.2", "1.0.99", "1.1.0", "1.1.1", "1.9.0",
      "1.99.99", "2.0.0", "2.0.1", "2.1.0", "2.1.2",
    ];
    for (const [index, version] of versions.entries()) {
      upsert(item.path, version, index + 1);
      if (index === 9) {
        assert.deepEqual(loadDocument(item.path).releases.map((entry) => entry.version),
          versions.slice(0, 10).reverse().map((released) => `v${released}`));
      }
    }
    assert.deepEqual(loadDocument(item.path).releases.map((entry) => entry.version), [
      "v2.1.2", "v2.1.0", "v2.0.1", "v2.0.0", "v1.99.99",
      "v1.9.0", "v1.1.1", "v1.1.0", "v1.0.99", "v1.0.2",
    ]);
    assert.equal(main(["check", "--file", item.path, "--expected-version", "2.1.2"]), 0);
    upsertRelease(item.path, {
      releaseDate: "2026-08-11",
      version: "v2.1.2",
      featureOptimizationsZhCn: ["替换后的优化"],
      featureOptimizationsEnUs: ["Replacement improvement"],
      bugFixesZhCn: [],
      bugFixesEnUs: [],
    });
    const replaced = loadDocument(item.path);
    assert.equal(replaced.releases.length, 10);
    assert.equal(replaced.releases.at(-1).version, "v1.0.2");
    assert.deepEqual(replaced.releases[0].featureOptimizations, [
      { "zh-CN": "替换后的优化", "en-US": "Replacement improvement" },
    ]);
  } finally { item.cleanup(); }
});

test("rejects_item_overflow_empty_release_and_missing_translation", () => {
  const item = fixture();
  try {
    assert.throws(() => upsertRelease(item.path, {
      releaseDate: "2026-08-26", version: "1.2.3",
      featureOptimizationsZhCn: Array.from({ length: 11 }, (_, index) => String(index)),
      featureOptimizationsEnUs: Array.from({ length: 11 }, (_, index) => `item ${index}`),
      bugFixesZhCn: [], bugFixesEnUs: [],
    }), /at most 10/);
    assert.throws(() => upsertRelease(item.path, {
      releaseDate: "2026-08-26", version: "1.2.3",
      featureOptimizationsZhCn: [], featureOptimizationsEnUs: [], bugFixesZhCn: [], bugFixesEnUs: [],
    }), /actual change/);
    assert.throws(() => upsertRelease(item.path, {
      releaseDate: "2026-08-26", version: "1.2.3",
      featureOptimizationsZhCn: ["新增能力"], featureOptimizationsEnUs: [], bugFixesZhCn: [], bugFixesEnUs: [],
    }), /same number/);
    const tenItems = Array.from({ length: 10 }, (_, index) => `Item ${index + 1}`);
    const fullSections = upsertRelease(item.path, {
      releaseDate: "2026-08-26", version: "1.2.3",
      featureOptimizationsZhCn: tenItems, featureOptimizationsEnUs: tenItems,
      bugFixesZhCn: tenItems, bugFixesEnUs: tenItems,
    });
    assert.equal(fullSections.releases[0].featureOptimizations.length, 10);
    assert.equal(fullSections.releases[0].bugFixes.length, 10);
    const document = upsert(item.path, "1.2.3", 26);
    assert.throws(() => renderDocument(document, "fr-FR"), /locale must be/);
  } finally { item.cleanup(); }
});

test("check_rejects_schema_types_duplicate_keys_and_noncanonical_values", () => {
  const item = fixture();
  try {
    for (const raw of [
      '{"schemaVersion":2,"releases":[]}',
      '{"schemaVersion":2.0,"releases":[]}',
      '{"schemaVersion":2,"schemaVersion":2,"releases":[]}',
    ]) {
      writeFileSync(item.path, raw, "utf8");
      assert.throws(() => loadDocument(item.path), ReleaseNotesError);
      assert.throws(() => parseDocumentBytes(readFileSync(item.path)), ReleaseNotesError);
    }
    unlinkSync(item.path);
    upsert(item.path, "1.2.3", 26);
    const canonical = JSON.parse(readFileSync(item.path, "utf8"));
    for (const version of [123, null]) {
      const document = structuredClone(canonical);
      document.releases[0].version = version;
      writeFileSync(item.path, JSON.stringify(document), "utf8");
      assert.throws(() => loadDocument(item.path), /version must be a string/);
    }
    for (const [field, value] of [["version", "vv1.2.3"], ["zh-CN", " 修复 1.2.3 "], ["zh-CN", "\uFEFF修复 1.2.3"]]) {
      const document = structuredClone(canonical);
      if (field === "version") document.releases[0].version = value;
      else document.releases[0].bugFixes[0][field] = value;
      writeFileSync(item.path, JSON.stringify(document), "utf8");
      assert.throws(() => loadDocument(item.path), ReleaseNotesError);
      assert.throws(() => parseDocumentBytes(readFileSync(item.path)), ReleaseNotesError);
    }
  } finally { item.cleanup(); }
});

test("check_rejects_wrong_latest_version", () => {
  const item = fixture();
  try {
    upsert(item.path, "2.0.0", 26);
    assert.equal(main(["check", "--file", item.path, "--expected-version", "2.0.1"]), 1);
  } finally { item.cleanup(); }
});

test("rejects_resource_over_one_mib_before_read_or_write", () => {
  const item = fixture();
  try {
    const oversized = "x".repeat(MAX_RELEASE_NOTES_BYTES);
    assert.throws(() => upsertRelease(item.path, {
      releaseDate: "2026-08-26", version: "1.2.3",
      featureOptimizationsZhCn: [oversized], featureOptimizationsEnUs: ["Improve release notes"],
      bugFixesZhCn: [], bugFixesEnUs: [],
    }), /1 MiB/);
    assert.equal(existsSync(item.path), false);
    writeFileSync(item.path, JSON.stringify({
      schemaVersion: 2,
      releases: [{
        releaseDate: "2026-08-26", version: "v1.2.3",
        featureOptimizations: [{ "zh-CN": oversized, "en-US": "Improve release notes" }], bugFixes: [],
      }],
    }), "utf8");
    assert.throws(() => loadDocument(item.path), /1 MiB/);
    assert.throws(() => parseDocumentBytes(readFileSync(item.path)), /1 MiB/);
  } finally { item.cleanup(); }
});

test("rejects_symlinked_release_notes", (context) => {
  const item = fixture();
  try {
    const target = join(item.root, "target.json");
    writeFileSync(target, '{"schemaVersion":2,"releases":[]}\n', "utf8");
    try {
      symlinkSync(target, item.path);
    } catch (error) {
      if (process.platform === "win32" && error.code === "EPERM") return context.skip("symlink privilege unavailable");
      throw error;
    }
    assert.throws(() => loadDocument(item.path), /regular file/);
    assert.throws(() => upsert(item.path, "1.0.0", 26), /regular file/);
  } finally {
    if (existsSync(item.path)) unlinkSync(item.path);
    item.cleanup();
  }
});
