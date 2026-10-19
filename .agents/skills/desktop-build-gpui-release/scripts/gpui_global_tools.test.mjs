/** 隔离 PATH 与模拟安装验证全局 CLI 选择，不改真实用户工具。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { findGlobalTool, requirePackager, setupPackager } from './gpui_global_tools.mjs';

/** 项目与全局目录相互独立，不读取测试宿主的真实 PATH。 */
function fixture(t) {
  const base = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'gpui-tools-'));
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const root = path.join(base, 'project'), global = path.join(base, 'global');
  fs.mkdirSync(root); fs.mkdirSync(global);
  return { root, global, tool: path.join(global, 'cargo-packager'), env: { PATH: global, CARGO_INSTALL_ROOT: global } };
}

/** 版本冲突不触发安装或覆盖，精确全局路径进入实际执行。 */
test('global_cli_exact_version_is_reused_and_conflicts_never_install', t => {
  const f = fixture(t); fs.writeFileSync(f.tool, 'fixture', { mode: 0o755 });
  const calls = [], run = (tool, args) => { calls.push([tool, args]); return { stdout: 'cargo-packager 0.11.8\n' }; };
  assert.equal(setupPackager(f.root, run, 'darwin', f.env), f.tool);
  assert.deepEqual(calls, [[f.tool, ['--version']]]);
  assert.throws(() => setupPackager(f.root, () => ({ stdout: 'cargo-packager 9.0.0\n' }), 'darwin', f.env), /different version/);
});

/** 仅显式 setup 安装全局工具且安装后重新解析路径与版本。 */
test('setup_installs_without_project_root_and_verifies_actual_path', t => {
  const f = fixture(t), calls = [];
  const run = (program, args) => {
    calls.push([program, args]);
    if (program === 'cargo') { fs.writeFileSync(f.tool, 'fixture', { mode: 0o755 }); return { stdout: '' }; }
    assert.equal(program, f.tool); return { stdout: 'cargo-packager 0.11.8\n' };
  };
  assert.equal(setupPackager(f.root, run, 'darwin', f.env), f.tool);
  assert.deepEqual(calls[0], ['cargo', ['install', 'cargo-packager', '--version', '0.11.8', '--locked', '--root', f.global]]);
  fs.rmSync(f.tool);
  assert.throws(() => setupPackager(f.root, () => ({ stdout: '' }), 'darwin', f.env), /missing from PATH/);
  assert.throws(() => requirePackager(f.root, () => { throw Error('must not execute'); }, 'darwin', f.env), /missing from PATH/);
});

/** 空与相对 PATH 不生效，项目私有或指回项目的链接不冒充全局工具。 */
test('path_resolution_rejects_project_tools_and_ignores_relative_entries', t => {
  const f = fixture(t), local = path.join(f.root, 'cargo-packager');
  fs.writeFileSync(local, 'fixture', { mode: 0o755 });
  assert.equal(findGlobalTool(f.root, 'cargo-packager', 'darwin', { PATH: ':relative' }), null);
  assert.throws(() => findGlobalTool(f.root, 'cargo-packager', 'darwin', { PATH: f.root }), /inside project/);
  fs.symlinkSync(local, f.tool);
  assert.throws(() => findGlobalTool(f.root, 'cargo-packager', 'darwin', f.env), /inside project/);
});

/** Windows 的标准 Path 大小写、分隔符与 exe 后缀被实际解析。 */
test('windows_path_case_and_executable_suffix_are_supported', t => {
  const f = fixture(t), executable = path.join(f.global, 'cargo-packager.exe');
  fs.writeFileSync(executable, 'fixture');
  assert.equal(findGlobalTool(f.root, 'cargo-packager', 'win32', { Path: `;relative;${f.global}` }), executable);
});

/** 环境和链接不能在 PATH 复核前先把独立 CLI 安进项目。 */
test('setup_rejects_project_install_destinations_before_cargo_runs', t => {
  const f = fixture(t), calls = [], run = (...args) => { calls.push(args); return { stdout: '' }; };
  for (const key of ['CARGO_INSTALL_ROOT', 'CARGO_HOME']) {
    assert.throws(() => setupPackager(f.root, run, 'darwin', { PATH: f.global, [key]: path.join(f.root, 'tools') }), /inside project/);
    assert.equal(calls.length, 0);
  }
  const link = path.join(f.global, 'linked-project'); fs.symlinkSync(f.root, link, 'junction');
  assert.throws(() => setupPackager(f.root, run, 'darwin', { PATH: f.global, CARGO_INSTALL_ROOT: path.join(link, 'tools') }), /inside project/);
  assert.equal(calls.length, 0);
});
