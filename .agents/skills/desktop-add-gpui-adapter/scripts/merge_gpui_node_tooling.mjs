#!/usr/bin/env node
/** 升级后按明确授权迁移 GPUI 根 Node 工程入口；不修改任何 core、UI 或发布事实。 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { assertRealPath } from './gpui_adapter_files.mjs';
import { renderGpuiPackageJson } from './gpui_node_tooling.mjs';

/** 对既有根 package 做有界快照；符号链接、目录与非 UTF-8 内容均不能参与合并。 */
export function packageSnapshot(root) {
  const file = path.join(root, 'package.json');
  assertRealPath(file, true);
  if (!fs.existsSync(file)) return null;
  const stat = fs.lstatSync(file);
  if (!stat.isFile() || stat.size > 1024 * 1024) throw new Error('package.json must be a bounded regular file');
  const bytes = fs.readFileSync(file);
  renderGpuiPackageJson(bytes);
  return bytes;
}

/** 安装前再次核对快照，不覆盖生成期间的用户修改或新出现的文件。 */
export function assertPackageSnapshot(root, original) {
  const current = packageSnapshot(root);
  if ((original === null) !== (current === null) || (original !== null && !original.equals(current))) {
    throw new Error('package.json changed during generation');
  }
}

/** 只接受已有 GPUI workspace 和实际保留工具，拒绝把迁移器当成另一个项目初始化入口。 */
function assertGpuiWorkspace(root) {
  assertRealPath(root);
  if (!fs.lstatSync(root).isDirectory()) throw new Error('root must be an existing workspace directory');
  const cargo = path.join(root, 'Cargo.toml');
  assertRealPath(cargo);
  if (!fs.lstatSync(cargo).isFile()) throw new Error('workspace manifest must be a regular file');
  const source = fs.readFileSync(cargo, 'utf8').replaceAll('\r\n', '\n');
  const sections = [...source.matchAll(/^\[([^\]\n]+)\][ \t]*(?:#[^\n]*)?$/gm)];
  const matches = sections.filter(match => match[1] === 'workspace.metadata.agent-first-harness');
  if (matches.length !== 1) throw new Error('expected unique Harness workspace metadata');
  const index = sections.indexOf(matches[0]);
  const metadata = source.slice(matches[0].index + matches[0][0].length, sections[index + 1]?.index ?? source.length);
  const framework = [...metadata.matchAll(/^[ \t]*gui-framework[ \t]*=[ \t]*([^\n]+)$/gm)];
  const interfaces = [...metadata.matchAll(/^[ \t]*interfaces[ \t]*=[ \t]*([^\n]+)$/gm)];
  if (framework.length !== 1 || !/^"gpui"[ \t]*(?:#[^\n]*)?$/.test(framework[0][1]) || interfaces.length !== 1 ||
      !/^\[(?:\s*"[a-z]+"\s*,)*\s*"[a-z]+"\s*,?\s*\][ \t]*(?:#[^\n]*)?$/.test(interfaces[0][1])) {
    throw new Error('migration requires an existing GPUI GUI workspace');
  }
  const selected = [...interfaces[0][1].split('#')[0].matchAll(/"([a-z]+)"/g)].map(match => match[1]);
  if (!selected.includes('gui') || new Set(selected).size !== selected.length) throw new Error('migration requires unique interfaces including gui');
  const helpers = [
    'desktop-add-gpui-adapter/scripts/gpui_validate.mjs',
    ...['check_file_line_limits.mjs', 'check_rust_chinese_comments.mjs', 'check_core_first.mjs'].map(name => `desktop-implement-change/scripts/${name}`),
    ...['release_git.mjs', 'release_notes.mjs', 'release_context.mjs'].map(name => `desktop-prepare-release/scripts/${name}`),
    'desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs',
    'desktop-build-gpui-release/scripts/build_gpui_release.mjs',
  ];
  for (const relative of helpers) {
    const file = path.join(root, '.agents/skills', relative);
    assertRealPath(file);
    if (!fs.lstatSync(file).isFile()) throw new Error('required retained Node helper must be a regular file');
  }
}

/** 全部冲突在写入前拒绝；替换单个根文件采用同文件系统原子 rename，重复迁移保留原字节。 */
export function mergeGpuiNodeTooling(rootValue) {
  const root = path.resolve(rootValue);
  assertGpuiWorkspace(root);
  const original = packageSnapshot(root);
  const content = renderGpuiPackageJson(original);
  if (original !== null && original.equals(Buffer.from(content))) return { root, changed: false };
  const stage = fs.mkdtempSync(path.join(root, '.gpui-node-tooling-'));
  try {
    const staged = path.join(stage, 'package.json');
    fs.writeFileSync(staged, content, { flag: 'wx' });
    if (original !== null) fs.chmodSync(staged, fs.lstatSync(path.join(root, 'package.json')).mode & 0o777);
    assertPackageSnapshot(root, original);
    if (original === null) fs.linkSync(staged, path.join(root, 'package.json'));
    else fs.renameSync(staged, path.join(root, 'package.json'));
  } finally { fs.rmSync(stage, { recursive: true, force: true }); }
  return { root, changed: true };
}

/** 迁移命令必须显式为 merge，避免无子命令的误操作。 */
export function main(argv = process.argv.slice(2)) {
  if (argv[0] !== 'merge') throw new Error('expected merge --root <workspace>');
  const { values } = parseArgs({ args: argv.slice(1), options: { root: { type: 'string' } }, allowPositionals: false });
  if (!values.root) throw new Error('--root is required');
  return mergeGpuiNodeTooling(values.root);
}

if (path.resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  try { process.stdout.write(`${JSON.stringify(main())}\n`); }
  catch (error) { process.stderr.write(`GPUI Node tooling: ${error.message}\n`); process.exitCode = 1; }
}
