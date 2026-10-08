/** GPUI 独立打包配置；版本、包与接口事实来自 Cargo metadata。 */
import fs from 'node:fs';
import path from 'node:path';
import { safePath, relativePath } from './gpui_filesystem.mjs';

export const PACKAGER_VERSION = '0.11.8';
export const TARGETS = {
  'aarch64-apple-darwin': { platform: 'macos', host: 'darwin', arch: 'aarch64', formats: ['app', 'dmg'] },
  'x86_64-apple-darwin': { platform: 'macos', host: 'darwin', arch: 'x86_64', formats: ['app', 'dmg'] },
  'x86_64-pc-windows-msvc': { platform: 'windows', host: 'win32', arch: 'x86_64', formats: ['nsis'] },
};

/** 初始化器只写中性身份，不把产品用途或发布选择写入配置。 */
export function renderDefaultConfig({ projectId, nameZh, nameEn, owner, logoPath = `${projectId}_gui/assets/logo.png` }) {
  const identifier = `com.${projectId.replaceAll('_', '-')}.app`;
  return {
    schemaVersion: 1, package: `${projectId}_gui`, binary: `${projectId}_gui`,
    productName: nameZh, productNameEn: nameEn, identifier, publisher: owner,
    icons: { source: logoPath, macos: 'packaging/icons/app.icns', windows: 'packaging/icons/app.ico' },
    macos: { minimumSystemVersion: '12.0', signingIdentity: null, background: 'packaging/macos/background.png' },
    windows: { signing: null, extractor: null },
    nsis: { installMode: 'currentUser', languages: ['SimpChinese', 'English'], displayLanguageSelector: true },
  };
}

/** 所有用户可见文字必须可安全进入文件名、plist 和 NSIS 字符串。 */
function text(value, label, filename = false) {
  if (typeof value !== 'string' || !value || value !== value.trim() || !value.isWellFormed() || value.length > 200
      || /[\x00-\x1f\x7f]/u.test(value) || value.includes('${') || (filename && /[\\/:*?<>|]/u.test(value))) throw new Error(`invalid ${label}`);
  return value;
}

/** 固定键集合使字段遗漏与额外 hook 配置都失败关闭。 */
function keys(object, expected, label) {
  if (!object || typeof object !== 'object' || Array.isArray(object) || Object.keys(object).sort().join(',') !== [...expected].sort().join(',')) throw new Error(`invalid ${label} fields`);
}

/** 单一严格 schema 防止残留 Tauri 配置、hook 或未知签名字段进入工具。 */
export function validateConfig(value) {
  keys(value, ['schemaVersion','package','binary','productName','productNameEn','identifier','publisher','icons','macos','windows','nsis'], 'GPUI config');
  if (value.schemaVersion !== 1) throw new Error('unsupported GPUI config schema');
  for (const key of ['package','binary']) if (!/^[A-Za-z][A-Za-z0-9_-]*$/u.test(value[key])) throw new Error(`invalid ${key}`);
  for (const key of ['productName','productNameEn','publisher']) text(value[key], key, key.startsWith('product'));
  if (!/^[a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)+$/u.test(value.identifier)) throw new Error('invalid application identifier');
  keys(value.icons, ['source','macos','windows'], 'icons');
  for (const icon of Object.values(value.icons)) relativePath(icon);
  if (!value.icons.source.endsWith('.png') || !value.icons.macos.endsWith('.icns') || !value.icons.windows.endsWith('.ico')) throw new Error('icons require PNG/ICNS/ICO paths');
  keys(value.macos, ['minimumSystemVersion','signingIdentity','background'], 'macos');
  if (!/^\d+\.\d+(?:\.\d+)?$/u.test(value.macos.minimumSystemVersion) || Number(value.macos.minimumSystemVersion.split('.')[0]) < 11) throw new Error('GPUI macOS minimum must be >= 11.0');
  relativePath(value.macos.background);
  if (value.macos.signingIdentity !== null && !text(value.macos.signingIdentity,'signingIdentity').startsWith('Developer ID Application: ')) throw new Error('macOS candidate signing needs Developer ID Application');
  keys(value.windows, ['signing','extractor'], 'windows');
  if (value.windows.extractor !== null) relativePath(value.windows.extractor);
  if (value.windows.signing !== null) {
    keys(value.windows.signing, ['certificateThumbprint','timestampUrl'], 'windows.signing');
    if (!/^[A-Fa-f0-9]{40}$/u.test(value.windows.signing.certificateThumbprint) || !/^https:\/\/[^\s]+$/u.test(value.windows.signing.timestampUrl)) throw new Error('invalid Authenticode configuration');
  }
  keys(value.nsis, ['installMode','languages','displayLanguageSelector'], 'nsis');
  if (!['currentUser','perMachine','both'].includes(value.nsis.installMode) || typeof value.nsis.displayLanguageSelector !== 'boolean'
      || !Array.isArray(value.nsis.languages) || !value.nsis.languages.length || value.nsis.languages.some(item => !['SimpChinese','English'].includes(item)) || new Set(value.nsis.languages).size !== value.nsis.languages.length) throw new Error('invalid NSIS options');
  return value;
}

/** Cargo 自己解析 TOML；只接受根工作区内选定的 GPUI 包和二进制。 */
export function projectFacts(root, metadata, config) {
  const policy = metadata.metadata?.['agent-first-harness'];
  if (!policy || !Array.isArray(policy.interfaces) || !policy.interfaces.includes('gui') || policy['gui-framework'] !== 'gpui') throw new Error('requires a selected GPUI GUI workspace');
  if (path.resolve(metadata.workspace_root) !== root) throw new Error('Cargo workspace must equal project root');
  if (!Array.isArray(policy['target-platforms']) || !policy['target-platforms'].length || policy['target-platforms'].some(value => !['macos','windows','linux'].includes(value))) throw new Error('invalid target-platforms');
  if (!['ignored','tracked'].includes(policy['dependency-lock-policy'] ?? 'ignored')) throw new Error('invalid dependency-lock-policy');
  if (policy['project-id'] !== undefined && (typeof policy['project-id'] !== 'string' || policy['project-id'].length > 64 || !/^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$/u.test(policy['project-id']))) throw new Error('invalid project-id');
  const packages = metadata.packages.filter(item => item.name === config.package && metadata.workspace_members.includes(item.id));
  if (packages.length !== 1) throw new Error('packaging package must resolve to one workspace member');
  const pkg = packages[0];
  const member = path.relative(root, pkg.manifest_path).split(path.sep).join('/');
  safePath(root, member, { type: 'file' });
  if (!pkg.targets.some(item => item.name === config.binary && item.kind.includes('bin'))) throw new Error('selected GUI binary is absent');
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/u.exec(pkg.version);
  if (!match || BigInt(match[1]) > 18446744073709551615n || Number(match[2]) > 99 || Number(match[3]) > 99) throw new Error('invalid base-100 Cargo version');
  const guiRoot = policy['gui-root'] ?? path.posix.dirname(member);
  if (guiRoot !== '.') safePath(root, guiRoot, { type: 'directory' });
  if (path.resolve(root, guiRoot) !== path.dirname(pkg.manifest_path)) throw new Error('gui-root disagrees with selected package');
  const manifests = policy['rust-test-manifests'] ?? ['Cargo.toml'];
  if (!Array.isArray(manifests) || !manifests.length) throw new Error('rust-test-manifests must not be empty');
  for (const manifest of manifests) safePath(root, manifest, { type: 'file' });
  return { project: policy['project-id'] ?? config.package, version: pkg.version, guiRoot, manifests, lockPolicy: policy['dependency-lock-policy'] ?? 'ignored', platforms: policy['target-platforms'] };
}

/** 配置读取不做修正，缺失资产在选定平台构建时阻断。 */
export function readConfig(root) {
  const file = safePath(root, 'packaging/gpui.json', { type: 'file' });
  if (fs.statSync(file).size > 64 * 1024) throw new Error('GPUI packaging config exceeds 64 KiB');
  const source = new TextDecoder('utf-8',{ fatal: true }).decode(fs.readFileSync(file));
  const value = JSON.parse(source);
  const frames = [];
  for (const match of source.matchAll(/"(?:\\.|[^"\\])*"|[{}\[\],:]|true|false|null|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/gu)) {
    const token = match[0], current = frames.at(-1);
    if (token === '{') frames.push({ object: true, key: true, keys: new Set() });
    else if (token === '[') frames.push({ object: false });
    else if (token === '}' || token === ']') frames.pop();
    else if (token === ',' && current?.object) current.key = true;
    else if (token === ':' && current?.object) current.key = false;
    else if (token.startsWith('"') && current?.object && current.key) {
      const key = JSON.parse(token);
      if (current.keys.has(key)) throw new Error(`duplicate GPUI config field: ${key}`);
      current.keys.add(key);
    }
  }
  return validateConfig(value);
}
