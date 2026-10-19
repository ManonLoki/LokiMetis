/** 产品验证入口必须接入精确登记，同时保留公共门禁与首次失败退出状态。 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { main, validateProject } from './validate.mjs';

/** 隔离工程只放所需脚本，不运行真实工具或访问用户配置。 */
function fixture() {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'lokimetris-validation-'));
  const scripts = ['scripts/check_project_line_limits.mjs', ...[
    'check_file_line_limits.mjs', 'check_rust_chinese_comments.mjs', 'check_core_first.mjs',
  ].map(name => '.agents/skills/desktop-implement-change/scripts/' + name)];
  for (const file of scripts) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), '// fixture\n');
  }
  return root;
}

/** 仅重定向行数入口；其余门禁即使前项失败仍以原参数执行。 */
test('project_validation_preserves_all_checks_and_first_failure', () => {
  const root = fixture();
  try {
    const calls = [];
    const status = validateProject(root, (command, args, options) => {
      calls.push({ command, args, options });
      return { status: calls.length === 1 ? 7 : 2 };
    });
    assert.equal(status, 7);
    assert.deepEqual(calls.map(call => [path.relative(root, call.args[0]), ...call.args.slice(1)]), [
      ['scripts/check_project_line_limits.mjs', '--root', root],
      ['.agents/skills/desktop-implement-change/scripts/check_rust_chinese_comments.mjs', '--root', root],
      ['.agents/skills/desktop-implement-change/scripts/check_core_first.mjs', '--workspace-root', root],
    ]);
    for (const call of calls) {
      assert.equal(call.command, process.execPath);
      assert.equal(call.options.shell, false);
      assert.equal(call.options.cwd, root);
    }
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

/** 非法参数不能成为任意检查器路径或 Shell 输入。 */
test('project_validation_rejects_unrecognized_cli_arguments', () => {
  assert.throws(() => main(['--check', 'arbitrary.mjs']), /Unknown option/);
  assert.throws(() => main(['arbitrary.mjs']), /positional/);
});

/** 产品包装器缺失或不是普通文件时，所有门禁都必须在执行前停止。 */
for (const kind of ['missing', 'directory']) {
  test(`project_validation_rejects_${kind}_wrapper_before_any_execution`, () => {
    const root = fixture();
    let calls = 0;
    try {
      const wrapper = path.join(root, 'scripts/check_project_line_limits.mjs');
      fs.unlinkSync(wrapper);
      if (kind === 'directory') fs.mkdirSync(wrapper);
      assert.throws(() => validateProject(root, () => { calls += 1; return { status: 0 }; }), /ENOENT|regular file/);
      assert.equal(calls, 0);
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  });
}

/** 末端脚本和父目录都不能通过链接把执行路径引向未经复核的内容。 */
for (const kind of ['file', 'parent']) {
  test(`project_validation_rejects_${kind}_symlink_before_any_execution`, context => {
    const root = fixture();
    let calls = 0;
    try {
      const original = path.join(root, kind === 'file' ? 'scripts/check_project_line_limits.mjs' : 'scripts');
      const moved = path.join(root, 'moved');
      fs.renameSync(original, moved);
      try { fs.symlinkSync(moved, original, kind === 'file' ? 'file' : 'dir'); }
      catch (error) {
        if (['EPERM', 'ENOSYS'].includes(error.code)) { context.skip('host disallows symbolic links'); return; }
        throw error;
      }
      assert.throws(() => validateProject(root, () => { calls += 1; return { status: 0 }; }), /symbolic links/);
      assert.equal(calls, 0);
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  });
}
