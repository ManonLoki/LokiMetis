#!/usr/bin/env node
/** 开发调试入口：用工程内皮肤资源目录启动 GUI，跨平台设置环境变量；额外参数透传给应用。 */
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const env = {
  ...process.env,
  LOKI_METRIS_DEV_SKIN_RESOURCES: process.env.LOKI_METRIS_DEV_SKIN_RESOURCES ?? path.join(root, 'loki_metris_gui', 'resources'),
};
const child = spawn('cargo', ['run', '-p', 'loki_metris_gui', '--', ...process.argv.slice(2)], { cwd: root, env, stdio: 'inherit' });
child.on('error', error => { console.error(error.message); process.exit(1); });
child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
