import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync,
  rmSync, symlinkSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import {
  readHarnessVersion,
  readHarnessVersionStamp,
  sameOpenedFile,
  selectHarnessVersion,
  shanghaiTimestamp,
  stampHarnessVersion,
  validHarnessTimestamp,
} from "./harness_version_clock.mjs";

/** 建立完全隔离的 Harness Git fixture，并在测试完成后清理。 */
function withHarness(name, callback) {
  test(name, () => {
    const root = mkdtempSync(join(tmpdir(), "harness-version-clock-"));
    const env = {
      ...process.env,
      GIT_CONFIG_GLOBAL: join(root, "global.gitconfig"),
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_TERMINAL_PROMPT: "0",
    };
    const git = (...args) => {
      const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8", env });
      assert.equal(result.status, 0, result.stderr);
      return result.stdout.trim();
    };
    try {
      git("init", "--quiet", "--initial-branch=main");
      git("config", "--local", "user.name", "Harness Clock Test");
      git("config", "--local", "user.email", "harness-clock@example.com");
      writeFileSync(join(root, "Version.md"), "- 当前版本：`202609090930`\n- 时间版本起始值：`202609090930`\n", "utf8");
      const skill = join(root, ".agents", "skills", "desktop-instantiate-project", "SKILL.md");
      mkdirSync(dirname(skill), { recursive: true });
      writeFileSync(skill, "# Harness\n", "utf8");
      git("add", ".");
      git("commit", "--quiet", "-m", "chore: baseline");
      callback({ root, git });
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("shanghai_timestamp_uses_explicit_zone_at_year_boundary", () => {
  assert.equal(shanghaiTimestamp(new Date("2026-12-31T16:01:00Z")), "202701010001");
  assert.equal(validHarnessTimestamp("202702300001"), false);
  assert.equal(validHarnessTimestamp("202701010001"), true);
});

test("opened Version.md accepts Windows lstat zero device but rejects a different inode", () => {
  const metadata = { dev: 0, ino: 562949953796197 };
  assert.equal(sameOpenedFile(metadata, { dev: 577193077, ino: metadata.ino, isFile: () => true }), true);
  assert.equal(sameOpenedFile(metadata, { dev: 577193077, ino: metadata.ino + 1, isFile: () => true }), false);
  assert.equal(sameOpenedFile({ dev: 2, ino: 3 }, { dev: 4, ino: 3, isFile: () => true }), false);
  assert.equal(sameOpenedFile(metadata, { dev: 577193077, ino: metadata.ino, isFile: () => false }), false);
});

withHarness("selector_uses_current_shanghai_minute_after_latest_tag", ({ root, git }) => {
  git("tag", "v202609090930-20260909");
  const selected = selectHarnessVersion(root, new Date("2026-09-09T01:31:12Z"));
  assert.deepEqual(selected, {
    status: "selected",
    version: "202609090931",
    previousTaggedVersion: "202609090930",
    recordedVersion: "202609090930",
  });
  assert.equal(readHarnessVersion(root), "202609090930");
});

withHarness("selector_rejects_same_minute_or_older_than_tag", ({ root, git }) => {
  git("tag", "v202609090930-20260909");
  assert.throws(
    () => selectHarnessVersion(root, new Date("2026-09-09T01:30:59Z")),
    /must be newer than tagged version 202609090930/,
  );
  assert.throws(
    () => selectHarnessVersion(root, new Date("2026-09-09T01:29:59Z")),
    /must be newer than tagged version 202609090930/,
  );
});

withHarness("selector_rejects_invalid_relevant_harness_tag", ({ root, git }) => {
  git("tag", "v202609090930-20261340");
  assert.throws(
    () => selectHarnessVersion(root, new Date("2026-09-09T01:31:00Z")),
    /invalid Harness release tag/,
  );
});

withHarness("selector_allows_tag_date_after_version_date", ({ root, git }) => {
  git("tag", "v202609090930-20260910");
  const selected = selectHarnessVersion(root, new Date("2026-09-09T01:31:00Z"));
  assert.equal(selected.previousTaggedVersion, "202609090930");
  assert.equal(selected.version, "202609090931");
});

withHarness("selector_rejects_recorded_target_newer_than_latest_tag", ({ root, git }) => {
  git("tag", "v202609090930-20260909");
  writeFileSync(join(root, "Version.md"), "- 当前版本：`202609090940`\n", "utf8");
  assert.throws(
    () => selectHarnessVersion(root, new Date("2026-09-09T01:35:00Z")),
    /already records untagged release target 202609090940; reuse it/,
  );
});

withHarness("stamp_updates_only_current_version_bytes_and_preserves_mode", ({ root, git }) => {
  git("tag", "v202609090930-20260909");
  const file = join(root, "Version.md");
  const original = Buffer.from("\ufeff# 版本\r\n\r\n- 当前版本：`202609090930`\r\n- 时间版本起始值：`202607301002`\r\n说明：保持原文。\r\n", "utf8");
  writeFileSync(file, original);
  chmodSync(file, 0o640);
  const result = stampHarnessVersion(root, new Date("2026-09-09T01:31:59Z"));
  assert.equal(result.status, "stamped");
  assert.equal(result.version, "202609090931");
  const expected = Buffer.from(original.toString("utf8").replace("202609090930", "202609090931"), "utf8");
  assert.deepEqual(readFileSync(file), expected);
  assert.equal(lstatSync(file).mode & 0o7777, 0o640);
  assert.equal(readdirSync(root).some((name) => name.startsWith(".Version.md.")), false);
  const receipt = readHarnessVersionStamp(root, "202609090931");
  assert.equal(receipt.schemaVersion, 1);
  assert.equal(receipt.stampedAt, "2026-09-09T01:31:59.000Z");
  assert.equal(receipt.version, "202609090931");
  assert.match(receipt.versionFileSha256, /^[0-9a-f]{64}$/u);
  writeFileSync(file, Buffer.from(expected.toString("utf8").replace("说明：保持原文。", "说明：已改动。"), "utf8"));
  assert.throws(
    () => readHarnessVersionStamp(root, "202609090931"),
    /Version.md changed after the managed Harness version stamp/,
  );
  writeFileSync(file, expected);
  assert.throws(
    () => stampHarnessVersion(root, new Date("2026-09-09T01:32:00Z")),
    /already records untagged release target 202609090931; reuse it/,
  );
  assert.deepEqual(readFileSync(file), expected);
});

withHarness("first_release_can_stamp_initial_version_once_without_a_tag", ({ root }) => {
  assert.equal(stampHarnessVersion(root, new Date("2026-09-09T01:31:00Z")).version, "202609090931");
  assert.throws(
    () => stampHarnessVersion(root, new Date("2026-09-09T01:32:00Z")),
    /already records untagged release target 202609090931; reuse it/,
  );
});

withHarness("stamp_rejects_symlink_and_directory_version_file", ({ root, git }) => {
  git("tag", "v202609090930-20260909");
  const file = join(root, "Version.md");
  const target = join(root, "elsewhere.md");
  writeFileSync(target, "- 当前版本：`202609090930`\n", "utf8");
  rmSync(file);
  symlinkSync(target, file);
  assert.throws(
    () => stampHarnessVersion(root, new Date("2026-09-09T01:31:00Z")),
    /Version.md must be a non-symbolic regular file/,
  );
  rmSync(file);
  mkdirSync(file);
  assert.throws(
    () => stampHarnessVersion(root, new Date("2026-09-09T01:31:00Z")),
    /Version.md must be a non-symbolic regular file/,
  );
});

withHarness("selector_rejects_malformed_related_tag_and_record_behind_tag", ({ root, git }) => {
  git("tag", "v202609090930-unknown");
  assert.throws(
    () => selectHarnessVersion(root, new Date("2026-09-09T01:31:00Z")),
    /invalid Harness release tag/,
  );
  git("tag", "-d", "v202609090930-unknown");
  git("tag", "v202609090940-20260909");
  assert.throws(
    () => selectHarnessVersion(root, new Date("2026-09-09T01:41:00Z")),
    /older than latest tag version 202609090940/,
  );
});

test("clock_cli_rejects_time_override", () => {
  const script = new URL("./harness_version_clock.mjs", import.meta.url);
  const result = spawnSync(process.execPath, [script.pathname, "stamp", "--project-root", ".", "--now", "2020-01-01"], {
    encoding: "utf8",
  });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /usage: harness_version_clock.mjs/);
});
