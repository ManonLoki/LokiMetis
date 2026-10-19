#!/usr/bin/env node
/** 复用 GPUI 工程 runner，仅将行数检查接到本项目的精确摘要登记层。 */
import path from 'node:path';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { validateGpui } from '../.agents/skills/desktop-add-gpui-adapter/scripts/gpui_validate.mjs';
import { assertRealPath } from '../.agents/skills/desktop-add-gpui-adapter/scripts/gpui_adapter_files.mjs';

/** 保留全部三项门禁、参数和失败传播，只替换唯一已知公共行数入口。 */
export function validateProject(rootValue, run = spawnSync) {
  const root = path.resolve(rootValue);
  const common = path.join(root, '.agents/skills/desktop-implement-change/scripts/check_file_line_limits.mjs');
  const project = path.join(root, 'scripts/check_project_line_limits.mjs');
  assertRealPath(project);
  if (!fs.lstatSync(project).isFile()) throw new Error('project validation helper must be a regular file');
  return validateGpui(root, (command, args, options) => run(command,
    args[0] === common ? [project, ...args.slice(1)] : args, options));
}

/** 产品入口只接受显式项目根，其他参数继续失败关闭。 */
export function main(args = process.argv.slice(2)) {
  const { values } = parseArgs({ args, options: { root: { type: 'string', default: process.cwd() } } });
  return validateProject(values.root);
}

if (path.resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  try { process.exitCode = main(); }
  catch (error) { process.stderr.write(`Project validation: ${error.message}\n`); process.exitCode = 1; }
}
