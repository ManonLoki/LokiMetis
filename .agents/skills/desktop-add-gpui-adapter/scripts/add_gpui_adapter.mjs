#!/usr/bin/env node
/** 仅向已有共享 core 的下游增加 GPUI；不创建第二个项目或覆盖已有 GUI。 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertRealPath, normalizeOptions, renderGpuiAdapterFiles } from './gpui_adapter_files.mjs';
import { renderGpuiPackageJson } from './gpui_node_tooling.mjs';
import { packageSnapshot, assertPackageSnapshot } from './merge_gpui_node_tooling.mjs';
import { nativeDependencies } from './gpui_native_capabilities.mjs';

/** 只接受可明确定位的唯一 TOML 节，非受支持布局失败而不猜测写入位置。 */
function section(source, name) {
  const matches = [...source.matchAll(/^\[([^\]\n]+)\][ \t]*(?:#[^\n]*)?$/gm)];
  const indices = matches.map((match, index) => ({ name: match[1], start: match.index + match[0].length, end: matches[index + 1]?.index ?? source.length }));
  const found = indices.filter(value => value.name === name);
  if (found.length !== 1) throw new Error(`expected one [${name}] section`);
  return found[0];
}

/** 限定单行标量或字符串数组的键；复杂内联表/转义布局交给显式人工合并。 */
function field(source, name, key, required = true) {
  const range = section(source, name);
  const body = source.slice(range.start, range.end);
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const matches = [...body.matchAll(new RegExp(`^[ \\t]*${escaped}[ \\t]*=[ \\t]*([^\\n]+)$`, 'gm'))];
  if (matches.length !== 1) {
    if (!required && matches.length === 0) return null;
    throw new Error(`expected one supported ${name}.${key} field`);
  }
  return { range, body, match: matches[0], value: matches[0][1].trim() };
}

/** 简单字符串数组不接收注释注入、未知表达式或重复项目。 */
function array(value, key) {
  if (!/^\[\s*(?:"[^"\\\n]*"\s*(?:,\s*"[^"\\\n]*"\s*)*,?\s*)?\]$/.test(value)) throw new Error(`unsupported array syntax for ${key}; merge explicitly`);
  const values = [...value.matchAll(/"([^"\\\n]*)"/g)].map(match => match[1]);
  if (new Set(values).size !== values.length) throw new Error(`duplicate array entries in ${key}`);
  return values;
}

/** 定点替换已验证键值，保留其余 Cargo 内容和用户修改。 */
function replaceField(source, name, key, value) {
  const found = field(source, name, key);
  const start = found.range.start + found.match.index;
  return source.slice(0, start) + `${key} = ${value}` + source.slice(start + found.match[0].length);
}

/** 不存在的兼容依赖才新增，已有冲突依赖失败而不静默改版本。 */
function dependency(source, key, value) {
  const found = field(source, 'workspace.dependencies', key, false);
  if (found) {
    if (found.value !== value) throw new Error(`conflicting workspace dependency ${key}`);
    return source;
  }
  const range = section(source, 'workspace.dependencies');
  return source.slice(0, range.end).trimEnd() + `\n${key} = ${value}\n\n` + source.slice(range.end);
}

/** 既有 core 与 Cargo 是先决条件，只增加 GUI、profile 与独立 GPUI 中性打包配置。 */
export function addGpuiAdapter(input) {
  if (typeof input.target !== 'string' || !path.isAbsolute(input.target)) throw new Error('target must be an existing absolute workspace path');
  const target = path.normalize(input.target);
  assertRealPath(target);
  const options = normalizeOptions(input);
  const cargoPath = path.join(target, 'Cargo.toml');
  const corePath = path.join(target, `${options.projectId}_core`, 'Cargo.toml');
  assertRealPath(cargoPath);
  assertRealPath(corePath);
  if (!fs.lstatSync(cargoPath).isFile() || !fs.lstatSync(corePath).isFile()) throw new Error('workspace and core Cargo manifests must be regular files');
  const original = fs.readFileSync(cargoPath, 'utf8');
  const core = fs.readFileSync(corePath, 'utf8');
  if (field(core, 'package', 'name').value !== JSON.stringify(`${options.projectId}_core`)) throw new Error('existing core package identity does not match');
  const members = array(field(original, 'workspace', 'members').value, 'members');
  if (!members.includes(`${options.projectId}_core`)) throw new Error('existing core must be an explicit workspace member');
  const metadata = 'workspace.metadata.agent-first-harness';
  const project = field(original, metadata, 'project-id', false);
  if (project && project.value !== JSON.stringify(options.projectId)) throw new Error('workspace project identity does not match');
  const interfaces = array(field(original, metadata, 'interfaces').value, 'interfaces');
  if (interfaces.includes('gui') || members.includes(`${options.projectId}_gui`) || fs.existsSync(path.join(target, `${options.projectId}_gui`))) throw new Error('an existing GUI must not be overwritten');
  const platforms = array(field(original, metadata, 'target-platforms').value, 'target-platforms');
  if (JSON.stringify([...platforms].sort()) !== JSON.stringify([...options.targetPlatforms].sort())) throw new Error('target platforms must match the existing workspace');
  const framework = field(original, metadata, 'gui-framework', false);
  if (framework && framework.value !== '"gpui"') throw new Error('an existing GUI framework must not be replaced');
  let cargo = replaceField(original, 'workspace', 'members', JSON.stringify([...members, `${options.projectId}_gui`]));
  cargo = replaceField(cargo, metadata, 'interfaces', JSON.stringify([...interfaces, 'gui']));
  if (!framework) {
    const range = section(cargo, metadata);
    cargo = cargo.slice(0, range.end).trimEnd() + '\ngui-framework = "gpui"\n\n' + cargo.slice(range.end);
  }
  for (const [key, value] of [
    [`${options.projectId}_core`, `{ path = "${options.projectId}_core" }`],
    ['gpui-kit', '"0.7.1"'], ['rust-i18n', '"4.2.0"'], ['sys-locale', '"0.3.2"'],
    ['tracing', '{ version = "0.1.44", default-features = false, features = ["std"] }'],
    ['tracing-subscriber', '{ version = "0.3.23", default-features = false, features = ["fmt", "registry", "std"] }'],
    ['tracing-appender', '{ version = "0.2.5", default-features = false }'],
    ...nativeDependencies(options),
  ]) cargo = dependency(cargo, key, value);
  const files = renderGpuiAdapterFiles(options);
  const originalPackage = packageSnapshot(target);
  files.set('package.json', renderGpuiPackageJson(originalPackage));
  for (const relative of files.keys()) {
    const file = path.join(target, relative);
    assertRealPath(file, true);
    if (fs.existsSync(file) && !(relative === 'package.json' && originalPackage !== null)) throw new Error(`existing adapter/profile file must not be overwritten: ${relative}`);
  }
  const stage = fs.mkdtempSync(path.join(target, '.gpui-adapter-add-'));
  const installed = [];
  const createdDirectories = [];
  let packageReplaced = false;
  try {
    for (const [relative, value] of files) {
      const staged = path.join(stage, relative);
      fs.mkdirSync(path.dirname(staged), { recursive: true });
      fs.writeFileSync(staged, value, { flag: 'wx' });
    }
    fs.writeFileSync(path.join(stage, 'Cargo.toml'), cargo, { flag: 'wx' });
    if (originalPackage !== null) {
      fs.writeFileSync(path.join(stage, 'package-original.json'), originalPackage, { flag: 'wx' });
      fs.chmodSync(path.join(stage, 'package.json'), fs.lstatSync(path.join(target, 'package.json')).mode & 0o777);
    }
    if (fs.readFileSync(cargoPath, 'utf8') !== original || fs.readFileSync(corePath, 'utf8') !== core) throw new Error('workspace changed during generation');
    assertPackageSnapshot(target, originalPackage);
    for (const relative of files.keys()) {
      if (relative === 'package.json' && originalPackage !== null) continue;
      const file = path.join(target, relative);
      assertRealPath(file, true);
      if (fs.existsSync(file)) throw new Error(`target changed during generation: ${relative}`);
      let directory = path.dirname(file);
      const missing = [];
      while (!fs.existsSync(directory)) { missing.push(directory); directory = path.dirname(directory); }
      for (const missingDirectory of missing.reverse()) { fs.mkdirSync(missingDirectory); createdDirectories.push(missingDirectory); }
      fs.linkSync(path.join(stage, relative), file);
      installed.push(file);
    }
    assertRealPath(cargoPath);
    if (fs.readFileSync(cargoPath, 'utf8') !== original) throw new Error('workspace changed before Cargo merge');
    if (originalPackage !== null) {
      assertPackageSnapshot(target, originalPackage);
      if (!originalPackage.equals(Buffer.from(files.get('package.json')))) {
        fs.renameSync(path.join(stage, 'package.json'), path.join(target, 'package.json'));
        packageReplaced = true;
      }
    }
    fs.renameSync(path.join(stage, 'Cargo.toml'), cargoPath);
  } catch (error) {
    if (packageReplaced) {
      assertRealPath(path.join(target, 'package.json'));
      if (!fs.readFileSync(path.join(target, 'package.json')).equals(Buffer.from(files.get('package.json')))) throw new Error('package.json changed before rollback; preserve concurrent user changes', { cause: error });
      fs.renameSync(path.join(stage, 'package-original.json'), path.join(target, 'package.json'));
    }
    for (const file of installed.reverse()) fs.unlinkSync(file);
    for (const directory of createdDirectories.reverse()) { if (fs.readdirSync(directory).length === 0) fs.rmdirSync(directory); }
    throw error;
  } finally { fs.rmSync(stage, { recursive: true, force: true }); }
  return { target, guiFramework: 'gpui', addedFiles: [...files.keys()], corePreserved: true };
}

/** CLI 的字段与初始化 wrapper 保持一致，但 target 必须已经拥有 core。 */
function argumentsFor(argv) {
  const known = ['target', 'project-id', 'name-zh', 'name-en', 'owner', 'about-page', 'sponsor-page', 'sidebar-mode', 'logo', 'target-platforms', 'system-tray', 'system-notification', 'autostart', 'single-instance', 'deep-link', 'global-shortcut'];
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const name = argv[index]?.slice(2);
    if (!argv[index]?.startsWith('--') || !known.includes(name) || argv[index + 1] === undefined || argv[index + 1].startsWith('--')) throw new Error(`unknown or incomplete argument: ${argv[index]}`);
    const key = name.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    if (Object.hasOwn(options, key)) throw new Error(`duplicate argument: ${name}`);
    options[key] = key === 'targetPlatforms' ? argv[index + 1].split(',') : argv[index + 1];
  }
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.stdout.write(JSON.stringify(addGpuiAdapter(argumentsFor(process.argv.slice(2))), null, 2) + '\n'); }
  catch (error) { process.stderr.write(`GPUI adapter: ${error.message}\n`); process.exitCode = 1; }
}
