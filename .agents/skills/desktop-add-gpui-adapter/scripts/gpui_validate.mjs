#!/usr/bin/env node
/** 在下游依次执行保留的机械检查；不依赖源 Harness 专属 runner、前端或 Shell 链。 */
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { assertRealPath } from './gpui_adapter_files.mjs';

const checks = [
  ['check_file_line_limits.mjs', '--root'],
  ['check_rust_chinese_comments.mjs', '--root'],
  ['check_core_first.mjs', '--workspace-root'],
];

/** 串行汇总三个保留的 Node 工程门禁，返回首个非零状态。 */
export function validateGpui(rootValue, run = spawnSync) {
  const root = path.resolve(rootValue);
  assertRealPath(root);
  if (!fs.lstatSync(root).isDirectory()) throw new Error('validation root must be a directory');
  const commands = checks.map(([name, option]) => [path.join(root, '.agents/skills/desktop-implement-change/scripts', name), option]);
  for (const [command] of commands) {
    assertRealPath(command);
    if (!fs.lstatSync(command).isFile()) throw new Error('validation helper must be a regular file');
  }
  let status = 0;
  for (const [command, option] of commands) {
    const result = run(process.execPath, [command, option, root], { cwd: root, stdio: 'inherit', shell: false });
    if (result.error) throw result.error;
    if (result.status !== 0 && status === 0) status = result.status ?? 1;
  }
  return status;
}

/** CLI 仅接收唯一项目根，不把任意脚本名或 Shell 内容作为参数执行。 */
export function main(argv = process.argv.slice(2)) {
  const { values, positionals } = parseArgs({ args: argv, options: { root: { type: 'string', default: process.cwd() } }, allowPositionals: false });
  if (positionals.length) throw new Error('unexpected positional argument');
  return validateGpui(values.root);
}

if (path.resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  try { process.exitCode = main(); }
  catch (error) { process.stderr.write(`GPUI validation: ${error.message}\n`); process.exitCode = 1; }
}
