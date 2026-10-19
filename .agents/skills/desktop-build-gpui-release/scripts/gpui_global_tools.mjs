/** 独立 CLI 只从当前进程全局 PATH 解析，不复用项目内私有工具。 */
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { PACKAGER_VERSION } from './gpui_config.mjs';

/** 忽略空项和相对 PATH，拒绝项目工具伪装成全局安装；符号链接解析后再次检查。 */
export function findGlobalTool(root, name, platform = process.platform, environment = process.env) {
  if (!/^[a-z0-9-]+(?:\.exe)?$/iu.test(name)) throw new Error('invalid global tool name');
  const pathValue = Object.entries(environment).find(([key]) => platform === 'win32' ? key.toLowerCase() === 'path' : key === 'PATH')?.[1] ?? '';
  const filename = platform === 'win32' && !name.endsWith('.exe') ? `${name}.exe` : name;
  const project = fs.realpathSync(root);
  for (const directory of pathValue.split(platform === 'win32' ? ';' : ':')) {
    if (!path.isAbsolute(directory)) continue;
    const candidate = path.join(directory, filename);
    let actual;
    try {
      actual = fs.realpathSync(candidate);
      if (!fs.statSync(actual).isFile()) continue;
      if (platform !== 'win32') fs.accessSync(actual, fs.constants.X_OK);
    } catch { continue; }
    const relative = path.relative(project, actual);
    if (relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))) {
      throw new Error(`${name} resolves inside project; use a global CLI installation`);
    }
    return actual;
  }
  return null;
}

/** 固定兼容版本在实际执行路径上核对，不静默使用不同的同名全局 CLI。 */
export function requirePackager(root, run, platform = process.platform, environment = process.env) {
  const tool = findGlobalTool(root, 'cargo-packager', platform, environment);
  if (!tool) throw new Error('global cargo-packager is missing from PATH; run authorized setup');
  if (run(tool, ['--version']).stdout.trim() !== `cargo-packager ${PACKAGER_VERSION}`) {
    throw new Error(`global cargo-packager has a different version; requires ${PACKAGER_VERSION}, explicit tool upgrade/downgrade needed`);
  }
  return tool;
}

/** 显式指定全局根以隔离 Cargo install.root 配置，安装前拒绝项目内路径及链接。 */
export function globalInstallRoot(root, environment = process.env) {
  const configured = environment.CARGO_INSTALL_ROOT ?? environment.CARGO_HOME ?? path.join(os.homedir(), '.cargo');
  if (typeof configured !== 'string' || !path.isAbsolute(configured)) throw new Error('Cargo global install root must be absolute');
  let existing = path.resolve(configured);
  const suffix = [];
  while (!fs.existsSync(existing)) {
    const parent = path.dirname(existing);
    if (parent === existing) throw new Error('Cargo global install root cannot be resolved');
    suffix.unshift(path.basename(existing)); existing = parent;
  }
  if (!fs.statSync(existing).isDirectory()) throw new Error('Cargo global install root must be a directory');
  const actual = path.join(fs.realpathSync(existing), ...suffix);
  const relative = path.relative(fs.realpathSync(root), actual);
  if (relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))) {
    throw new Error('Cargo install root is inside project; select a user global location before setup');
  }
  return actual;
}

/** 仅显式 setup 安装到 Cargo 用户全局位置；安装后从 PATH 复读路径与实际版本。 */
export function setupPackager(root, run, platform = process.platform, environment = process.env) {
  const existing = findGlobalTool(root, 'cargo-packager', platform, environment);
  if (existing) return requirePackager(root, run, platform, environment);
  const destination = globalInstallRoot(root, environment);
  run('cargo', ['install', 'cargo-packager', '--version', PACKAGER_VERSION, '--locked', '--root', destination], { env: environment });
  return requirePackager(root, run, platform, environment);
}
