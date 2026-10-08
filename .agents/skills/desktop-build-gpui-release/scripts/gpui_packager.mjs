/** cargo-packager 0.11.8 的原生配置、签名和最终资源核对。 */
import fs from 'node:fs';
import path from 'node:path';
import { safePath, sha256 } from './gpui_filesystem.mjs';
import { TARGETS } from './gpui_config.mjs';

/** 生成工具配置而不运行用户 hook，不继承未批准的自动签名配置。 */
export function packagerConfig({ root, work, config, facts, target, format, resources, signing }) {
  const result = {
    name: config.package, productName: config.productName, version: facts.version,
    identifier: config.identifier, publisher: config.publisher,
    binaries: [{ path: config.binary, main: true }], binariesDir: path.join(work, 'binaries'),
    outDir: path.join(work, 'packages'), targetTriple: target,
    formats: format === 'dmg' ? ['app', 'dmg'] : [format],
    icons: [safePath(root, config.icons[format === 'nsis' ? 'windows' : 'macos'], { type: 'file' })],
    resources: resources.map(file => ({ src: path.join(work, 'resources', file), target: file })),
  };
  if (format === 'nsis') {
    result.windows = { allowDowngrades: false };
    result.nsis = { ...config.nsis, installerIcon: result.icons[0] };
  } else {
    result.macos = { minimumSystemVersion: config.macos.minimumSystemVersion };
    if (signing.selection === 'enabled') result.macos.signingIdentity = config.macos.signingIdentity;
    if (format === 'dmg') result.dmg = {
      background: safePath(root, config.macos.background, { type: 'file' }),
      windowSize: { width: 660, height: 440 }, appPosition: { x: 180, y: 220 }, appFolderPosition: { x: 480, y: 220 },
    };
  }
  return result;
}

/** 固定版本产物命名；不扫描并误收其他旧版本。 */
export function packagePath(work, config, facts, target, format) {
  const arch = TARGETS[target].arch === 'x86_64' ? 'x64' : 'aarch64';
  const name = format === 'nsis' ? `${config.binary}_${facts.version}_${arch}-setup.exe`
    : format === 'dmg' ? `${config.productName}_${facts.version}_${arch}.dmg` : `${config.productName}.app`;
  return safePath(work, `packages/${name}`, { type: format === 'app' ? 'directory' : 'file' });
}

/** 签名选择由授权来源决定，环境变量存在不会反向开启签名。 */
export function signingChoice(selection, source = 'not-requested') {
  if (!['enabled','disabled'].includes(selection) || !['not-requested','configured','requested','channel-required'].includes(source)
      || (selection === 'disabled') !== (source === 'not-requested')) throw new Error('invalid signing selection/source');
  return { selection, source };
}

/** macOS 签名后只有完整公证、stapling 和验证成功才产生候选。 */
export function finalizeSigning({ run, config, signing, format, artifact, app, env }) {
  if (signing.selection === 'disabled') return { signingStatus: 'unsigned', notarizationStatus: format === 'nsis' ? 'not-applicable' : 'not-run', signingEvidence: { ...signing, status: 'unsigned', remainingRisk: 'Operating-system trust and distribution requirements remain unverified.' } };
  if (format === 'nsis') {
    const settings = config.windows.signing;
    if (!settings) throw new Error('enabled Windows signing requires approved Authenticode configuration');
    run('signtool', ['sign','/sha1',settings.certificateThumbprint,'/fd','SHA256','/tr',settings.timestampUrl,'/td','SHA256',artifact], { env, quiet: true });
    run('signtool', ['verify','/pa','/all',artifact], { env, quiet: true });
    return { signingStatus: 'signed', notarizationStatus: 'not-applicable', signingEvidence: { ...signing, status: 'verified-authenticode' } };
  }
  throw new Error('GPUI macOS signing/notarization is unavailable; enabled selection fails closed');
}

/** 包内资源必须保持源字节，候选每份许可证与更新日志都核对。 */
export function verifyResources(resourceRoot, sourceRoot, files) {
  for (const file of files) {
    const bundled = safePath(resourceRoot, file, { type: 'file' });
    const source = safePath(sourceRoot, file, { type: 'file' });
    if (sha256(bundled) !== sha256(source)) throw new Error(`bundled resource bytes differ: ${file}`);
  }
  return 'byte-identical';
}

/** 读取最终应用 plist，核对显示壳层以外的稳定安装身份与版本。 */
export function verifyMacIdentity(run, app, config, facts) {
  const plist = safePath(app,'Contents/Info.plist',{ type: 'file' });
  for (const [field,expected] of [['CFBundleIdentifier',config.identifier],['CFBundleExecutable',config.binary],['CFBundleShortVersionString',facts.version]]) {
    if (run('plutil',['-extract',field,'raw','-o','-',plist],{ quiet: true }).stdout.trim() !== expected) throw new Error(`macOS bundle identity differs: ${field}`);
  }
  return 'verified';
}

/** 最终 DMG 只读挂载；不接受 SLA、不修改布局或候选字节。 */
export function verifyDmg({ run, artifact, mount, root, config, facts, resources, binarySha256 }) {
  fs.mkdirSync(mount);
  let attached = false;
  try {
    run('hdiutil', ['attach','-readonly','-nobrowse','-noautoopen','-mountpoint',mount,artifact], { quiet: true });
    attached = true;
    const apps = fs.readdirSync(mount).filter(file => file.endsWith('.app'));
    if (apps.length !== 1) throw new Error('DMG must contain exactly one application bundle');
    const app = safePath(mount, apps[0], { type: 'directory' });
    verifyMacIdentity(run,app,config,facts);
    const store = safePath(mount, '.DS_Store', { type: 'file' });
    if (!fs.statSync(store).size) throw new Error('DMG Finder layout is missing');
    const background = safePath(mount, `.background/${path.basename(config.macos.background)}`, { type: 'file' });
    if (sha256(background) !== sha256(safePath(root, config.macos.background, { type: 'file' }))) throw new Error('DMG background differs');
    const applications = path.join(mount, 'Applications');
    if (!fs.lstatSync(applications).isSymbolicLink() || fs.readlinkSync(applications) !== '/Applications') throw new Error('DMG Applications drag target is invalid');
    verifyResources(path.join(app, 'Contents', 'Resources'), root, resources);
    if (sha256(safePath(app,`Contents/MacOS/${config.binary}`,{ type: 'file' })) !== binarySha256) throw new Error('DMG binary differs from this build');
    return 'final-volume-byte-identical';
  } finally { if (attached) run('hdiutil', ['detach',mount], { quiet: true }); }
}

/** NSIS 只读解包到本次暂存区；没有显式项目内解包器时不安装应用来取证。 */
export function verifyNsis({ run, root, config, artifact, destination, resources, binarySha256 }) {
  if (!config.windows.extractor) throw new Error('NSIS candidate requires an approved project-local 7z/7zr extractor; local packages remain unverified');
  const extractor = safePath(root, config.windows.extractor, { type: 'file' });
  fs.mkdirSync(destination);
  run(extractor, ['x','-y',`-o${destination}`,artifact], { quiet: true });
  const matches = [];
  function walk(directory) {
    for (const file of fs.readdirSync(directory)) {
      const full = safePath(destination, path.relative(destination, path.join(directory, file)).split(path.sep).join('/'));
      if (fs.lstatSync(full).isDirectory()) walk(full);
      else if (file === 'release-notes.json') matches.push(path.dirname(full));
    }
  }
  walk(destination);
  if (matches.length !== 1) throw new Error('NSIS must contain one release-notes resource');
  verifyResources(matches[0], root, resources);
  if (sha256(safePath(matches[0], `${config.binary}.exe`, { type: 'file' })) !== binarySha256) throw new Error('NSIS binary differs from this build');
  return 'final-installer-byte-identical';
}
