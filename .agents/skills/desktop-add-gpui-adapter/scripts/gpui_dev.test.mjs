/** 真实 Cargo/标准库 Rust 工程验证开发循环；不把该工程夹具当作原生窗口或候选验收。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { startGpuiDev, snapshotInputs, observeOwnedProcessGroup, knownOwnedProcessGroup } from './gpui_dev.mjs';
import { renderDefaultConfig } from '../../desktop-build-gpui-release/scripts/gpui_config.mjs';

const runner = fileURLToPath(new URL('./gpui_dev.mjs', import.meta.url));

/** 关闭回调遇到权限错误必须得到未知值，只有ESRCH才能证明组消失。 */
test('owned_group_observation_preserves_unknown_without_throwing', () => {
  const handle = { child: { pid: 1234 }, exited: true };
  const denied = Object.assign(new Error('denied'), { code: 'EPERM' });
  assert.deepEqual(observeOwnedProcessGroup(handle, () => { throw denied; }), { status: 'unknown', error: denied });
  assert.deepEqual(observeOwnedProcessGroup(handle, () => { throw Object.assign(new Error('gone'), { code: 'ESRCH' }); }), { status: 'gone' });
  let requested;
  assert.deepEqual(observeOwnedProcessGroup(handle, (pid, signal) => { requested = [pid, signal]; }), { status: 'present' });
  assert.deepEqual(requested, [-1234, 0]);
});

/** 瞬时权限未知在同一owned组上重查，随后ESRCH才允许收尾。 */
test('owned_group_transient_unknown_retries_until_confirmed_gone', async () => {
  const handle = { child: { pid: 1234 }, exited: true };
  let observations = 0;
  const status = await knownOwnedProcessGroup(handle, { intervalMs: 1, observe: current => {
    assert.equal(current, handle);
    return ++observations === 1 ? { status: 'unknown', error: new Error('EPERM') } : { status: 'gone' };
  } });
  assert.equal(status, 'gone'); assert.equal(observations, 2);
});

/** leader已退出仍不能忘记活后代；持续不可观测或未退出保持明确失败。 */
test('owned_group_persistent_unknown_or_live_descendants_never_report_cleanup', async () => {
  const handle = { child: { pid: 1234 }, exited: true };
  assert.equal(await knownOwnedProcessGroup(handle, { observe: () => ({ status: 'present' }) }), 'present');
  for (const status of ['unknown', 'present']) {
    await assert.rejects(knownOwnedProcessGroup(handle, { requireGone: true, timeoutMs: 5, intervalMs: 1, observe: () => ({ status, error: new Error('EPERM') }) }), /cleanup remains unverified/);
  }
});

/** leader退出后经历未知，即使同数字进程组再出现也不能恢复信号发送资格。 */
test('exited_leader_unknown_then_present_does_not_restore_group_identity', async () => {
  const handle = { child: { pid: 1234 }, exited: true };
  let observations = 0;
  await assert.rejects(knownOwnedProcessGroup(handle, { timeoutMs: 5, intervalMs: 1, observe: () => (
    ++observations === 1 ? { status: 'unknown', error: new Error('EPERM') } : { status: 'present' }
  ) }), /cleanup remains unverified/);
  assert.equal(handle.groupIdentityUncertain, true);
  assert.ok(observations >= 2);
});

/** 非默认包名/二进制名、含空格目录与未跟踪外部词典的 build.rs 覆盖真实 Cargo 缓存边界。 */
function fixture() {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'gpui dev 工程 '));
  const files = {
    '.cargo/config.toml': '[build]\ntarget-dir = "target"\n',
    'Cargo.toml': '[workspace]\nmembers = ["core", "native ui"]\nresolver = "2"\n[workspace.metadata.agent-first-harness]\ninterfaces = ["gui"]\ngui-framework = "gpui"\ntarget-platforms = ["macos", "windows", "linux"]\ngui-root = "native ui"\n',
    'core/Cargo.toml': '[package]\nname = "dev_core"\nversion = "0.1.0"\nedition = "2024"\n',
    'core/src/lib.rs': 'pub fn value() -> &\'static str { "core-one" }\n',
    'native ui/Cargo.toml': '[package]\nname = "custom_native"\nversion = "0.1.0"\nedition = "2024"\n[[bin]]\nname = "desktop_example"\npath = "src/main.rs"\n[dependencies]\ndev_core = { path = "../core" }\n',
    'native ui/build.rs': 'fn main() { println!("cargo:rerun-if-changed=build.rs"); let text = std::fs::read_to_string("locales/en.txt").unwrap(); std::fs::write(std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("locale.rs"), format!("{:?}", text)).unwrap(); }\n',
    'native ui/locales/en.txt': 'locale-one',
    'native ui/assets/logo.data': 'asset-one',
    'native ui/src/main.rs': `use std::{fs::OpenOptions, io::Write, thread, time::Duration};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut file = OpenOptions::new().append(true).create(true).open(&args[1]).unwrap();
    writeln!(file, "{}|{}|{}|{}|{}", std::process::id(), dev_core::value(), include!(concat!(env!("OUT_DIR"), "/locale.rs")), include_str!("../assets/logo.data"), args[2]).unwrap();
    loop { thread::sleep(Duration::from_millis(50)); }
}
`,
  };
  for (const [name, content] of Object.entries(files)) {
    const file = path.join(root, name); fs.mkdirSync(path.dirname(file), { recursive: true }); fs.writeFileSync(file, content);
  }
  fs.mkdirSync(path.join(root, 'packaging'));
  const config = renderDefaultConfig({ projectId: 'dev_fixture', nameZh: '工程验证', nameEn: 'Engineering Fixture', owner: 'Owner' });
  config.package = 'custom_native'; config.binary = 'desktop_example';
  fs.writeFileSync(path.join(root, 'packaging/gpui.json'), JSON.stringify(config));
  return root;
}

/** 有界等待只观察真实日志/进程结果，失败保留最后一次断言消息。 */
async function until(check, message) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) { if (check()) return; await delay(40); }
  assert.fail(message);
}

/** 进程存在性用信号零探测，不把关闭父 Cargo 当成应用已被回收。 */
function alive(pid) {
  try { process.kill(Number(pid), 0); return true; }
  catch (error) { if (error.code === 'ESRCH') return false; throw error; }
}

/** 原子资源保存、增删与自定义 target 排除都改变正确的输入集合。 */
test('input_snapshot_tracks_sources_and_arbitrary_resources_but_ignores_outputs_and_links', () => {
  const root = fixture();
  try {
    const excluded = [path.join(root, 'custom-output')];
    const initial = snapshotInputs(root, excluded);
    for (const name of ['target/a.rs', 'release/a.rs', 'node_modules/a.rs', '.git/a.rs', 'custom-output/a.rs']) {
      fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true }); fs.writeFileSync(path.join(root, name), 'generated');
    }
    assert.equal(snapshotInputs(root, excluded), initial);
    fs.writeFileSync(path.join(root, 'native ui/assets/new.custom-format'), 'new');
    assert.notEqual(snapshotInputs(root, excluded), initial);
    fs.unlinkSync(path.join(root, 'native ui/assets/new.custom-format'));
    assert.equal(snapshotInputs(root, excluded), initial);
    const link = path.join(root, 'native ui/assets/link');
    if (process.platform !== 'win32') { fs.symlinkSync(root, link, 'dir'); assert.equal(snapshotInputs(root, excluded), initial); }
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

/** 真实编译和运行证明首次启动、core/词典/资源更新、失败恢复及 app 主动退出后仍可开发。 */
test('dev_rebuilds_real_rust_and_resources_recovers_from_errors_and_reaps_app', { timeout: 90_000 }, async () => {
  const root = fixture(), evidence = path.join(root, 'starts.log'), messages = [];
  let session;
  const starts = () => fs.existsSync(evidence) ? fs.readFileSync(evidence, 'utf8').trim().split('\n') : [];
  try {
    session = await startGpuiDev(root, { appArgs: [evidence, 'literal $(no-shell) & spaces'], pollMs: 40, debounceMs: 80, log: value => messages.push(value) });
    await until(() => starts().length === 1, 'first application did not start');
    assert.match(starts()[0], /core-one\|locale-one\|asset-one\|literal \$\(no-shell\) & spaces$/u);
    const firstPid = starts()[0].split('|')[0];
    fs.writeFileSync(path.join(root, 'core/src/lib.rs'), 'pub fn value() -> &\'static str { "core-two" }\n');
    await until(() => starts().length === 2, 'core update did not restart the application');
    assert.equal(alive(firstPid), false);
    assert.match(starts()[1], /core-two\|locale-one/u);
    // build.rs 故意不跟踪词典；仅重新运行 cargo build 会复用旧条目。
    fs.writeFileSync(path.join(root, 'native ui/locales/en.txt'), 'locale-two');
    await until(() => starts().length === 3, 'locale update did not restart');
    assert.match(starts()[2], /core-two\|locale-two/u);
    fs.writeFileSync(path.join(root, 'native ui/assets/logo.data'), 'asset-two');
    await until(() => starts().length === 4, 'asset update did not restart');
    assert.match(starts()[3], /locale-two\|asset-two/u);
    fs.mkdirSync(path.join(root, 'target'), { recursive: true });
    fs.writeFileSync(path.join(root, 'target/generated.rs'), 'ignored output');
    await delay(300);
    assert.equal(starts().length, 4, 'build outputs caused a restart loop');
    const source = fs.readFileSync(path.join(root, 'core/src/lib.rs'), 'utf8');
    fs.writeFileSync(path.join(root, 'core/src/lib.rs'), 'invalid Rust source');
    await until(() => messages.some(message => message.startsWith('Build failed')), 'compiler failure was not reported');
    assert.equal(starts().length, 4);
    fs.writeFileSync(path.join(root, 'core/src/lib.rs'), source.replace('core-two', 'core-new'));
    await until(() => starts().length === 5, 'compile failure did not recover');
    process.kill(Number(starts()[4].split('|')[0]));
    await until(() => messages.some(message => /Application exited/u.test(message)), 'normal app exit was not observed');
    fs.writeFileSync(path.join(root, 'core/src/lib.rs'), source.replace('core-two', 'core-end'));
    await until(() => starts().length === 6, 'watcher did not survive application exit');
    const lastPid = starts()[5].split('|')[0];
    await session.close(); await session.done;
    assert.equal(alive(lastPid), false, 'application survived watcher shutdown');
  } finally { await session?.close(); fs.rmSync(root, { recursive: true, force: true }); }
});

/** 真实慢构建期间继续保存，构建完成后必须追加编译且只运行最新内容。 */
test('edits_during_build_are_coalesced_without_launching_stale_artifacts', { timeout: 60_000 }, async () => {
  const root = fixture(), evidence = path.join(root, 'starts.log'), messages = [];
  let session;
  try {
    fs.appendFileSync(path.join(root, 'native ui/build.rs'), '\n');
    const build = fs.readFileSync(path.join(root, 'native ui/build.rs'), 'utf8').replace('fn main() {', 'fn main() { std::thread::sleep(std::time::Duration::from_millis(600));');
    fs.writeFileSync(path.join(root, 'native ui/build.rs'), build);
    session = await startGpuiDev(root, { appArgs: [evidence, 'safe'], pollMs: 30, debounceMs: 100, log: value => messages.push(value) });
    await until(() => messages.some(message => message.startsWith('Building')), 'build did not start');
    for (const value of ['first-save', 'last-save']) {
      fs.writeFileSync(path.join(root, 'native ui/locales/en.txt'), value); await delay(70);
    }
    await until(() => fs.existsSync(evidence) && fs.readFileSync(evidence, 'utf8').includes('last-save'), 'latest artifact did not launch');
    assert.equal(fs.readFileSync(evidence, 'utf8').trim().split('\n').length, 1);
    assert.match(fs.readFileSync(evidence, 'utf8'), /last-save/u);
    assert.ok(messages.filter(message => message.startsWith('Building')).length >= 2);
  } finally { await session?.close(); fs.rmSync(root, { recursive: true, force: true }); }
});

/** CLI 中断回收真实应用；只启用 tracked Cargo.lock，不依赖前端锁文件或 npm 安装。 */
test('cli_sigterm_cleans_up_app_and_tracked_lock_policy_builds', { timeout: 60_000, skip: process.platform === 'win32' }, async () => {
  const root = fixture(), evidence = path.join(root, 'starts.log');
  let child, childDone;
  try {
    const generated = spawnSync('cargo', ['generate-lockfile', '--offline'], { cwd: root, encoding: 'utf8' });
    assert.equal(generated.status, 0, generated.stderr);
    fs.appendFileSync(path.join(root, 'Cargo.toml'), 'dependency-lock-policy = "tracked"\n');
    assert.equal(spawnSync('git', ['init', '-q'], { cwd: root }).status, 0);
    await assert.rejects(startGpuiDev(root, { log: () => {} }), /Git.*Cargo.lock/u);
    assert.equal(spawnSync('git', ['add', 'Cargo.lock'], { cwd: root }).status, 0);
    child = spawn(process.execPath, [runner, '--root', root, '--', evidence, 'safe'], { stdio: ['ignore', 'ignore', 'pipe'] });
    let diagnostic = ''; child.stderr.on('data', chunk => { diagnostic += chunk; });
    childDone = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', code => resolve(code)); });
    await until(() => fs.existsSync(evidence) && fs.readFileSync(evidence, 'utf8').includes('|safe'), `CLI application did not start: ${diagnostic}`);
    const pid = fs.readFileSync(evidence, 'utf8').split('|')[0];
    child.kill('SIGTERM');
    assert.equal(await childDone, 0);
    assert.equal(alive(pid), false);
    assert.equal(fs.existsSync(path.join(root, 'pnpm-lock.yaml')), false);
  } finally { if (child?.exitCode === null) child.kill('SIGTERM'); await childDone; fs.rmSync(root, { recursive: true, force: true }); }
});

/** leader 先退出且后代忽略 TERM 时仍清理整组，不能只回收最外层 GUI/Cargo。 */
test('shutdown_reaps_owned_descendants_after_the_application_leader_exits', { timeout: 60_000, skip: process.platform === 'win32' }, async () => {
  const root = fixture(), evidence = path.join(root, 'starts.log'), workerPid = path.join(root, 'worker.log');
  let session;
  try {
    const file = path.join(root, 'native ui/src/main.rs');
    const worker = `process.on('SIGTERM', () => {}); require('node:fs').writeFileSync(process.argv[1], String(process.pid)); setInterval(() => {}, 1000);`;
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace('let mut file =', `let _worker = std::process::Command::new(&args[3]).args(["-e", ${JSON.stringify(worker)}, &args[4]]).spawn().unwrap();\n    let mut file =`));
    session = await startGpuiDev(root, { appArgs: [evidence, 'safe', process.execPath, workerPid], log: () => {} });
    await until(() => fs.existsSync(workerPid) && fs.readFileSync(workerPid, 'utf8').length > 0, 'owned worker did not start');
    const pid = fs.readFileSync(workerPid, 'utf8');
    await session.close();
    await until(() => !alive(pid), 'owned descendant survived watcher shutdown');
  } finally { await session?.close(); fs.rmSync(root, { recursive: true, force: true }); }
});

/** Ctrl+C 等中断在 Cargo 正运行 build.rs 时也必须回收构建后代。 */
test('abort_during_compilation_reaps_the_running_build_script', { timeout: 60_000 }, async () => {
  const root = fixture(), buildPid = path.join(root, 'build.log');
  let session;
  try {
    const file = path.join(root, 'native ui/build.rs');
    fs.writeFileSync(file, fs.readFileSync(file, 'utf8').replace('fn main() {', 'fn main() { std::fs::write("../build.log", std::process::id().to_string()).unwrap(); std::thread::sleep(std::time::Duration::from_secs(20));'));
    const controller = new AbortController();
    session = await startGpuiDev(root, { signal: controller.signal, log: () => {} });
    await until(() => fs.existsSync(buildPid) && fs.readFileSync(buildPid, 'utf8').length > 0, 'build script did not start');
    const pid = fs.readFileSync(buildPid, 'utf8');
    controller.abort(); await session.done;
    await until(() => !alive(pid), 'build script survived abort');
  } finally { await session?.close(); fs.rmSync(root, { recursive: true, force: true }); }
});
