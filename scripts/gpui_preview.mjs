#!/usr/bin/env node
/** 只准备 macOS 开发预览；本地 ad-hoc 身份不构成发布、分发或运行时验收。 */
import fs from 'node:fs';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual, parseArgs } from 'node:util';
import { assertNodeRuntime, commandRunner, gitIdentity } from '../.agents/skills/desktop-build-gpui-release/scripts/build_gpui_release.mjs';
import { readConfig, projectFacts, TARGETS } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_config.mjs';
import { safeRoot, safePath, sha256 } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_filesystem.mjs';
import { collectResources, assertResourcesStable, stageResources } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_resources.mjs';
import { verifyResources } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_packager.mjs';
import { assertDependencyLocks, readDependencyLockPolicy } from '../.agents/skills/desktop-implement-change/scripts/project_lock_policy.mjs';

const PREVIEW_ROOT = 'target/gpui-preview';
const PREVIEW_APP = `${PREVIEW_ROOT}/LokiMetris.app`;

/** 所有进程都经 argv 执行，替身的非零结果同样不能产生成功证据。 */
function checkedRunner(root, supplied) {
  const run = supplied ?? commandRunner(root);
  return (program, args, options = {}) => {
    const result = run(program, args, options);
    if (!result || result.error || result.status !== 0) throw new Error(`${path.basename(program)} failed during development preview`);
    return result;
  };
}

/** comm 保留完整可执行路径中的空格，参数提及应用不算该应用正在运行。 */
export function assertPreviewStopped(run, executables) {
  const result = run('/bin/ps', ['-Aww', '-o', 'pid=,comm='], { quiet: true, timeout: 10_000 });
  const expected = new Set(executables);
  for (const line of result.stdout.split(/\r?\n/u).filter(value => value.trim())) {
    const match = /^\s*(0|[1-9]\d*)\s+(.+?)\s*$/u.exec(line);
    if (!match) throw new Error('cannot safely parse macOS process identities');
    if (expected.has(match[2])) throw new Error(`development preview is running (pid ${match[1]}); close it before preparing another bundle`);
  }
}

/** 递归拒绝链接和特殊文件；签名暂存文件还必须是独立 inode。 */
function assertOrdinaryTree(root, relative, independent = false) {
  const full = safePath(root, relative), stat = fs.lstatSync(full);
  if (stat.isDirectory()) {
    for (const child of fs.readdirSync(full)) assertOrdinaryTree(root, `${relative}/${child}`, independent);
  } else if (!stat.isFile() || (independent && stat.nlink !== 1)) {
    throw new Error(`preview tree requires ordinary independent files: ${relative}`);
  }
}

/** Git 源码集合也拒绝已跟踪的链接，不依赖 diff 恰好覆盖这些路径。 */
function sourceIdentity(root, run) {
  for (const relative of run('git', ['ls-files', '-z']).stdout.split('\0').filter(Boolean)) safePath(root, relative, { type: 'file', missing: true });
  return gitIdentity(root, run);
}

/** 所有边界复核配置、源码与资源，构建过程中漂移即保留旧预览。 */
function assertInputsStable(root, run, config, identity, resources, iconHash) {
  if (!isDeepStrictEqual(readConfig(root), config)) throw new Error('preview configuration changed during build');
  if (!isDeepStrictEqual(sourceIdentity(root, run), identity)) throw new Error('preview source bytes changed during build');
  assertResourcesStable(root, config, 'macos', false, resources);
  if (sha256(safePath(root, config.icons.macos, { type: 'file' })) !== iconHash) throw new Error('preview icon bytes changed during build');
}

/** 使用同一显式锁策略读取 metadata 和构建，不升级到 release profile。 */
function cargoArguments(base, policy) {
  return [...base, ...(policy === 'tracked' ? ['--locked'] : [])];
}

/** 保留 Cargo 缓存目录配置，只清除发布注入、隐式签名和环境目标架构。 */
function previewEnvironment(config, environment) {
  const env = { ...environment, MACOSX_DEPLOYMENT_TARGET: config.macos.minimumSystemVersion };
  for (const key of Object.keys(env)) {
    if (key.startsWith('APPLE_') || key.startsWith('CARGO_PACKAGER_') || key === 'HARNESS_GPUI_RELEASE_NOTES_RS' || key === 'CARGO_BUILD_TARGET') delete env[key];
  }
  return env;
}

/** XML 文本只转义值；身份与版本完全取经过验证的配置和 Cargo metadata。 */
function xml(value) {
  return String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;');
}

/** 构造可由 LaunchServices 与 UserNotifications 识别的完整应用身份。 */
export function renderPreviewPlist(config, facts) {
  const values = {
    CFBundleDevelopmentRegion: 'en', CFBundleDisplayName: config.productName,
    CFBundleExecutable: config.binary, CFBundleIdentifier: config.identifier,
    CFBundleInfoDictionaryVersion: '6.0', CFBundleName: config.productNameEn,
    CFBundlePackageType: 'APPL', CFBundleShortVersionString: facts.version,
    CFBundleVersion: facts.version, CFBundleIconFile: path.posix.basename(config.icons.macos),
    LSMinimumSystemVersion: config.macos.minimumSystemVersion, NSPrincipalClass: 'NSApplication',
  };
  const fields = Object.entries(values).map(([key, value]) => `  <key>${key}</key><string>${xml(value)}</string>`);
  return `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict>\n${fields.join('\n')}\n  <key>LSUIElement</key><${config.macos.agent ? 'true' : 'false'}/>\n  <key>NSHighResolutionCapable</key><true/>\n  <key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>\n</dict></plist>\n`;
}

/** Cargo 的绝对目标目录可在项目外，但所有现存组件必须是真实普通目录。 */
function cargoTargetDirectory(value) {
  if (typeof value !== 'string' || !path.isAbsolute(value) || path.resolve(value) !== value) throw new Error('Cargo target_directory must be an absolute ordinary path');
  let ancestor = value;
  while (true) {
    try { fs.lstatSync(ancestor); break; } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      ancestor = path.dirname(ancestor);
    }
  }
  safeRoot(ancestor);
  const relative = path.relative(ancestor, value).split(path.sep).join('/');
  return relative ? safePath(ancestor, relative, { missing: true, type: 'directory' }) : ancestor;
}

/** 只接受 metadata 指定缓存中默认 host debug 下的唯一当前包产物。 */
function cargoExecutable(output, config, metadata, targetDirectory) {
  const member = metadata.packages.find(item => item.name === config.package && metadata.workspace_members.includes(item.id));
  const artifacts = output.split(/\r?\n/u).filter(Boolean).flatMap(line => {
    let event;
    try { event = JSON.parse(line); } catch { return []; }
    return event.reason === 'compiler-artifact' && event.package_id === member.id
      && event.target?.name === config.binary && event.target.kind?.includes('bin')
      && event.profile?.test === false && event.executable ? [event.executable] : [];
  });
  if (artifacts.length !== 1 || !path.isAbsolute(artifacts[0])) throw new Error('Cargo did not report exactly one development GUI executable');
  const expected = path.join(targetDirectory, 'debug', config.binary);
  if (artifacts[0] !== expected) throw new Error('Cargo preview executable differs from metadata target_directory/debug/binary; explicit build.target is unsupported by this host-cache preview');
  safeRoot(targetDirectory);
  return safePath(targetDirectory, `debug/${config.binary}`, { type: 'file' });
}

/** 旧目录只能由本次捕获的 inode 重命名，不能接管并发出现的陌生包。 */
function assertPreviousUnchanged(root, previous) {
  const full = safePath(root, PREVIEW_APP, { missing: true });
  const current = fs.existsSync(full) ? fs.lstatSync(full) : null;
  if (previous ? !current || current.dev !== previous.dev || current.ino !== previous.ino : current) throw new Error('preview destination changed during preparation');
  if (current) assertOrdinaryTree(root, PREVIEW_APP);
}

/** 完整 stage 先签名验证；最后才备份旧包并发布，失败不覆盖旧字节。 */
export function preparePreview(options, hooks = {}) {
  if ((hooks.platform ?? process.platform) !== 'darwin') throw new Error('GPUI development preview requires macOS');
  const root = safeRoot(options.root), run = checkedRunner(root, hooks.run);
  const config = readConfig(root), identity = sourceIdentity(root, run);
  const policy = readDependencyLockPolicy(root);
  const resources = collectResources(root, config, 'macos', false);
  const icon = safePath(root, config.icons.macos, { type: 'file' }), iconHash = sha256(icon);
  const destination = safePath(root, PREVIEW_APP, { missing: true, type: 'directory' });
  const previous = fs.existsSync(destination) ? fs.lstatSync(destination) : null;
  if (previous) assertOrdinaryTree(root, PREVIEW_APP);
  const executable = path.join(destination, 'Contents', 'MacOS', config.binary);
  assertPreviewStopped(run, [executable]);
  run('git', ['check-ignore', '-q', '--no-index', '--', `${PREVIEW_ROOT}/.development-preview`], { quiet: true });
  const parent = safePath(root, PREVIEW_ROOT, { missing: true });
  fs.mkdirSync(parent, { recursive: true });
  const lock = safePath(root, `${PREVIEW_ROOT}/.prepare-lock`, { missing: true });
  try { fs.mkdirSync(lock); } catch (error) {
    if (error.code === 'EEXIST') throw new Error('another preview preparation owns the lock; refusing concurrent replacement');
    throw error;
  }
  const lockIdentity = fs.lstatSync(lock), buildRun = randomUUID();
  let work = null, backup = null, movedPrevious = false;
  const cleanupWarnings = [];
  try {
    const env = previewEnvironment(config, hooks.env ?? process.env);
    const metadata = JSON.parse(run('cargo', cargoArguments(['metadata', '--no-deps', '--format-version', '1'], policy), { env }).stdout);
    const facts = projectFacts(root, metadata, config);
    if (!facts.platforms.includes('macos') || facts.lockPolicy !== policy) throw new Error('Cargo metadata disagrees with native preview configuration');
    const targetDirectory = cargoTargetDirectory(metadata.target_directory);
    assertDependencyLocks(root, { rustTestManifests: facts.manifests });
    const target = run('rustc', ['-vV'], { env }).stdout.match(/^host: (.+)$/mu)?.[1];
    if (TARGETS[target]?.host !== 'darwin') throw new Error('development preview requires a native macOS Rust host');
    const build = run('cargo', cargoArguments(['build', '--package', config.package, '--bin', config.binary, '--message-format', 'json-render-diagnostics'], policy), { env });
    const binary = cargoExecutable(build.stdout, config, metadata, targetDirectory), binaryHash = sha256(binary);
    assertInputsStable(root, run, config, identity, resources, iconHash);
    const workRelative = `${PREVIEW_ROOT}/.prepare-${buildRun}`;
    work = safePath(root, workRelative, { missing: true });
    fs.mkdirSync(work);
    const appRelative = `${workRelative}/LokiMetris.app`, app = safePath(root, appRelative, { missing: true });
    fs.mkdirSync(path.join(app, 'Contents', 'MacOS'), { recursive: true });
    const bundledBinary = path.join(app, 'Contents', 'MacOS', config.binary);
    fs.copyFileSync(binary, bundledBinary, fs.constants.COPYFILE_EXCL);
    fs.chmodSync(bundledBinary, fs.statSync(binary).mode & 0o777);
    if (sha256(binary) !== binaryHash || sha256(bundledBinary) !== binaryHash) throw new Error('preview executable bytes changed during copy');
    fs.writeFileSync(path.join(app, 'Contents', 'Info.plist'), renderPreviewPlist(config, facts), { flag: 'wx' });
    const resourceRoot = path.join(app, 'Contents', 'Resources');
    stageResources(root, resourceRoot, resources);
    const bundledIcon = path.join(resourceRoot, path.posix.basename(config.icons.macos));
    fs.copyFileSync(icon, bundledIcon, fs.constants.COPYFILE_EXCL);
    if (sha256(bundledIcon) !== iconHash) throw new Error('preview icon bytes changed during copy');
    assertOrdinaryTree(root, appRelative, true);
    run('/usr/bin/plutil', ['-lint', path.join(app, 'Contents', 'Info.plist')], { quiet: true, timeout: 10_000 });
    run('/usr/bin/codesign', ['--force', '--sign', '-', app], { env, quiet: true, timeout: 60_000 });
    run('/usr/bin/codesign', ['--verify', '--strict', '--verbose=4', app], { env, quiet: true, timeout: 60_000 });
    const signature = run('/usr/bin/codesign', ['-dv', '--verbose=4', app], { env, quiet: true, timeout: 10_000 });
    const details = `${signature.stdout}\n${signature.stderr}`;
    if (details.match(/^Identifier=(.+)$/mu)?.[1] !== config.identifier || !/^Signature=adhoc$/mu.test(details)
        || /^Info\.plist=not bound$/mu.test(details) || /^Sealed Resources=none$/mu.test(details)) throw new Error('preview bundle lacks the expected bound ad-hoc identity');
    assertOrdinaryTree(root, appRelative, true);
    verifyResources(resourceRoot, root, resources);
    if (sha256(bundledIcon) !== iconHash || sha256(binary) !== binaryHash) throw new Error('preview signing changed source executable or icon bytes');
    assertInputsStable(root, run, config, identity, resources, iconHash);
    const result = {
      distribution: 'development-only', releaseCandidate: false, buildRun, project: facts.project,
      version: facts.version, target, bundleIdentifier: config.identifier, bundle: destination,
      sourceCommit: identity.commit, sourceDirty: identity.dirty, sourceSnapshotSha256: identity.sourceSha256,
      cargoTargetDirectory: targetDirectory, cargoExecutableSha256: binaryHash, bundledExecutableSha256: sha256(bundledBinary),
      resourceVerification: 'byte-identical', bundledResources: resources.map(({ target, sha256 }) => ({ target, sha256 })),
      signing: { kind: 'local-adhoc', systemVerified: !hooks.run, verification: hooks.run ? 'command-substitute-only' : 'strict-bundle-verified' },
      notarization: 'Not performed', runtimeVerification: 'Unverified', backup: null, cleanupWarnings,
    };
    assertPreviousUnchanged(root, previous);
    assertPreviewStopped(run, [executable]);
    if (previous) {
      const backupRelative = `${PREVIEW_ROOT}/previous-${buildRun}.app`;
      backup = safePath(root, backupRelative, { missing: true });
      if (fs.existsSync(backup)) throw new Error('preview backup destination already exists');
      fs.renameSync(destination, backup);
      movedPrevious = true;
      result.backup = backup;
    }
    assertPreviewStopped(run, [executable, ...(backup ? [path.join(backup, 'Contents', 'MacOS', config.binary)] : [])]);
    safePath(root, PREVIEW_APP, { missing: true });
    if (fs.existsSync(destination)) throw new Error('preview destination appeared before publication; refusing overwrite');
    fs.renameSync(app, destination);
    movedPrevious = false;
    try { fs.rmdirSync(work); } catch { cleanupWarnings.push('empty preparation directory retained'); }
    return result;
  } catch (error) {
    if (movedPrevious) {
      try {
        safePath(root, PREVIEW_APP, { missing: true });
        if (fs.existsSync(destination)) throw new Error('destination is occupied');
        assertOrdinaryTree(root, path.relative(root, backup).split(path.sep).join('/'));
        fs.renameSync(backup, destination);
      } catch { throw new Error(`${error.message}; previous preview retained at ${backup}; destination was not overwritten`, { cause: error }); }
    }
    throw error;
  } finally {
    try {
      const current = fs.lstatSync(lock);
      if (current.isSymbolicLink() || current.ino !== lockIdentity.ino || current.dev !== lockIdentity.dev) throw new Error('lock ownership changed');
      fs.rmdirSync(lock);
    } catch { cleanupWarnings.push('preparation lock retained; inspect before another run'); }
  }
}

/** CLI 只有真实开发预览入口，不暴露命令替身、签名身份或启动应用开关。 */
export function main(argv = process.argv.slice(2)) {
  const { values, positionals } = parseArgs({ args: argv, allowPositionals: true, options: { root: { type: 'string', default: '.' }, help: { type: 'boolean', short: 'h' } } });
  if (values.help) { process.stdout.write('Usage: pnpm gpui:preview [--root <project>]\nPrepares a macOS development-only ad-hoc app; does not launch or publish it.\n'); return 0; }
  if (positionals.length) throw new Error('preview accepts no positional commands');
  process.stdout.write(JSON.stringify(preparePreview({ root: values.root }), null, 2) + '\n');
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { assertNodeRuntime(); process.exitCode = main(); }
  catch (error) { process.stderr.write(`GPUI preview: ${error.message}\n`); process.exitCode = 1; }
}
