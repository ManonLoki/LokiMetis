/** Git publish 冻结 journal、部分失败消息与恢复语义。 */

import test from "node:test";
import assert from "node:assert/strict";
import { lifecycleFixture } from "./git_lifecycle_test_support.mjs";
import { pendingFailure } from "./git_publication_report.mjs";

test("single_target_pending_errors_preserve_stable_codes", () => {
  const pending = {
    head: "e".repeat(40),
    targets: [{ remote: "origin", branch: "main", confirmed: false }],
  };
  const cases = {
    "push-failed": "push-rejected",
    "verification-failed": "remote-verification-failed",
    "local-state-changed": "local-state-changed",
    "state-write-failed": "state-write-failed",
  };
  for (const [kind, expected] of Object.entries(cases)) {
    assert.equal(pendingFailure(pending, 0, kind, "detail", "outcome")[0], expected);
  }
});

test("additional_drift_reports_later_confirmed_targets", () => {
  const pending = {
    head: "e".repeat(40),
    targets: [
      { remote: "github", branch: "main", confirmed: true },
      { remote: "origin", branch: "stable", confirmed: true },
      { remote: "backup", branch: "integration", confirmed: true },
      { remote: "archive", branch: "delivery", confirmed: false },
    ],
  };
  const [code, message] = pendingFailure(
    pending, 1, "verification-failed",
    "no longer matches the frozen published HEAD",
    "previous confirmation has changed",
  );
  assert.equal(code, "additional-verification-failed");
  assert.match(message, /later confirmed additional targets.*remote 'backup' branch 'integration'/);
  assert.match(message, /not attempted.*remote 'archive' branch 'delivery'/);
});

test("publish_primary_only_failure_persists_and_resumes_frozen_head", () => {
  const item = lifecycleFixture();
  try {
    const { repository, bare } = item.initializeRepository({ remote: true });
    item.helper(repository, ["start", "--summary", "retry-primary-only"]);
    const expectedHead = item.commitFile(repository, "retry-primary-only.txt", "retry me\n");
    item.installHook(bare, 'while read old new ref; do\n  if [ "$ref" = "refs/heads/main" ]; then exit 1; fi\ndone\n');
    const rejected = item.helper(repository, ["publish"], { success: false }).payload;
    assert.equal(rejected.code, "push-rejected");
    assert.match(rejected.message, /could not be confirmed/);
    assert.deepEqual(item.state(repository).pendingPublish, {
      head: expectedHead,
      targets: [{ remote: "origin", branch: "main", confirmed: false }],
    });
    item.installHook(bare, "while read old new ref; do :; done\n");
    const resumed = item.helper(repository, ["publish"]).payload;
    assert.equal(resumed.status, "published");
    assert.equal(resumed.head, expectedHead);
    assert.equal(item.state(repository).pendingPublish, null);
  } finally {
    item.cleanup();
  }
});

test("additional_remote_rejection_preserves_prefix_and_retry_succeeds", () => {
  const item = lifecycleFixture();
  try {
    const { repository } = item.initializeRepository({ remote: true });
    item.git(repository, "remote", "rename", "origin", "github");
    item.addBareRemote(repository, "origin", "stable");
    const backup = item.addBareRemote(repository, "backup", "integration");
    item.addBareRemote(repository, "archive", "delivery");
    item.helper(repository, ["start", "--summary", "retry-additional", "--remote", "github"]);
    const expectedHead = item.commitFile(repository, "retry-additional.txt", "retry me\n");
    item.installHook(backup, 'while read old new ref; do\n  if [ "$ref" = "refs/heads/integration" ]; then exit 1; fi\ndone\n');
    const args = [
      "publish", "--remote", "github", "--also-remote", "origin",
      "--also-remote", "backup", "--also-remote", "archive",
    ];
    const rejected = item.helper(repository, args, { success: false }).payload;
    assert.equal(rejected.code, "additional-push-failed");
    assert.match(rejected.message, /remote 'backup' branch 'integration'/);
    assert.match(rejected.message, /remote 'archive' branch 'delivery'/);
    const pending = item.state(repository).pendingPublish;
    assert.equal(pending.head, expectedHead);
    assert.deepEqual(pending.targets.map((target) => target.confirmed), [true, true, false, false]);
    const changed = item.helper(repository, args.slice(0, -2), { success: false }).payload;
    assert.equal(changed.code, "publish-in-progress");
    item.installHook(backup, "while read old new ref; do :; done\n");
    const resumed = item.helper(repository, args).payload;
    assert.equal(resumed.status, "published");
    assert.equal(resumed.head, expectedHead);
    assert.deepEqual(resumed.publishedRemotes, [
      { remote: "github", branch: "main" },
      { remote: "origin", branch: "stable" },
      { remote: "backup", branch: "integration" },
      { remote: "archive", branch: "delivery" },
    ]);
    assert.equal(item.state(repository).pendingPublish, null);
  } finally {
    item.cleanup();
  }
});
