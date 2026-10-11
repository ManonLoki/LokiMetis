/** 开发预览的真实临时文件回归；命令替身不提供系统签名或 UI 验证证据。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { preparePreview, renderPreviewPlist, assertPreviewStopped } from './gpui_preview.mjs';
import { renderDefaultConfig } from '../.agents/skills/desktop-build-gpui-release/scripts/gpui_config.mjs';

/** 每个夹具拥有独立 Git 根和被忽略的旧预览，不访问真实 HOME 或工程产物。 */
function fixture(t, { withPrevious = true } = {}) {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'gpui preview-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const config = renderDefaultConfig({ projectId: 'sample', nameZh: '测试应用', nameEn: 'Sample App', owner: 'Fixture' });
  config.resources = [{ source: 'runtime/skins', macos: 'skins', windows: 'resources/skins' }];
  const files = {
    'Cargo.toml': '[workspace]\nmembers = ["sample_gui"]\n[workspace.metadata.agent-first-harness]\nproject-id = "sample"\ninterfaces = ["gui"]\ngui-framework = "gpui"\ntarget-platforms = ["macos"]\n',
    'sample_gui/Cargo.toml': '[package]\nname = "sample_gui"\nversion = "0.2.0"\nedition = "2024"\n',
    'sample_gui/src/main.rs': 'fn main() {}\n',
    'packaging/gpui.json': JSON.stringify(config),
    'packaging/icons/app.icns': Buffer.from('fixture-icon'),
    'LICENSE.zh-CN.md': '测试应用 商业许可证', 'LICENSE.en.md': 'Sample App Commercial License',
    '.gitignore': '/target/\n/runtime/\nCargo.lock\n',
    'runtime/skins/nested/theme.json': '{"name":"fixture"}',
    'runtime/skins/nested/preview.png': Buffer.from([0, 1, 127, 255]),
  };
  for (const [relative, bytes] of Object.entries(files)) {
    const file = path.join(root, relative);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, bytes);
  }
  for (const args of [['init', '-b', 'main'], ['config', 'user.name', 'Fixture'], ['config', 'user.email', 'fixture@example.invalid'], ['add', '.'], ['commit', '-qm', 'fixture']]) {
    const result = spawnSync('git', args, { cwd: root, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  }
  const app = path.join(root, 'target/gpui-preview/LokiMetris.app');
  const binary = path.join(app, 'Contents/MacOS/sample_gui');
  if (withPrevious) {
    fs.mkdirSync(path.dirname(binary), { recursive: true });
    fs.writeFileSync(binary, 'previous-preview', { mode: 0o755 });
  }
  const metadata = {
    workspace_root: root, target_directory: path.join(root, 'target'), workspace_members: ['sample-id'],
    metadata: { 'agent-first-harness': { 'project-id': 'sample', interfaces: ['gui'], 'gui-framework': 'gpui', 'target-platforms': ['macos'] } },
    packages: [{ id: 'sample-id', name: 'sample_gui', version: '0.2.0', manifest_path: path.join(root, 'sample_gui/Cargo.toml'), targets: [{ name: 'sample_gui', kind: ['bin'] }] }],
  };
  return { root, config, app, binary, metadata };
}

/** 替身只改夹具文件；cargo、plutil 和 codesign 均不运行真实程序。 */
function fakeCommands(f, overrides = {}) {
  const calls = [];
  let processChecks = 0;
  const run = (program, args, options = {}) => {
    calls.push({ program, args, options });
    const result = { status: 0, stdout: '', stderr: '' };
    if (program === 'git') return spawnSync('git', args, { cwd: f.root, encoding: 'utf8' });
    if (program === '/bin/ps') {
      processChecks += 1;
      if (overrides.beforeProcess) overrides.beforeProcess(processChecks);
      return { ...result, status: overrides.psFailure ? 1 : 0,
        stdout: overrides.runningAt === processChecks ? ` 123 ${f.binary}\n` : ' 1 /sbin/launchd\n' };
    }
    if (program === 'rustc') return { ...result, stdout: 'host: aarch64-apple-darwin\n' };
    if (program === 'cargo' && args[0] === 'metadata') return { ...result, stdout: JSON.stringify(f.metadata) };
    if (program === 'cargo' && args[0] === 'build') {
      const executable = overrides.artifactPath ?? path.join(f.metadata.target_directory, 'debug/sample_gui');
      fs.mkdirSync(path.dirname(executable), { recursive: true });
      if (overrides.artifactLink) fs.symlinkSync(path.join(f.root, 'LICENSE.en.md'), executable);
      else fs.writeFileSync(executable, 'compiled-preview', { mode: 0o755 });
      if (overrides.resourceDrift) fs.writeFileSync(path.join(f.root, 'runtime/skins/nested/preview.png'), 'changed-ignored-resource');
      if (overrides.sourceDrift) fs.appendFileSync(path.join(f.root, 'sample_gui/src/main.rs'), '// changed\n');
      const event = { reason: 'compiler-artifact', package_id: 'sample-id', target: { name: 'sample_gui', kind: ['bin'] }, profile: { test: false }, executable };
      return { ...result, stdout: JSON.stringify(event) + '\n' };
    }
    if (program === '/usr/bin/plutil') return result;
    if (program === '/usr/bin/codesign' && args[0] === '--force') {
      if (overrides.signFailure) return { ...result, status: 1 };
      fs.appendFileSync(path.join(args.at(-1), 'Contents/MacOS/sample_gui'), '-signed-copy');
      return result;
    }
    if (program === '/usr/bin/codesign' && args[0] === '--verify') {
      if (overrides.bundleDrift) fs.writeFileSync(path.join(args.at(-1), 'Contents/Resources/skins/nested/preview.png'), 'tampered-bundle');
      return { ...result, status: overrides.verifyFailure ? 1 : 0 };
    }
    if (program === '/usr/bin/codesign' && args[0] === '-dv') return {
      ...result, stderr: `Identifier=${overrides.wrongIdentity ? 'foreign.app' : f.config.identifier}\nSignature=adhoc\nInfo.plist entries=14\nSealed Resources version=2\n`,
    };
    throw new Error(`unexpected fixture command: ${program}`);
  };
  return { calls, run };
}

/** 唯一注入点只供单元测试；CLI 不接受替身或假平台选项。 */
function prepare(f, fake, platform = 'darwin', environment = {}) {
  return preparePreview({ root: f.root }, { run: fake.run, platform, env: { HOME: '/fixture-home-not-written', APPLE_SIGNING_IDENTITY: 'must-not-use', HARNESS_GPUI_RELEASE_NOTES_RS: '/must-not-use', ...environment } });
}

/** 成功路径保留旧包，并核对完整资源、真实来源字节和替身证据边界。 */
test('prepares_development_bundle_and_preserves_previous_preview', t => {
  const f = fixture(t), fake = fakeCommands(f), result = prepare(f, fake);
  assert.equal(result.distribution, 'development-only');
  assert.equal(result.releaseCandidate, false);
  assert.deepEqual(result.signing, { kind: 'local-adhoc', systemVerified: false, verification: 'command-substitute-only' });
  assert.equal(result.runtimeVerification, 'Unverified');
  assert.equal(fs.readFileSync(path.join(result.backup, 'Contents/MacOS/sample_gui'), 'utf8'), 'previous-preview');
  assert.equal(fs.readFileSync(f.binary, 'utf8'), 'compiled-preview-signed-copy');
  assert.equal(fs.readFileSync(path.join(f.metadata.target_directory, 'debug/sample_gui'), 'utf8'), 'compiled-preview');
  assert.deepEqual(fs.readFileSync(path.join(f.app, 'Contents/Resources/skins/nested/preview.png')), Buffer.from([0, 1, 127, 255]));
  assert.equal(fs.readFileSync(path.join(f.app, 'Contents/Resources/LICENSE.en.md'), 'utf8'), 'Sample App Commercial License');
  assert.equal(fs.readFileSync(path.join(f.app, 'Contents/Resources/app.icns'), 'utf8'), 'fixture-icon');
  const plist = fs.readFileSync(path.join(f.app, 'Contents/Info.plist'), 'utf8');
  for (const expected of ['com.sample.app', 'sample_gui', 'Sample App', '测试应用', '0.2.0', '12.0', 'app.icns', 'NSHighResolutionCapable']) assert.ok(plist.includes(expected));
  const build = fake.calls.find(call => call.program === 'cargo' && call.args[0] === 'build');
  assert.equal(build.args.includes('--release'), false);
  assert.equal(build.args.includes('--target'), false);
  assert.equal(build.options.env.CARGO_TARGET_DIR, undefined);
  assert.ok(build.args.includes('json-render-diagnostics'));
  assert.equal(build.options.env.HOME, '/fixture-home-not-written');
  assert.equal(build.options.env.APPLE_SIGNING_IDENTITY, undefined);
  assert.equal(build.options.env.HARNESS_GPUI_RELEASE_NOTES_RS, undefined);
  const sign = fake.calls.find(call => call.program === '/usr/bin/codesign' && call.args[0] === '--force');
  assert.deepEqual(sign.args.slice(0, 3), ['--force', '--sign', '-']);
  assert.ok(fake.calls.some(call => call.program === '/usr/bin/codesign' && call.args.includes('--strict')));
  assert.equal(fs.existsSync(path.join(f.root, 'target/gpui-preview/.prepare-lock')), false);
  assert.equal(fs.existsSync(path.join(f.root, 'release')), false);
});

/** metadata 的外部缓存仍为唯一构建产物来源，预览和备份始终留在项目 target。 */
test('reuses_external_cargo_target_directory_without_forcing_a_target_triple', t => {
  const f = fixture(t), cache = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'cargo host cache-'));
  t.after(() => fs.rmSync(cache, { recursive: true, force: true }));
  f.metadata.target_directory = cache;
  const fake = fakeCommands(f), result = prepare(f, fake, 'darwin', { CARGO_TARGET_DIR: cache, CARGO_BUILD_TARGET: 'aarch64-unknown-linux-gnu' });
  const metadata = fake.calls.find(call => call.program === 'cargo' && call.args[0] === 'metadata');
  const build = fake.calls.find(call => call.program === 'cargo' && call.args[0] === 'build');
  assert.equal(metadata.options.env.CARGO_TARGET_DIR, cache);
  assert.equal(build.options.env.CARGO_TARGET_DIR, cache);
  assert.equal(build.options.env.CARGO_BUILD_TARGET, undefined);
  assert.equal(build.args.includes('--target'), false);
  assert.equal(result.cargoTargetDirectory, cache);
  assert.equal(result.bundle, f.app);
  assert.ok(result.backup.startsWith(path.join(f.root, 'target/gpui-preview/previous-')));
  assert.equal(fs.readFileSync(path.join(cache, 'debug/sample_gui'), 'utf8'), 'compiled-preview');
});

/** 普通目录以外的 metadata 缓存以及任何非默认 host 产物都不能被当作预览输入。 */
test('rejects_unsafe_cargo_target_directory_and_unknown_artifact_paths', t => {
  const linked = fixture(t), actual = path.join(linked.root, 'target/actual-cache'), alias = path.join(linked.root, 'target/cache-alias');
  fs.mkdirSync(actual); fs.symlinkSync(actual, alias); linked.metadata.target_directory = alias;
  const linkedFake = fakeCommands(linked);
  assert.throws(() => prepare(linked, linkedFake), /symbolic link/u);
  assert.equal(linkedFake.calls.some(call => call.program === 'cargo' && call.args[0] === 'build'), false);
  const nonDirectory = fixture(t); nonDirectory.metadata.target_directory = path.join(nonDirectory.root, 'LICENSE.en.md');
  assert.throws(() => prepare(nonDirectory, fakeCommands(nonDirectory)), /must be a directory/u);
  for (const relative of ['target/debug/other_gui', 'target/aarch64-apple-darwin/debug/sample_gui']) {
    const f = fixture(t), fake = fakeCommands(f, { artifactPath: path.join(f.root, relative) });
    assert.throws(() => prepare(f, fake), /differs from metadata target_directory/u);
    assert.equal(fs.readFileSync(f.binary, 'utf8'), 'previous-preview');
  }
});

/** 已运行、构建期间启动和备份瞬间启动分别阻断，旧预览均回到原路径。 */
test('refuses_running_preview_before_build_and_at_publication_boundaries', t => {
  for (const runningAt of [1, 2, 3]) {
    const f = fixture(t), fake = fakeCommands(f, { runningAt });
    assert.throws(() => prepare(f, fake), /preview is running/u);
    assert.equal(fs.readFileSync(f.binary, 'utf8'), 'previous-preview');
    if (runningAt === 1) assert.equal(fake.calls.some(call => call.program === 'cargo'), false);
  }
});

/** 签名、strict verify 和签名身份失败都发生于旧包重命名前。 */
test('signing_and_verification_failures_keep_previous_preview_bytes', t => {
  for (const setting of [{ signFailure: true }, { verifyFailure: true }, { wrongIdentity: true }]) {
    const f = fixture(t), fake = fakeCommands(f, setting);
    assert.throws(() => prepare(f, fake), /codesign failed|bound ad-hoc identity/u);
    assert.equal(fs.readFileSync(f.binary, 'utf8'), 'previous-preview');
    assert.equal(fs.readdirSync(path.join(f.root, 'target/gpui-preview')).some(name => name.startsWith('previous-')), false);
    assert.equal(fs.existsSync(path.join(f.root, 'target/gpui-preview/.prepare-lock')), false);
  }
});

/** Git 忽略资源仍核对字节；源码和已暂存资源漂移也不能发布。 */
test('rejects_ignored_resource_source_and_bundled_byte_drift', t => {
  for (const setting of [{ resourceDrift: true }, { sourceDrift: true }, { bundleDrift: true }]) {
    const f = fixture(t), fake = fakeCommands(f, setting);
    assert.throws(() => prepare(f, fake), /resources changed|source bytes changed|resource bytes differ/u);
    assert.equal(fs.readFileSync(f.binary, 'utf8'), 'previous-preview');
  }
});

/** 进程检查失败不能静默视为无进程；已有锁也不得被删除或接管。 */
test('process_inspection_failure_and_existing_lock_fail_closed', t => {
  const failed = fixture(t), ps = fakeCommands(failed, { psFailure: true });
  assert.throws(() => prepare(failed, ps), /ps failed/u);
  assert.equal(ps.calls.some(call => call.program === 'cargo'), false);
  const locked = fixture(t), lock = path.join(locked.root, 'target/gpui-preview/.prepare-lock');
  fs.mkdirSync(lock);
  assert.throws(() => prepare(locked, fakeCommands(locked)), /owns the lock/u);
  assert.equal(fs.existsSync(lock), true);
  assert.equal(fs.readFileSync(locked.binary, 'utf8'), 'previous-preview');
});

/** 新目标若被并发占用，保留旧包备份与陌生目标，不删除任何一方。 */
test('publication_collision_preserves_backup_without_overwriting_foreign_destination', t => {
  const f = fixture(t), fake = fakeCommands(f, { beforeProcess: count => {
    if (count === 3) { fs.mkdirSync(f.app); fs.writeFileSync(path.join(f.app, 'foreign'), 'foreign-owner'); }
  } });
  assert.throws(() => prepare(f, fake), /previous preview retained.*destination was not overwritten/u);
  assert.equal(fs.readFileSync(path.join(f.app, 'foreign'), 'utf8'), 'foreign-owner');
  const backup = fs.readdirSync(path.join(f.root, 'target/gpui-preview')).find(name => name.startsWith('previous-'));
  assert.equal(fs.readFileSync(path.join(f.root, 'target/gpui-preview', backup, 'Contents/MacOS/sample_gui'), 'utf8'), 'previous-preview');
});

/** 原有包、资源和 Cargo 产物中的符号链接均不能进入签名或发布。 */
test('rejects_symlinked_bundle_resources_and_cargo_artifact', t => {
  const bundle = fixture(t), saved = `${bundle.app}.saved`;
  fs.renameSync(bundle.app, saved); fs.symlinkSync(saved, bundle.app);
  assert.throws(() => prepare(bundle, fakeCommands(bundle)), /symbolic link/u);
  const resource = fixture(t);
  fs.symlinkSync(path.join(resource.root, 'LICENSE.en.md'), path.join(resource.root, 'runtime/skins/linked'));
  assert.throws(() => prepare(resource, fakeCommands(resource)), /symbolic link/u);
  const artifact = fixture(t);
  assert.throws(() => prepare(artifact, fakeCommands(artifact, { artifactLink: true })), /symbolic link/u);
  assert.equal(fs.readFileSync(artifact.binary, 'utf8'), 'previous-preview');
});

/** 无旧预览时也可发布；显式删除的已跟踪源码不被误判为链接或未知输入。 */
test('creates_first_preview_and_accepts_tracked_source_deletions', t => {
  const f = fixture(t, { withPrevious: false });
  fs.unlinkSync(path.join(f.root, 'sample_gui/src/main.rs'));
  const result = prepare(f, fakeCommands(f));
  assert.equal(result.backup, null);
  assert.equal(result.sourceDirty, true);
  assert.equal(fs.readFileSync(f.binary, 'utf8'), 'compiled-preview-signed-copy');
});

/** comm 只匹配完整路径，路径空格保留且相似后缀不误报。 */
test('process_matching_preserves_spaces_and_requires_exact_executable_path', () => {
  const executable = '/project with spaces/target/gpui-preview/LokiMetris.app/Contents/MacOS/sample_gui';
  const run = stdout => () => ({ status: 0, stdout, stderr: '' });
  assert.throws(() => assertPreviewStopped(run(`12 ${executable}\n`), [executable]), /pid 12/u);
  assert.doesNotThrow(() => assertPreviewStopped(run(`0 kernel_task\n12 ${executable}-other\n13 /usr/bin/node\n`), [executable]));
  assert.throws(() => assertPreviewStopped(run('unparseable\n'), [executable]), /cannot safely parse/u);
});

/** plist 转义不改变配置值语义，非 macOS 宿主在任何命令之前拒绝。 */
test('plist_escapes_identity_text_and_non_macos_never_runs_commands', t => {
  const f = fixture(t);
  const plist = renderPreviewPlist({ ...f.config, productName: '中&文', productNameEn: 'A "B"' }, { version: '0.2.0' });
  assert.ok(plist.includes('中&amp;文'));
  assert.ok(plist.includes('A &quot;B&quot;'));
  const fake = fakeCommands(f);
  assert.throws(() => prepare(f, fake, 'win32'), /requires macOS/u);
  assert.equal(fake.calls.length, 0);
});
