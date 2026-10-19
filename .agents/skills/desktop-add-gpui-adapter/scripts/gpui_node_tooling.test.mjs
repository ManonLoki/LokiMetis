/** GPUI Node 工程入口的字段保留、失败关闭、迁移幂等与跨平台调用回归。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { GPUI_ENGINES, GPUI_SCRIPTS, renderGpuiPackageJson } from './gpui_node_tooling.mjs';
import { mergeGpuiNodeTooling, packageSnapshot, assertPackageSnapshot } from './merge_gpui_node_tooling.mjs';
import { validateGpui } from './gpui_validate.mjs';

const helperNames = [
  'desktop-add-gpui-adapter/scripts/gpui_dev.mjs',
  'desktop-add-gpui-adapter/scripts/gpui_validate.mjs',
  ...['check_file_line_limits.mjs', 'check_rust_chinese_comments.mjs', 'check_core_first.mjs'].map(name => `desktop-implement-change/scripts/${name}`),
  ...['release_git.mjs', 'release_notes.mjs', 'release_context.mjs'].map(name => `desktop-prepare-release/scripts/${name}`),
  'desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs',
  'desktop-build-gpui-release/scripts/build_gpui_release.mjs',
];

/** 最小迁移边界夹具只供文件操作断言；真实 pnpm 执行另用完整下游工具。 */
function fixture() {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'harness-gpui-node-test-'));
  fs.writeFileSync(path.join(root, 'Cargo.toml'), '[workspace]\nmembers = []\n\n[workspace.metadata.agent-first-harness]\ninterfaces = ["cli", "gui"]\ngui-framework = "gpui"\n');
  for (const name of helperNames) {
    const file = path.join(root, '.agents/skills', name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, '// 仅供边界检查的测试文件。\n');
  }
  return root;
}

/** 新工程只有标准库 Node 工具入口，不产生 npm 依赖、前端、下载或精确包管理器 pin。 */
test('new_package_declares_compatible_engines_and_real_retained_commands', () => {
  const document = JSON.parse(renderGpuiPackageJson());
  assert.deepEqual(document, { private: true, engines: GPUI_ENGINES, scripts: GPUI_SCRIPTS });
  assert.equal(Object.hasOwn(document, 'dependencies'), false);
  assert.equal(Object.hasOwn(document, 'packageManager'), false);
  assert.equal(document.scripts.dev, 'node .agents/skills/desktop-add-gpui-adapter/scripts/gpui_dev.mjs --root .');
  assert.match(document.scripts.test, /^cargo test --workspace --all-targets --all-features$/);
  assert.match(document.scripts['release:inspect'], /release_git\.mjs inspect --project-root \.$/);
  assert.match(document.scripts['release:git'], /git_lifecycle\.mjs release --project-root \.$/);
  assert.doesNotMatch(JSON.stringify(document), /validate_harness|run_harness_tests|tauri|react|pnpm install/);
});

/** 合并保留公开性、任意脚本、依赖、packageManager 与其他 engine，不把产品变成私有包。 */
test('existing_package_fields_are_preserved_and_complete_merge_is_byte_idempotent', () => {
  for (const privateFields of [{ private: false }, {}]) {
    const original = { name: 'user-owned', ...privateFields, scripts: { user: 'node user.mjs' }, engines: { npm: '>=10' }, packageManager: 'pnpm@12.4.2', dependencies: { owned: '1.0.0' }, custom: { values: [1, { a: 'b' }] } };
    const merged = JSON.parse(renderGpuiPackageJson(JSON.stringify(original)));
    for (const key of ['name', 'private', 'packageManager', 'dependencies', 'custom']) assert.deepEqual(merged[key], original[key]);
    assert.equal(Object.hasOwn(merged, 'private'), Object.hasOwn(original, 'private'));
    assert.equal(merged.scripts.user, original.scripts.user);
    assert.equal(merged.engines.npm, original.engines.npm);
    const bytes = ` ${JSON.stringify(merged)} \n`;
    assert.equal(renderGpuiPackageJson(Buffer.from(bytes)), bytes);
  }
});

/** 不解释或降低用户 engine 范围，也不替换任何非等值受管脚本。 */
test('conflicting_or_ambiguous_packages_are_rejected', () => {
  for (const source of [
    '{"engines":{"node":">=26.0.0"}}', '{"engines":{"pnpm":">=13.0.0"}}',
    '{"scripts":{"test":"user-test"}}', '{"engines":[]}', '{"scripts":{"other":true}}',
    '{"scripts":{"dev":"cargo run"}}',
    '{"private":"false"}', '[]', '{"scripts":{},"scripts":{}}', '{"custom":{"a":1,"\\u0061":2}}',
  ]) assert.throws(() => renderGpuiPackageJson(source));
  assert.throws(() => renderGpuiPackageJson(Buffer.from([0xff])), /encoded data/);
  assert.throws(() => renderGpuiPackageJson(Buffer.alloc(1024 * 1024 + 1)), /bounded/);
  assert.throws(() => renderGpuiPackageJson(JSON.stringify({ user: 'x'.repeat(1024 * 1024 - 100) })), /bounded/);
});

/** Windows CRLF 检出仍可迁移，第二次不改 package 原字节或产生 staging 残留。 */
test('migration_accepts_crlf_and_is_idempotent_without_ui_or_core_changes', () => {
  const root = fixture();
  try {
    const cargo = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8').replaceAll('\n', '\r\n');
    fs.writeFileSync(path.join(root, 'Cargo.toml'), cargo);
    fs.writeFileSync(path.join(root, 'package.json'), '{"private":false,"scripts":{"custom":"node user.mjs"}}');
    assert.equal(mergeGpuiNodeTooling(root).changed, true);
    const packageBytes = fs.readFileSync(path.join(root, 'package.json'));
    assert.equal(mergeGpuiNodeTooling(root).changed, false);
    assert.deepEqual(fs.readFileSync(path.join(root, 'package.json')), packageBytes);
    assert.equal(fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8'), cargo);
    assert.equal(fs.readdirSync(root).some(name => name.startsWith('.gpui-')), false);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

/** 重复或非 GPUI metadata 不能把受限迁移器用于未知工程。 */
test('migration_rejects_duplicate_metadata_keys_and_other_frameworks_before_write', () => {
  for (const change of [
    value => value.replace('interfaces =', 'interfaces = ["gui"]\ninterfaces ='),
    value => value.replace('gui-framework =', 'gui-framework = "gpui"\ngui-framework ='),
    value => value.replace('"gpui"', '"tauri"'),
    value => value.replace('["cli", "gui"]', '["gui", "gui"]'),
  ]) {
    const root = fixture();
    try {
      const file = path.join(root, 'Cargo.toml');
      fs.writeFileSync(file, change(fs.readFileSync(file, 'utf8')));
      assert.throws(() => mergeGpuiNodeTooling(root), /migration requires/);
      assert.equal(fs.existsSync(path.join(root, 'package.json')), false);
      assert.equal(fs.readdirSync(root).some(name => name.startsWith('.gpui-')), false);
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  }
});

/** 快照覆盖缺席与既有文件两态，用户在生成期间写入不会被静默覆盖。 */
test('package_snapshot_detects_concurrent_edits_and_new_files', () => {
  const root = fixture();
  try {
    const empty = packageSnapshot(root);
    fs.writeFileSync(path.join(root, 'package.json'), '{}');
    assert.throws(() => assertPackageSnapshot(root, empty), /changed during/);
    const original = packageSnapshot(root);
    fs.writeFileSync(path.join(root, 'package.json'), '{"user":true}');
    assert.throws(() => assertPackageSnapshot(root, original), /changed during/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

/** 缺失或链接的受管 helper 在创建 package 前失败，不留下不可运行的迁移。 */
test('migration_rejects_missing_or_symlinked_helpers_and_package', () => {
  const root = fixture();
  try {
    const helper = path.join(root, '.agents/skills', helperNames[0]);
    fs.unlinkSync(helper);
    assert.throws(() => mergeGpuiNodeTooling(root), /ENOENT/);
    fs.symlinkSync(path.join(root, 'Cargo.toml'), helper, 'file');
    assert.throws(() => mergeGpuiNodeTooling(root), /symbolic links/);
    fs.unlinkSync(helper);
    fs.writeFileSync(helper, '// test\n');
    fs.symlinkSync(path.join(root, 'Cargo.toml'), path.join(root, 'package.json'), 'file');
    assert.throws(() => mergeGpuiNodeTooling(root), /symbolic links/);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

/** runner 使用当前 Node 和参数数组串行调用保留门禁，汇总失败而不构建 Rust 或调用 Shell。 */
test('validation_runner_executes_three_retained_checks_and_propagates_failure', () => {
  const root = fixture();
  try {
    const calls = [];
    const status = validateGpui(root, (command, args, options) => {
      calls.push({ command, args, options });
      return { status: calls.length === 1 ? 7 : 2 };
    });
    assert.equal(status, 7);
    assert.equal(calls.length, 3);
    assert.deepEqual(calls.map(call => [path.basename(call.args[0]), call.args[1]]), [
      ['check_file_line_limits.mjs', '--root'], ['check_rust_chinese_comments.mjs', '--root'], ['check_core_first.mjs', '--workspace-root'],
    ]);
    for (const call of calls) {
      assert.equal(call.command, process.execPath);
      assert.equal(call.options.shell, false);
      assert.equal(call.options.cwd, root);
      assert.equal(call.args[2], root);
    }
    assert.equal(validateGpui(root, () => ({ status: 0 })), 0);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
