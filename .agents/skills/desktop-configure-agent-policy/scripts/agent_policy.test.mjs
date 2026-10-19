import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { CAPABILITY_FIELDS, inspect, main, migrateTaskWorktrees, setCapability } from "./agent_policy.mjs";
import { atomicWrite, readPolicy, setAction } from "../../desktop-switch-post-release-action/scripts/post_release_action.mjs";

const sourceRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
// 测试确认事实与能力固定，不继承已初始化下游的真实偏好。
const template = fs.readFileSync(path.join(sourceRoot, "docs/AGENT_POLICY.md"), "utf8")
  .replaceAll("\r\n", "\n")
  .replace(/^---\r?\n[\s\S]*?\r?\n---(?=\r?\n)/u, [
    "---", "schema_version: 5", "confirmed_by: previous-user", "confirmed_at: 2026-10-08",
    "decision_mode: reuse_then_infer_then_ask", ...CAPABILITY_FIELDS.map((field) => `${field}: disabled`),
    "post_release_action: push_release_branch", "---",
  ].join("\n"));

/** 在独立临时 Git 仓库中验证策略 writer，不访问实际用户策略。 */
function fixture(t, text = template) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "capability-policy-")));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const result = spawnSync("git", ["-C", root, "init", "-q"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  fs.mkdirSync(path.join(root, "docs"));
  const file = path.join(root, "docs/AGENT_POLICY.md");
  fs.writeFileSync(file, text);
  return { root, file, bytes: () => fs.readFileSync(file) };
}

/** 生成一次明确确认的单项修改，不替换任何实际用户信息。 */
function choice(field = "user_owned_tasks", extra = {}) {
  return { field, value: "enabled", expectedValue: "disabled", confirmedBy: "user-chat", confirmedAt: "2026-10-09", confirmedChoice: true, ...extra };
}

const legacy4 = template.replace("schema_version: 5", "schema_version: 4").replace("task_worktrees: disabled\n", "");
/** 摘要来自即将迁移的真实字节；确认来源只进入调用事实。 */
function migration(f, extra = {}) {
  return { value: "disabled", expectedSchema: "4", expectedSha256: crypto.createHash("sha256").update(f.bytes()).digest("hex"), confirmedBy: "migration-user", confirmedAt: "2026-10-10", confirmedChoice: true, ...extra };
}

/** 旧策略保留原五项和动作的读写，不把缺少工作树选择补为关闭。 */
test("legacy_readers_keep_selection_required_and_original_writers", (t) => {
  const f = fixture(t, legacy4);
  assert.deepEqual(inspect(f.root).task_worktrees, { status: "selection_required", value: null });
  assert.throws(() => setCapability(f.root, choice("task_worktrees")), /selection_required/u);
  assert.equal(setCapability(f.root, choice()).schema_version, 4);
  assert.equal(setAction(f.root, "push_release_branch", "push_release_branch", { confirmedChoice: true }).changed, false);
  assert.ok(!Object.hasOwn(inspect(f.root).capabilities, "task_worktrees"));
  const v3 = fixture(t, legacy4.replace("schema_version: 4", "schema_version: 3").replace("post_release_action: push_release_branch\n", ""));
  assert.equal(inspect(v3.root).schema_version, 3);
  assert.equal(inspect(v3.root).task_worktrees.status, "selection_required");
});

/** 受限迁移只提升 schema 并插入选项，CRLF、权限、正文和旧确认逐字保留。 */
test("migration_preserves_original_facts_and_same_value_is_noop", (t) => {
  for (const value of ["enabled", "disabled"]) {
    const f = fixture(t, legacy4.replaceAll("\n", "\r\n"));
    fs.chmodSync(f.file, 0o640);
    const originalMode = fs.statSync(f.file).mode & 0o777;
    const before = f.bytes().toString();
    const options = migration(f, { value });
    assert.equal(migrateTaskWorktrees(f.root, options).schema_version, 5);
    assert.equal(f.bytes().toString(), before.replace("schema_version: 4", "schema_version: 5").replace("user_owned_tasks: disabled\r\n", `user_owned_tasks: disabled\r\ntask_worktrees: ${value}\r\n`));
    const bytes = f.bytes();
    const stat = fs.statSync(f.file);
    assert.equal(stat.mode & 0o777, originalMode);
    assert.equal(migrateTaskWorktrees(f.root, options).changed, false);
    assert.deepEqual(f.bytes(), bytes);
    assert.equal(fs.statSync(f.file).ino, stat.ino);
    assert.equal(fs.statSync(f.file).mtimeMs, stat.mtimeMs);
    assert.throws(() => migrateTaskWorktrees(f.root, { ...options, value: value === "enabled" ? "disabled" : "enabled" }), /正常 set/u);
  }
});

/** 摘要漂移、非法选择、未合并正文或非完整旧 schema 均零写入。 */
test("migration_fails_closed_on_drift_invalid_schema_and_stale_body", (t) => {
  for (const text of [
    legacy4,
    legacy4.replaceAll("environment.type=local", "removed-local"),
    `${legacy4}\nGit user-owned Task 固定使用独立 Worktree\n`,
    legacy4.replace("e2e_hint: disabled\n", ""),
    legacy4.replace("e2e_hint: disabled", "e2e_hint: disabled\ne2e_hint: enabled"),
    legacy4.replace("e2e_hint: disabled", "e2e_hint: pending"),
    legacy4.replace("e2e_hint: disabled", "e2e_hint: disabled\nextra: disabled"),
    legacy4.replace("schema_version: 4", "schema_version: 3").replace("post_release_action: push_release_branch\n", ""),
  ]) {
    const f = fixture(t, text);
    const before = f.bytes();
    const options = migration(f, text === legacy4 ? { expectedSha256: "0".repeat(64) } : {});
    assert.throws(() => migrateTaskWorktrees(f.root, options));
    assert.deepEqual(f.bytes(), before);
  }
  const f = fixture(t, legacy4);
  for (const extra of [{ confirmedChoice: false }, { value: "pending" }, { expectedSchema: "3" }, { expectedSha256: "bad" }, { confirmedBy: "pending" }, { confirmedAt: "2026-02-30" }]) {
    assert.throws(() => migrateTaskWorktrees(f.root, migration(f, extra)));
    assert.equal(f.bytes().toString(), legacy4);
  }
  fs.writeFileSync(path.join(f.root, "docs/.AGENT_POLICY.post-release.lock"), "owned-lock");
  assert.throws(() => migrateTaskWorktrees(f.root, migration(f)), /锁/u);
  assert.equal(f.bytes().toString(), legacy4);
});

/** CLI 不接受缺少确认或额外参数，完整调用可复读单一选择。 */
test("migration_cli_requires_explicit_confirmation_and_exact_arguments", (t) => {
  const f = fixture(t, legacy4);
  const options = migration(f);
  const args = ["migrate-task-worktrees", "--project-root", f.root, "--value", "disabled", "--expected-schema", "4", "--expected-sha256", options.expectedSha256, "--confirmed-by", options.confirmedBy, "--confirmed-at", options.confirmedAt];
  assert.throws(() => main(args), /缺少/u);
  assert.throws(() => main([...args, "--confirmed-user-choice", "--field", "e2e_hint"]), /未知/u);
  assert.equal(main([...args, "--confirmed-user-choice"]).task_worktrees.value, "disabled");
});

/** 每种能力只修改自身与确认字段，并保留 CRLF、正文、发布动作及其他能力。 */
test("single_capability_changes_preserve_unselected_fields_body_and_crlf", (t) => {
  for (const field of CAPABILITY_FIELDS) {
    const f = fixture(t, template.replaceAll("\n", "\r\n"));
    const before = f.bytes().toString();
    const result = setCapability(f.root, choice(field));
    assert.equal(result.changed, true);
    assert.equal(result.capabilities[field], "enabled");
    const expected = before.replace(`${field}: disabled`, `${field}: enabled`)
      .replace("confirmed_by: previous-user", "confirmed_by: user-chat").replace("confirmed_at: 2026-10-08", "confirmed_at: 2026-10-09");
    assert.equal(f.bytes().toString(), expected);
    assert.equal(result.post_release_action, "push_release_branch");
  }
});

/** 同值修改不触碰字节、权限、inode或原始确认事实，inspect也不创建锁。 */
test("same_value_and_inspect_are_byte_preserving", (t) => {
  const f = fixture(t);
  const bytes = f.bytes();
  const before = fs.statSync(f.file);
  const result = setCapability(f.root, choice("user_owned_tasks", { value: "disabled" }));
  assert.equal(result.changed, false);
  assert.deepEqual(f.bytes(), bytes);
  assert.equal(fs.statSync(f.file).ino, before.ino);
  assert.equal(inspect(f.root).confirmed_by, "previous-user");
  assert.deepEqual(fs.readdirSync(path.dirname(f.file)), ["AGENT_POLICY.md"]);
});

/** 非法或未经确认的写入以及旧值漂移均失败且不改变文件。 */
test("invalid_choices_and_stale_expected_values_fail_without_writes", (t) => {
  const f = fixture(t);
  const before = f.bytes();
  for (const extra of [
    { confirmedChoice: false }, { field: "post_release_action" }, { field: "schema_version" },
    { value: "pending" }, { expectedValue: "enabled" }, { confirmedBy: "pending" },
    { confirmedBy: "a\nuser_owned_tasks: enabled" }, { confirmedBy: "user # comment" }, { confirmedAt: "2026-02-30" },
    { confirmedAt: "2026-02-30T00:00:00Z" }, { confirmedAt: "2026-10-09T00:00:00" },
    { confirmedAt: "2026-10-09T24:00:00Z" }, { confirmedAt: "2026-10-09T00:00:00+24:00" },
    ...["- user", "? user", "@user", "`user"].map((confirmedBy) => ({ confirmedBy })),
  ]) {
    assert.throws(() => setCapability(f.root, choice("user_owned_tasks", extra)));
    assert.deepEqual(f.bytes(), before);
  }
  assert.throws(() => main(["inspect", "--project-root", f.root, "--project-root", f.root]), /重复/u);
  assert.throws(() => main(["inspect", "--project-root"]), /缺少/u);
  assert.throws(() => main(["upsert", "--project-root", f.root]), /命令/u);
});

/** 旧 schema、未确认字段和重复frontmatter均不能被能力入口自动修复。 */
test("unsupported_or_unconfirmed_policies_fail_closed", (t) => {
  for (const text of [
    template.replace("schema_version: 5", "schema_version: 3").replace("task_worktrees: disabled\n", "").replace("post_release_action: push_release_branch\n", ""),
    template.replace("superpowers: disabled", "superpowers: pending"),
    template.replace("task_worktrees: disabled", "task_worktrees: pending"),
    template.replace("task_worktrees: disabled", "task_worktrees: automatic"),
    template.replace("e2e_hint: disabled", "e2e_hint: disabled\ne2e_hint: enabled"),
    template.replace("post_release_action: push_release_branch", "post_release_action: pending"),
  ]) {
    const f = fixture(t, text);
    assert.throws(() => setCapability(f.root, choice()));
    assert.equal(f.bytes().toString(), text);
  }
});

/** 发布动作与能力修改看到同一把锁，不删除已有锁或并发覆盖。 */
test("both_policy_writers_share_the_same_lock", (t) => {
  const f = fixture(t);
  const lock = path.join(f.root, "docs/.AGENT_POLICY.post-release.lock");
  fs.writeFileSync(lock, "owned-lock");
  const before = f.bytes();
  assert.throws(() => setCapability(f.root, choice()), /锁/u);
  assert.throws(() => setAction(f.root, "push_release_branch", "push_release_branch", { confirmedChoice: true }), /锁/u);
  assert.equal(fs.readFileSync(lock, "utf8"), "owned-lock");
  assert.deepEqual(f.bytes(), before);
});

/** 能力配置不依赖本地打包资产，原发布动作仍保留自己的适用性检查。 */
test("capability_changes_do_not_disable_post_release_validation", (t) => {
  const f = fixture(t, template.replace("post_release_action: push_release_branch", "post_release_action: local_package"));
  assert.equal(setCapability(f.root, choice()).post_release_action, "local_package");
  assert.throws(() => setAction(f.root, "push_release_branch", "local_package", { confirmedChoice: true }));
});

/** 原子写入拒绝读取后的真实字节漂移并清理临时文件。 */
test("atomic_write_refuses_drift_and_cleans_its_temporary_file", (t) => {
  const f = fixture(t);
  const policy = readPolicy(f.root, { validateActionSupport: false });
  fs.appendFileSync(f.file, "user edit\n");
  const before = f.bytes();
  assert.throws(() => atomicWrite(policy, template), /变化/u);
  assert.deepEqual(f.bytes(), before);
  assert.deepEqual(fs.readdirSync(path.dirname(f.file)), ["AGENT_POLICY.md"]);
});

/** 权限恢复遭文件系统拒绝时，保留原策略及元数据，并清理未提交的临时文件。 */
test("atomic_write_permission_failure_preserves_original_and_cleans_temporary_file", (t) => {
  const f = fixture(t);
  const policy = readPolicy(f.root, { validateActionSupport: false });
  const before = fs.statSync(f.file);
  t.mock.method(fs, "fchmodSync", () => { throw Object.assign(new Error("permission restoration denied"), { code: "EPERM" }); });
  assert.throws(() => atomicWrite(policy, template.replace("e2e_hint: disabled", "e2e_hint: enabled")), { code: "EPERM" });
  assert.deepEqual(f.bytes(), policy.bytes);
  const after = fs.statSync(f.file);
  assert.equal(after.mode, before.mode);
  assert.equal(after.ino, before.ino);
  assert.equal(after.mtimeMs, before.mtimeMs);
  assert.deepEqual(fs.readdirSync(path.dirname(f.file)), ["AGENT_POLICY.md"]);
});

/** Harness 源与父仓库子目录都不接受下游永久能力写入。 */
test("source_harness_and_nonroot_targets_are_rejected", (t) => {
  const f = fixture(t);
  fs.mkdirSync(path.join(f.root, "nested"));
  assert.throws(() => inspect(path.join(f.root, "nested")), /独立/u);
  fs.mkdirSync(path.join(f.root, ".agents/skills/desktop-instantiate-project"), { recursive: true });
  fs.writeFileSync(path.join(f.root, ".agents/skills/desktop-instantiate-project/SKILL.md"), "fixture");
  fs.writeFileSync(path.join(f.root, "Version.md"), "fixture");
  const before = f.bytes();
  assert.throws(() => setCapability(f.root, choice()), /Harness/u);
  assert.deepEqual(f.bytes(), before);
});

/** 写入保留实际文件权限，且带时区的真实确认时间可复读。 */
test("updates_preserve_mode_and_accept_valid_zoned_timestamps", (t) => {
  const f = fixture(t);
  fs.chmodSync(f.file, 0o640);
  const mode = fs.statSync(f.file).mode & 0o777;
  setCapability(f.root, choice("user_owned_tasks", { confirmedAt: "2026-10-09T12:34:56.123+08:00" }));
  assert.equal(fs.statSync(f.file).mode & 0o777, mode);
  assert.equal(inspect(f.root).confirmed_at, "2026-10-09T12:34:56.123+08:00");
});

// 在独立进程固定 umask，观察真实文件权限与完整策略字节，不改变测试宿主的全局掩码。
for (const command of ["set", "migrate-task-worktrees"]) {
  for (const mode of [0o640, 0o664, 0o764]) {
    /** 普通切换与受限迁移均须保留 umask 会过滤的已有权限，且只产生预期策略变化。 */
    test(`${command.replaceAll("-", "_")}_preserves_${mode.toString(8)}_under_umask_0022`, {
      skip: process.platform === "win32" ? "Windows 不提供 POSIX umask 与完整权限位；需在 POSIX 宿主执行" : false,
    }, (t) => {
      const f = fixture(t, command === "set" ? template : legacy4);
      fs.chmodSync(f.file, mode);
      const before = f.bytes().toString();
      assert.equal(fs.statSync(f.file).mode & 0o777, mode);
      const options = migration(f, { value: "enabled" });
      const args = command === "set"
        ? [command, "--field", "task_worktrees", "--value", "enabled", "--expected-value", "disabled"]
        : [command, "--value", "enabled", "--expected-schema", "4", "--expected-sha256", options.expectedSha256];
      const script = new URL("./agent_policy.mjs", import.meta.url).href;
      const result = spawnSync(process.execPath, ["--input-type=module", "--eval",
        `process.umask(0o022); const { main } = await import(${JSON.stringify(script)}); console.log(JSON.stringify(main(process.argv.slice(1))));`,
        ...args, "--project-root", f.root, "--confirmed-by", options.confirmedBy,
        "--confirmed-at", options.confirmedAt, "--confirmed-user-choice",
      ], { encoding: "utf8" });
      assert.equal(result.status, 0, result.stderr || result.stdout);
      assert.equal(JSON.parse(result.stdout).changed, true);
      assert.equal(fs.statSync(f.file).mode & 0o777, mode);
      const expected = command === "set"
        ? before.replace("task_worktrees: disabled", "task_worktrees: enabled")
          .replace("confirmed_by: previous-user", `confirmed_by: ${options.confirmedBy}`)
          .replace("confirmed_at: 2026-10-08", `confirmed_at: ${options.confirmedAt}`)
        : before.replace("schema_version: 4", "schema_version: 5")
          .replace("user_owned_tasks: disabled\n", "user_owned_tasks: disabled\ntask_worktrees: enabled\n");
      assert.equal(f.bytes().toString(), expected);
      assert.deepEqual(fs.readdirSync(path.dirname(f.file)), ["AGENT_POLICY.md"]);
    });
  }
}

/** 实际支持符号链接的宿主必须拒绝根、docs和策略文件链接。 */
test("symlinked_policy_paths_are_rejected_without_touching_the_target", (t) => {
  for (const kind of ["root", "docs", "policy"]) {
    const f = fixture(t);
    const original = kind === "root" ? f.root : kind === "docs" ? path.dirname(f.file) : f.file;
    const moved = `${original}-real`;
    fs.renameSync(original, moved);
    t.after(() => fs.rmSync(moved, { recursive: true, force: true }));
    try { fs.symlinkSync(moved, original, kind === "policy" ? "file" : "dir"); }
    catch (error) {
      fs.renameSync(moved, original);
      if (["EPERM", "EACCES", "ENOSYS", "ENOTSUP"].includes(error.code)) { t.skip(`宿主不支持创建测试链接: ${error.code}`); return; }
      throw error;
    }
    const before = fs.readFileSync(f.file);
    assert.throws(() => setCapability(f.root, choice()));
    assert.deepEqual(fs.readFileSync(f.file), before);
  }
});
