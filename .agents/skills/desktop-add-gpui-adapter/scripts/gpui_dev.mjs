#!/usr/bin/env node
/** 原生 Rust 开发循环：监听输入、增量编译并重启 owned 二进制，不安装额外 watcher。 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { readConfig, projectFacts } from '../../desktop-build-gpui-release/scripts/gpui_config.mjs';
import { safePath } from '../../desktop-build-gpui-release/scripts/gpui_filesystem.mjs';
import { readDependencyLockPolicy, assertDependencyLocks } from '../../desktop-implement-change/scripts/project_lock_policy.mjs';

const ignored = new Set(['.git', '.agents', '.harness', 'target', 'node_modules', 'release', 'dist', '.cache']);
const resources = new Set(['assets', 'resources', 'locales', 'packaging', '.cargo']);

/** 跨平台轮询不跟随链接；元数据签名覆盖增删、原子保存、同大小写入及任意格式的资源。 */
export function snapshotInputs(root, excludedRoots = [], resourcesOnly = false) {
  const hash = crypto.createHash('sha256');
  let count = 0;
  function visit(directory, relative = '') {
    let entries;
    try { entries = fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)); }
    catch (error) { if (error.code === 'ENOENT') return; throw error; }
    for (const entry of entries) {
      if (entry.isSymbolicLink() || ignored.has(entry.name) || entry.name.startsWith('.gpui-')) continue;
      const file = path.join(directory, entry.name), name = path.join(relative, entry.name);
      if (excludedRoots.some(excluded => file === excluded || file.startsWith(`${excluded}${path.sep}`))) continue;
      if (entry.isDirectory()) { visit(file, name); continue; }
      if (!entry.isFile()) continue;
      const resource = name.split(path.sep).some(segment => resources.has(segment));
      if (resourcesOnly && !resource) continue;
      if (!resource && !entry.name.endsWith('.rs') && !['Cargo.toml', 'Cargo.lock', 'rust-toolchain', 'rust-toolchain.toml'].includes(entry.name)) continue;
      if (++count > 50_000) throw new Error('development input scan exceeds 50000 files');
      let stat;
      try { stat = fs.statSync(file, { bigint: true }); }
      catch (error) { if (error.code === 'ENOENT') continue; throw error; }
      hash.update(JSON.stringify([name, String(stat.size), String(stat.mtimeNs), String(stat.ctimeNs)]));
    }
  }
  visit(root);
  return hash.digest('hex');
}

/** leader 退出不代表后代结束；权限或其他探测错误保持未知，不能当作组已消失。 */
export function observeOwnedProcessGroup(handle, probe = process.kill) {
  if (!handle.child.pid) return { status: 'gone' };
  try { probe(-handle.child.pid, 0); return { status: 'present' }; }
  catch (error) { return error.code === 'ESRCH' ? { status: 'gone' } : { status: 'unknown', error }; }
}

/** 只重查本会话拥有的组；持续不可观测时失败并保留句柄，不扩大终止范围。 */
export async function knownOwnedProcessGroup(handle, { observe = observeOwnedProcessGroup, timeoutMs = 2000, intervalMs = 20, requireGone = false } = {}) {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    const result = observe(handle);
    // leader退出后的未知身份不能因同数字PGID再次存在而恢复发送信号的资格。
    if (result.status === 'unknown' && handle.exited) handle.groupIdentityUncertain = true;
    if (result.status === 'gone' || (result.status === 'present' && !requireGone && !handle.groupIdentityUncertain)) return result.status;
    if (performance.now() >= deadline) throw new Error('could not observe owned development process group; cleanup remains unverified', { cause: result.error });
    await new Promise(resolve => setTimeout(resolve, intervalMs));
  }
}

/** 每个构建/应用拥有独立进程组；仅终止本循环创建的进程与后代。 */
async function terminateProcess(handle) {
  if (handle.exited && (process.platform === 'win32' || await knownOwnedProcessGroup(handle) === 'gone')) { handle.cleanup(); return; }
  if (process.platform === 'win32') {
    await new Promise((resolve, reject) => {
      const killer = spawn('taskkill', ['/PID', String(handle.child.pid), '/T', '/F'], { shell: false, windowsHide: true, stdio: 'ignore' });
      killer.once('error', reject);
      killer.once('exit', code => code === 0 || handle.exited ? resolve() : reject(new Error('could not stop owned development process')));
    });
  } else {
    const kill = signal => {
      try { process.kill(-handle.child.pid, signal); }
      catch (error) { if (error.code !== 'ESRCH') throw error; }
    };
    kill('SIGTERM');
    let timer;
    await Promise.race([handle.done.catch(() => {}), new Promise(resolve => { timer = setTimeout(resolve, 2000); })]);
    clearTimeout(timer);
    if (await knownOwnedProcessGroup(handle) === 'present') kill('SIGKILL');
  }
  await handle.done.catch(() => {});
  if (process.platform !== 'win32') await knownOwnedProcessGroup(handle, { requireGone: true });
  handle.cleanup();
}

/** 重编译与关闭可能同时收尾同一 handle，复用停止任务避免 Windows 重复 taskkill 误报。 */
function stopProcess(handle) {
  if (!handle) return Promise.resolve();
  handle.stopping ??= terminateProcess(handle);
  return handle.stopping;
}

/** 开发会话只启动调试二进制；编译失败保留监听，后续保存可恢复，关闭时回收所有 owned 进程。 */
export async function startGpuiDev(rootValue, { appArgs = [], pollMs = 250, debounceMs = 200, signal = null, log = message => process.stderr.write(`[gpui dev] ${message}\n`) } = {}) {
  const root = fs.realpathSync(path.resolve(rootValue));
  safePath(root, 'Cargo.toml', { type: 'file' });
  const children = new Set();
  let app = null, closed = false, busy = false, pending = false, deadline = 0, timer = null, builtResources = null;
  let resolveDone, rejectDone;
  const done = new Promise((resolve, reject) => { resolveDone = resolve; rejectDone = reject; });
  // 初始化失败前也安装 rejection handler，调用方仍通过 done 获取真实失败。
  done.catch(() => {});
  const abort = () => { void close().catch(() => {}); };
  signal?.addEventListener('abort', abort, { once: true });
  if (signal?.aborted) abort();

  function launch(command, args, onLine = null, capture = false) {
    if (closed) throw new Error('development session is closed');
    const child = spawn(command, args, { cwd: root, shell: false, detached: process.platform !== 'win32', windowsHide: true, stdio: ['ignore', onLine || capture ? 'pipe' : 'inherit', 'inherit'] });
    const handle = { child, exited: false, done: null, cleanup: () => children.delete(handle) };
    children.add(handle);
    child.stdout?.setEncoding('utf8');
    handle.done = new Promise((resolve, reject) => {
      let output = '', partial = '';
      child.stdout?.on('data', chunk => {
        if (capture) {
          output += chunk;
          if (Buffer.byteLength(output) > 16 * 1024 * 1024) { child.kill(); reject(new Error('Cargo metadata exceeds 16 MiB')); }
        } else {
          partial += chunk;
          let newline;
          while ((newline = partial.indexOf('\n')) !== -1) {
            const line = partial.slice(0, newline); partial = partial.slice(newline + 1);
            onLine(line);
          }
        }
      });
      child.once('error', error => { handle.exited = true; children.delete(handle); reject(error); });
      child.once('close', (code, signal) => {
        handle.exited = true;
        // 探测失败不能逃出事件回调或提前遗忘后代，done 仍须完成供关闭阶段有界复查。
        const state = process.platform === 'win32' ? 'gone' : observeOwnedProcessGroup(handle).status;
        if (state === 'unknown') handle.groupIdentityUncertain = true;
        if (state === 'gone') handle.cleanup();
        if (partial) onLine?.(partial);
        resolve({ code, signal, output });
      });
    });
    return handle;
  }

  async function resolveProject() {
    const config = readConfig(root);
    const lockPolicy = readDependencyLockPolicy(root);
    if (lockPolicy === 'tracked') assertDependencyLocks(root);
    const args = ['metadata', '--format-version', '1', '--no-deps'];
    if (lockPolicy === 'tracked') args.push('--locked');
    const result = await launch('cargo', args, null, true).done;
    if (result.code !== 0) throw new Error('Cargo metadata failed');
    const metadata = JSON.parse(result.output);
    const facts = projectFacts(root, metadata, config);
    if (facts.lockPolicy === 'tracked') assertDependencyLocks(root, { rustTestManifests: facts.manifests });
    return { config, facts, metadata };
  }

  let project, excluded, signature;
  try {
    project = await resolveProject();
    excluded = [path.resolve(project.metadata.target_directory)];
    if (excluded[0] === root) throw new Error('Cargo target directory must differ from workspace root');
    signature = snapshotInputs(root, excluded);
  }
  catch (error) { await close().catch(() => {}); throw error; }

  async function rebuild() {
    if (closed || busy || !pending || Date.now() < deadline) return;
    busy = true; pending = false;
    try {
      // Windows 不允许覆盖正在运行的 exe；所有平台都先停止旧应用，避免并存及单实例转发。
      await stopProcess(app); app = null;
      project = await resolveProject();
      excluded = [path.resolve(project.metadata.target_directory)];
      if (excluded[0] === root) throw new Error('Cargo target directory must differ from workspace root');
      const { config, facts } = project;
      const resourceSignature = snapshotInputs(root, excluded, true);
      if (resourceSignature !== builtResources) {
        // 既有 build.rs/过程宏可能没有声明外部资源依赖；只清理所选 GUI 的调试缓存，保留依赖及发布产物。
        const cleanArgs = ['clean', '--package', config.package, '--profile', 'dev'];
        if (facts.lockPolicy === 'tracked') cleanArgs.push('--locked');
        const cleaned = await launch('cargo', cleanArgs).done;
        if (cleaned.code !== 0) throw new Error('Could not invalidate GUI debug resource cache');
      }
      log(`Building ${config.package}/${config.binary} (debug).`);
      let executable = null;
      const args = ['build', '--package', config.package, '--bin', config.binary, '--message-format=json-render-diagnostics'];
      if (facts.lockPolicy === 'tracked') args.push('--locked');
      const result = await launch('cargo', args, line => {
        let message;
        try { message = JSON.parse(line); } catch { return; }
        if (message.reason === 'compiler-artifact' && message.target?.name === config.binary && message.target?.kind?.includes('bin') && message.executable) executable = message.executable;
      }).done;
      if (closed) return;
      if (result.code !== 0) throw new Error('Build failed; save an input to retry.');
      if (!executable || !fs.statSync(executable).isFile()) throw new Error('Cargo did not report the selected executable');
      builtResources = resourceSignature;
      // 编译期间再次保存时不启动过时产物；poll 之外再取快照，覆盖短于轮询周期的构建。
      const current = snapshotInputs(root, excluded);
      if (current !== signature) { signature = current; pending = true; deadline = Date.now() + debounceMs; }
      if (pending) return;
      log('Starting application. Changes rebuild and restart it; in-memory state resets.');
      app = launch(executable, appArgs);
      app.done.then(result => { if (!closed) log(`Application exited (${result.code ?? result.signal}); watching for changes.`); }, error => log(`Application failed: ${error.message}; watching for changes.`));
    } catch (error) { if (!closed) log(error.message); }
    finally { busy = false; }
  }

  async function close(error = null) {
    if (closed) return done;
    closed = true; clearInterval(timer); signal?.removeEventListener('abort', abort);
    try {
      await Promise.all([...children].map(stopProcess));
      if (error) rejectDone(error); else resolveDone();
    } catch (failure) { rejectDone(failure); }
    return done;
  }

  timer = setInterval(() => {
    if (closed) return;
    try {
      const current = snapshotInputs(root, excluded);
      if (signature !== current) { signature = current; pending = true; deadline = Date.now() + debounceMs; }
      void rebuild();
    } catch (error) { void close(error).catch(() => {}); }
  }, pollMs);
  log('Watching Rust/Cargo inputs, .cargo, locales, assets, resources and packaging. Ctrl+C stops the session.');
  pending = true;
  void rebuild();
  return { done, close };
}

/** 根目录参数由 pnpm 提供，-- 后的应用参数作为数组直传，不进入 Shell。 */
export async function main(argv = process.argv.slice(2)) {
  const separator = argv.indexOf('--');
  const { values } = parseArgs({ args: separator < 0 ? argv : argv.slice(0, separator), options: { root: { type: 'string' } }, allowPositionals: false });
  if (!values.root) throw new Error('--root is required');
  const controller = new AbortController();
  const stop = () => controller.abort();
  process.on('SIGINT', stop); process.on('SIGTERM', stop);
  try {
    const session = await startGpuiDev(values.root, { appArgs: separator < 0 ? [] : argv.slice(separator + 1), signal: controller.signal });
    await session.done;
  }
  finally { process.off('SIGINT', stop); process.off('SIGTERM', stop); }
}

if (path.resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  main().catch(error => { process.stderr.write(`GPUI dev: ${error.message}\n`); process.exitCode = 1; });
}
