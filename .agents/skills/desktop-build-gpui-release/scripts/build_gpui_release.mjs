#!/usr/bin/env node
/** GPUI 原生构建与项目隔离打包入口，不调用 Tauri、前端、安装或远端。 */
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs, isDeepStrictEqual } from 'node:util';
import { randomUUID, createHash } from 'node:crypto';
import { assertDependencyLocks, readDependencyLockPolicy } from '../../desktop-implement-change/scripts/project_lock_policy.mjs';
import { safePath, safeRoot, sha256, isolateRelease, publishDirectory, exactFiles } from './gpui_filesystem.mjs';
import { readConfig, projectFacts, TARGETS, PACKAGER_VERSION } from './gpui_config.mjs';
import { candidateSelection, releasedContext, releaseNotes, aboutPageEnabled, renderReleaseNotesRust, containsBytes, candidateManifest } from './gpui_evidence.mjs';
import { packagerConfig, packagePath, signingChoice, finalizeSigning, verifyResources, verifyMacIdentity, verifyDmg, verifyNsis } from './gpui_packager.mjs';
import { renderPlatformIcons, renderDmgBackground } from './gpui_icons.mjs';

/** 工程入口只接受完整稳定版本，较高正式版本不必降级到最低 LTS。 */
export function assertNodeRuntime(version = process.versions.node) {
  const match = typeof version === 'string' && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/u.exec(version);
  const supported = match && (BigInt(match[1]) > 24n || (BigInt(match[1]) === 24n && BigInt(match[2]) >= 21n));
  if (!supported) throw new Error('GPUI packaging requires stable Node.js >=24.21.0');
}

/** 命令以 argv 执行；签名命令隐藏诊断，任何非零/超时直接阻断。 */
export function commandRunner(root) {
  return (program, args, { env = process.env, quiet = false, timeout = 30 * 60 * 1000 } = {}) => {
    const result = spawnSync(program,args,{ cwd: root, env, encoding: 'utf8', input: '', maxBuffer: 64 * 1024 * 1024, timeout, windowsHide: true });
    if (result.error || result.status !== 0) throw new Error(`${path.basename(program)} failed${quiet ? '' : `: ${(result.error?.message || result.stderr?.trim() || result.stdout?.trim() || String(result.status)).slice(-8000)}`}`);
    if (!quiet && result.stderr) process.stderr.write(result.stderr);
    return result;
  };
}

/** 根 Git 身份检查适用于 local 与 candidate，禁止在 Harness 源直接打产品包。 */
export function gitIdentity(root, run) {
  if (path.resolve(run('git',['rev-parse','--show-toplevel']).stdout.trim()) !== root) throw new Error('project root must equal independent Git top level');
  if (fs.existsSync(path.join(root,'Version.md')) && fs.existsSync(path.join(root,'.agents/skills/desktop-instantiate-project/SKILL.md'))) throw new Error('Harness source cannot build a product');
  const commit = run('git',['rev-parse','HEAD']).stdout.trim();
  if (!/^[0-9a-f]{40}$/u.test(commit)) throw new Error('project must have a committed source identity');
  const status = run('git',['status','--porcelain=v1','--untracked-files=all']).stdout;
  const digest = createHash('sha256').update(status).update(run('git',['diff','--binary','HEAD']).stdout);
  const untracked = run('git',['ls-files','--others','--exclude-standard','-z']).stdout.split('\0').filter(Boolean).sort();
  for (const relative of untracked) digest.update(relative).update(sha256(safePath(root,relative,{ type: 'file' })));
  return { commit, dirty: !!status.trim(), status, sourceSha256: digest.digest('hex') };
}

/** local 可从 dirty 开始，但源文件/HEAD 在本次测试和构建过程中必须保持同一快照。 */
function assertSourceStable(root, run, original) {
  if (!isDeepStrictEqual(gitIdentity(root,run),original)) throw new Error('source commit or worktree bytes changed during build');
}

/** tracked 策略在 metadata、测试和构建全部传 --locked。 */
function cargoArguments(base, lockPolicy) { return [...base,...(lockPolicy === 'tracked' ? ['--locked'] : [])]; }

/** 隔离目录必须真实受 Git 忽略，避免构建污染 clean 源码。 */
function assertIgnored(run, relative) {
  const result = run('git',['check-ignore','-q','--no-index','--',relative]);
  if (result.status !== 0) throw new Error(`build directory must be Git-ignored: ${relative}`);
}

/** macOS 通知候选必须明确选择签名；缺失或歧义 profile 在任何测试和旧候选隔离前失败关闭。 */
function assertMacosNotificationSigning(root, signing) {
  const source = fs.readFileSync(safePath(root,'docs/GUI_APP_PROFILE.md',{ type: 'file' }),'utf8');
  const headers = [...source.matchAll(/^```gui-initialization-config[ \t]*\r?$/gmu)];
  const blocks = [...source.matchAll(/^```gui-initialization-config[ \t]*\r?\n([\s\S]*?)^```[ \t]*\r?$/gmu)];
  if (headers.length !== 1 || blocks.length !== 1 || blocks[0][1].includes('```')) throw new Error('GUI profile requires one closed initialization configuration block');
  const fields = [...blocks[0][1].matchAll(/^[ \t]*system_notification[ \t]*:[ \t]*(.*?)[ \t]*\r?$/gmu)];
  if (fields.length !== 1 || !['enabled','disabled'].includes(fields[0][1])) throw new Error('GUI profile requires one explicit system_notification choice');
  if (fields[0][1] === 'enabled' && signing.selection !== 'enabled') throw new Error('macOS candidate with system_notification = enabled requires enabled signing; unsigned candidates are unsupported');
}

/** 每个真实 workspace 单独运行非空全量单元测试，不用子集或 mock 当构建证据。 */
export function runWorkspaceTests(root, facts, run, env) {
  const seen = new Set(), suites = [];
  for (const manifest of facts.manifests) {
    const args = cargoArguments(['metadata','--no-deps','--format-version','1','--manifest-path',safePath(root,manifest,{ type: 'file' })],facts.lockPolicy);
    const workspace = JSON.parse(run('cargo',args,{ env }).stdout).workspace_root;
    const relative = path.relative(root,workspace).split(path.sep).join('/');
    if (relative) safePath(root,relative,{ type: 'directory' });
    if (path.resolve(workspace) !== root && (!relative || relative.startsWith('..') || path.isAbsolute(relative))) throw new Error('test workspace escapes project');
    if (seen.has(workspace)) continue;
    seen.add(workspace);
    const command = cargoArguments(['test','--workspace','--all-targets','--all-features','--manifest-path',path.join(workspace,'Cargo.toml')],facts.lockPolicy);
    const listing = run('cargo',[...command,'--','--list'],{ env });
    const discovered = (listing.stdout.match(/^.+: test\r?$/gmu) ?? []).length;
    if (!discovered) throw new Error(`workspace has zero unit tests: ${manifest}`);
    const execution = run('cargo',command,{ env });
    const passed = [...execution.stdout.matchAll(/test result: ok\. (\d+) passed;/gu)].reduce((sum,match) => sum + Number(match[1]),0);
    if (!passed) throw new Error(`workspace executed zero passing tests: ${manifest}`);
    suites.push({ manifest, discovered, passed, status: 'passed' });
  }
  return { status: 'passed', discovered: suites.reduce((sum,item) => sum + item.discovered,0), passed: suites.reduce((sum,item) => sum + item.passed,0), suites };
}

/** 清除可隐式影响签名和构建产物位置的环境，仅传本次明确选择。 */
function buildEnvironment(config, target, cargoDirectory, candidate, notesPath, environment) {
  const env = { ...environment, CARGO_TARGET_DIR: cargoDirectory, CARGO_BUILD_TARGET: target };
  for (const key of Object.keys(env)) if (key.startsWith('APPLE_') || key.startsWith('CARGO_PACKAGER_') || key === 'HARNESS_GPUI_RELEASE_NOTES_RS') delete env[key];
  if (candidate) env.HARNESS_GPUI_RELEASE_NOTES_RS = notesPath;
  if (TARGETS[target].platform === 'macos') env.MACOSX_DEPLOYMENT_TARGET = config.macos.minimumSystemVersion;
  return env;
}

/** 固定版本工具只装项目私有目录，已有不同版本或残缺安装不覆盖。 */
export function setupPackager(root, run, platform = process.platform) {
  assertIgnored(run,'target/harness-tools/.gpui-private');
  const relative = `target/harness-tools/cargo-packager-${PACKAGER_VERSION}`;
  const destination = safePath(root,relative,{ missing: true });
  const tool = path.join(destination,'bin',`cargo-packager${platform === 'win32' ? '.exe' : ''}`);
  if (fs.existsSync(destination)) {
    safePath(root,`${relative}/bin/${path.basename(tool)}`,{ type: 'file' });
    if (run(tool,['--version']).stdout.trim() !== `cargo-packager ${PACKAGER_VERSION}`) throw new Error('project-local cargo-packager has a different version; refusing overwrite');
    return tool;
  }
  const parent = safePath(root,'target/harness-tools',{ missing: true }); fs.mkdirSync(parent,{ recursive: true });
  const stage = fs.mkdtempSync(path.join(parent,'.install-'));
  try {
    run('cargo',['install','cargo-packager','--version',PACKAGER_VERSION,'--locked','--root',stage]);
    const executable = safePath(stage,`bin/${path.basename(tool)}`,{ type: 'file' });
    if (run(executable,['--version']).stdout.trim() !== `cargo-packager ${PACKAGER_VERSION}`) throw new Error('installed cargo-packager version mismatch');
    safePath(root,relative,{ missing: true });
    if (fs.existsSync(destination)) throw new Error('tool directory appeared during installation');
    fs.renameSync(stage,destination);
  } catch (error) { fs.rmSync(stage,{ recursive: true, force: true }); throw error; }
  return tool;
}

/** 平台图标只填充缺失文件，相同字节幂等、不同字节需回身份流程。 */
export function writeIcons(root, config) {
  const bytes = fs.readFileSync(safePath(root,config.icons.source,{ type: 'file' }));
  const icons = renderPlatformIcons(bytes);
  const outputs = [[config.icons.macos,icons.macos],[config.icons.windows,icons.windows],[config.macos.background,renderDmgBackground()]];
  for (const [relative,data] of outputs) {
    const file = safePath(root,relative,{ missing: true });
    if (fs.existsSync(file) && !fs.readFileSync(file).equals(data)) throw new Error(`refusing to overwrite packaging identity asset: ${relative}`);
  }
  for (const [relative,data] of outputs) {
    const file = safePath(root,relative,{ missing: true }); fs.mkdirSync(path.dirname(file),{ recursive: true });
    if (!fs.existsSync(file)) fs.writeFileSync(file,data,{ flag: 'wx' });
  }
  return outputs.map(([file]) => file);
}

/** 完整构建只写忽略的隔离区；测试 hook 仅由模块调用方注入，CLI 无覆盖开关。 */
export function executeBuild(options, hooks = {}) {
  const root = safeRoot(options.root), run = hooks.run ?? commandRunner(root);
  const platform = hooks.platform ?? process.platform, environment = hooks.env ?? process.env;
  const identity = gitIdentity(root,run), config = readConfig(root);
  const lockPolicy = readDependencyLockPolicy(root);
  const metadata = JSON.parse(run('cargo',cargoArguments(['metadata','--no-deps','--format-version','1'],lockPolicy)).stdout);
  const facts = projectFacts(root,metadata,config);
  if (options.command === 'icons') return { files: writeIcons(root,config) };
  if (options.command === 'setup') return { packager: setupPackager(root,run,platform), version: PACKAGER_VERSION };
  const target = options.target ?? run('rustc',['-vV']).stdout.match(/^host: (.+)$/mu)?.[1];
  const info = TARGETS[target], format = options.format ?? (platform === 'darwin' ? 'dmg' : 'nsis');
  if (!info || !info.formats.includes(format) || info.host !== platform || !facts.platforms.includes(info.platform)) throw new Error('unsupported/unselected native GPUI target or format; Linux and cross-system packaging are unavailable');
  if (format === 'nsis' && (hooks.arch ?? process.arch) !== 'x64') throw new Error('Windows NSIS currently requires a native x64 host');
  const candidate = options.mode === 'candidate';
  if (!['candidate','local'].includes(options.mode)) throw new Error('--mode must be local or candidate');
  const signing = signingChoice(options.signing ?? 'disabled',options.signingSource ?? 'not-requested');
  if (!candidate && signing.selection !== 'disabled') throw new Error('local packages must be explicitly unsigned');
  if (candidate && info.platform === 'macos') assertMacosNotificationSigning(root,signing);
  if (info.platform === 'macos' && signing.selection === 'enabled') throw new Error('GPUI macOS signing/notarization is not implemented; enabled selection fails closed, never downgraded');
  if (format === 'nsis' && signing.selection === 'enabled' && !config.windows.signing) throw new Error('Windows signing configuration is incomplete');
  const selections = candidate ? candidateSelection(options.e2e,options.e2eReason,options.e2eRisk) : null;
  if (candidate && format === 'nsis' && !config.windows.extractor) throw new Error('NSIS candidate requires an approved project-local 7z/7zr extractor');
  const context = candidate ? (hooks.context ?? releasedContext)(root,facts) : null;
  if (candidate && identity.dirty) throw new Error('candidate source must be clean');
  const resources = ['LICENSE.zh-CN.md','LICENSE.en.md',...(candidate ? ['release-notes.json'] : [])];
  for (const file of resources) safePath(root,file,{ type: 'file' });
  const notes = candidate ? releaseNotes(root,facts.version) : null;
  const embedNotes = candidate && aboutPageEnabled(root);
  if (candidate) {
    for (const [file,name] of [['LICENSE.zh-CN.md',config.productName],['LICENSE.en.md',config.productNameEn]]) {
      if (!fs.readFileSync(path.join(root,file),'utf8').includes(name)) throw new Error(`license product identity differs: ${file}`);
    }
  }
  safePath(root,config.icons[format === 'nsis' ? 'windows' : 'macos'],{ type: 'file' });
  if (format === 'dmg') safePath(root,config.macos.background,{ type: 'file' });
  const toolRelative = `target/harness-tools/cargo-packager-${PACKAGER_VERSION}/bin/cargo-packager${platform === 'win32' ? '.exe' : ''}`;
  const buildRun = randomUUID();
  const plan = { buildRun, mode: options.mode, project: facts.project, version: facts.version, target, format, package: config.package, binary: config.binary, lockPolicy, signing, ...(candidate ? { sourceCommit: context.sourceCommit, ...selections } : { sourceCommit: identity.commit, dirty: identity.dirty }) };
  if (options.command === 'preview') return { ...plan, tool: fs.existsSync(safePath(root,toolRelative,{ missing: true })) ? 'present-version-unchecked' : 'setup-required' };
  const tool = safePath(root,toolRelative,{ type: 'file' });
  if (run(tool,['--version']).stdout.trim() !== `cargo-packager ${PACKAGER_VERSION}`) throw new Error('cargo-packager version differs; run setup');
  assertIgnored(run,'target/gpui-packaging/.gpui-private');
  if (candidate) {
    run(process.execPath,[safePath(root,'.agents/skills/desktop-manage-version/scripts/version_gate.mjs',{ type: 'file' }),'check','--project-root',root,'--phase','release']);
    assertIgnored(run,'release/.gpui-candidate');
    assertIgnored(run,'.release-clean.gpui-stage');
    isolateRelease(root);
  }
  (hooks.locks ?? assertDependencyLocks)(root,{ rustTestManifests: facts.manifests });
  const workRoot = safePath(root,'target/gpui-packaging',{ missing: true }); fs.mkdirSync(workRoot,{ recursive: true });
  const work = fs.mkdtempSync(path.join(workRoot,'build-'));
  const cargoDirectory = safePath(root,'target/gpui-packaging/cargo-cache',{ missing: true });
  const notesFile = path.join(work,'release_notes.rs');
  if (embedNotes) fs.writeFileSync(notesFile,renderReleaseNotesRust(notes),{ flag: 'wx' });
  const env = buildEnvironment(config,target,cargoDirectory,embedNotes,notesFile,environment);
  const unitTests = runWorkspaceTests(root,facts,run,env);
  assertSourceStable(root,run,identity);
  if (candidate && !isDeepStrictEqual((hooks.context ?? releasedContext)(root,facts),context)) throw new Error('release context changed during tests');
  const build = run('cargo',cargoArguments(['build','--release','--package',config.package,'--bin',config.binary,'--target',target,'--message-format','json-render-diagnostics'],lockPolicy),{ env });
  const executables = build.stdout.split(/\r?\n/u).filter(Boolean).flatMap(line => {
    let event; try { event = JSON.parse(line); } catch { return []; }
    return event.reason === 'compiler-artifact' && event.target?.name === config.binary && event.executable ? [event.executable] : [];
  });
  if (executables.length !== 1) throw new Error('Cargo did not report exactly one GUI executable');
  const binary = safePath(cargoDirectory,path.relative(cargoDirectory,executables[0]).split(path.sep).join('/'),{ type: 'file' });
  if (embedNotes && !containsBytes(binary,notes.bytes)) throw new Error('GUI binary lacks this release\'s verified update-log bytes; connect the GPUI build.rs/about-page contract');
  if (!isDeepStrictEqual(readConfig(root),config)) throw new Error('GPUI packaging config changed during build');
  fs.mkdirSync(path.join(work,'binaries'));
  const packagedBinary = path.join(work,'binaries',config.binary + (format === 'nsis' ? '.exe' : ''));
  fs.copyFileSync(binary,packagedBinary);
  if (format === 'nsis' && signing.selection === 'enabled') finalizeSigning({ run, config, signing, format, artifact: packagedBinary, app: null, env });
  const binarySha256 = sha256(packagedBinary);
  fs.mkdirSync(path.join(work,'resources'));
  for (const file of resources) fs.copyFileSync(safePath(root,file,{ type: 'file' }),path.join(work,'resources',file));
  const generated = packagerConfig({ root, work, config, facts, target, format, resources, signing });
  const packagerFile = path.join(work,'packager.json'); fs.writeFileSync(packagerFile,JSON.stringify(generated,null,2) + '\n',{ flag: 'wx' });
  // Finder 布局受打包超时限制，避免 CI 开关静默删除已要求的布局。
  const packageEnv = { ...env }; if (format === 'dmg') delete packageEnv.CI; else packageEnv.CI = 'true';
  assertSourceStable(root,run,identity);
  if (candidate && !isDeepStrictEqual((hooks.context ?? releasedContext)(root,facts),context)) throw new Error('release context changed before packaging');
  run(tool,['--config',packagerFile],{ env: packageEnv, timeout: 10 * 60 * 1000 });
  let artifact = packagePath(work,config,facts,target,format);
  const app = format === 'nsis' ? null : packagePath(work,config,facts,target,'app');
  if (app) {
    verifyMacIdentity(run,app,config,facts);
    verifyResources(path.join(app,'Contents','Resources'),root,resources);
    if (sha256(safePath(app,`Contents/MacOS/${config.binary}`,{ type: 'file' })) !== binarySha256) throw new Error('app bundle binary differs from this build');
  }
  const signingResult = finalizeSigning({ run, config, signing, format, artifact, app, env });
  let resourceVerification = format === 'nsis' ? 'Unverified' : 'app-bundle-byte-identical';
  if (format === 'dmg') resourceVerification = verifyDmg({ run, artifact, mount: path.join(work,'mount'), root, config, facts, resources, binarySha256 });
  if (candidate && format === 'nsis') resourceVerification = verifyNsis({ run, root, config, artifact, destination: path.join(work,'extracted'), resources, binarySha256 });
  const extension = format === 'app' ? 'app.zip' : format === 'dmg' ? 'dmg' : 'exe';
  const filename = `${facts.project.replaceAll('_','-')}-v${facts.version}-${info.platform}-${info.arch}.${extension}`;
  if (format === 'app') {
    const zip = path.join(work,filename); run('ditto',['-c','-k','--sequesterRsrc','--keepParent',artifact,zip]); artifact = zip;
    const extracted = path.join(work,'zip-resources'); fs.mkdirSync(extracted);
    run('ditto',['-x','-k',zip,extracted]);
    const extractedApp = safePath(extracted,`${config.productName}.app`,{ type: 'directory' });
    verifyMacIdentity(run,extractedApp,config,facts);
    verifyResources(path.join(extractedApp,'Contents','Resources'),root,resources);
    if (sha256(safePath(extractedApp,`Contents/MacOS/${config.binary}`,{ type: 'file' })) !== binarySha256) throw new Error('app archive binary differs from this build');
    resourceVerification = 'final-app-archive-byte-identical';
  }
  const digest = sha256(artifact);
  assertSourceStable(root,run,identity);
  if (!isDeepStrictEqual(readConfig(root),config)) throw new Error('GPUI packaging config changed during packaging');
  if (candidate && !isDeepStrictEqual((hooks.context ?? releasedContext)(root,facts),context)) throw new Error('release context changed before manifest');
  const output = candidate ? fs.mkdtempSync(path.join(root,'.release-clean.gpui-stage-')) : fs.mkdtempSync(path.join(workRoot,'local-'));
  fs.copyFileSync(artifact,path.join(output,filename));
  fs.writeFileSync(path.join(output,`${filename}.sha256`),`${digest}  ${filename}\n`,{ flag: 'wx' });
  const manifest = candidate ? { ...candidateManifest({ facts, config, context, target, targetInfo: info, format, artifact: filename, artifactSha256: digest, unitTests, selections, signing, signingResult, notes, resourceVerification, host: { platform, arch: hooks.arch ?? process.arch, release: os.release() }, buildRun }), binarySha256 }
    : { ...plan, artifact: filename, sha256: digest, unitTests, ...signingResult, resourceVerification, runtimeVerification: 'Unverified', distribution: 'local-test-only', sourceDirty: identity.dirty, sourceSnapshotSha256: identity.sourceSha256 };
  const manifestName = `${filename}.manifest.json`;
  fs.writeFileSync(path.join(output,manifestName),JSON.stringify(manifest,null,2) + '\n',{ flag: 'wx' });
  const files = [filename,`${filename}.sha256`,manifestName];
  exactFiles(output,files);
  if (sha256(path.join(output,filename)) !== digest) throw new Error('final artifact copy hash differs');
  if (candidate) publishDirectory(root,output,files);
  return { ...manifest, directory: candidate ? path.join(root,'release') : output, ...(candidate && selections.e2eSelection === 'enabled' ? { next: '$desktop-verify-delivery' } : {}) };
}

/** CLI 不提供测试宿主、假构建或伪证据选项。 */
export function main(argv = process.argv.slice(2)) {
  const { positionals, values } = parseArgs({ args: argv, allowPositionals: true, options: {
    root: { type: 'string', default: '.' }, mode: { type: 'string', default: 'local' }, target: { type: 'string' }, format: { type: 'string' },
    e2e: { type: 'string' }, 'e2e-reason': { type: 'string' }, 'e2e-risk': { type: 'string' }, signing: { type: 'string' }, 'signing-source': { type: 'string' }, help: { type: 'boolean', short: 'h' },
  } });
  if (values.help) { process.stdout.write('Usage: node build_gpui_release.mjs preview|setup|icons|build --root <project> --mode local|candidate [--target <native-triple>] [--format app|dmg|nsis] [--e2e enabled|disabled --e2e-reason <text> --e2e-risk <text>] [--signing disabled|enabled --signing-source not-requested|configured|requested|channel-required]\n'); return 0; }
  if (positionals.length !== 1 || !['preview','setup','icons','build'].includes(positionals[0])) throw new Error('expected one command: preview|setup|icons|build');
  const result = executeBuild({ ...values, command: positionals[0], e2eReason: values['e2e-reason'], e2eRisk: values['e2e-risk'], signingSource: values['signing-source'] });
  process.stdout.write(JSON.stringify(result,null,2) + '\n'); return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { assertNodeRuntime(); process.exitCode = main(); }
  catch (error) { process.stderr.write(`GPUI build: ${error.message}\n`); process.exitCode = 1; }
}
