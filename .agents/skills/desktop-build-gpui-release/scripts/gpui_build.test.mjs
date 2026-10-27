/** 用隔离 Git/真实文件验证流水线边界；外部编译/打包仅为单元替身，不作为产物验收。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { assertNodeRuntime, commandRunner, executeBuild, runWorkspaceTests, setupPackager } from './build_gpui_release.mjs';
import { renderDefaultConfig, projectFacts, readConfig } from './gpui_config.mjs';
import { relativePath, safePath, sha256, isolateRelease, publishDirectory } from './gpui_filesystem.mjs';
import { candidateSelection, containsBytes, renderReleaseNotesRust } from './gpui_evidence.mjs';
import { packagerConfig, signingChoice, finalizeSigning, verifyResources, verifyNsis } from './gpui_packager.mjs';
import { encodePng, decodePng, renderPlatformIcons, renderDmgBackground } from './gpui_icons.mjs';
import { collectResources, assertResourcesStable } from './gpui_resources.mjs';
import { localDmgSigningHook, signLocalDmgApp, verifyLocalBundle } from './gpui_macos_identity.mjs';

test('failed real subprocess retains captured test stdout alongside compiler stderr even in quiet mode', () => {
  const run = commandRunner(fs.realpathSync(os.tmpdir()));
  const args = ['-e', 'process.stdout.write("FAILED regression case"); process.stderr.write("error: test failed"); process.exitCode=1'];
  assert.throws(() => run(process.execPath,args), error => error.message.includes('FAILED regression case') && error.message.includes('error: test failed'));
  assert.throws(() => run(process.execPath,args,{quiet:true}), error => error.message.includes('FAILED regression case') && error.message.includes('error: test failed'));
});

/** 临时根使用 canonical 路径，避免 macOS /var 别名误入路径门禁。 */
function fixture(t, { systemNotification = 'disabled', withResources = false } = {}) {
  const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'gpui-build-test-'));
  t.after(() => fs.rmSync(root,{ recursive: true, force: true }));
  const config = renderDefaultConfig({ projectId: 'sample', nameZh: '测试应用', nameEn: 'Test App', owner: 'Owner' });
  if (withResources) config.resources = [{source:'assets/skins',macos:'skins',windows:'resources/skins'}];
  const files = {
    'Cargo.toml': '[workspace]\nmembers = ["sample_gui"]\n[workspace.metadata.agent-first-harness]\nproject-id = "sample"\ninterfaces = ["gui"]\ngui-framework = "gpui"\ntarget-platforms = ["macos", "windows"]\n',
    'sample_gui/Cargo.toml': '[package]\nname = "sample_gui"\nversion = "0.1.0"\nedition = "2024"\n',
    'sample_gui/src/main.rs': 'fn main() {}\n',
    'packaging/gpui.json': JSON.stringify(config),
    'packaging/icons/app.icns': 'icns-test', 'packaging/icons/app.ico': 'ico-test',
    'packaging/macos/background.png': 'background-test',
    'LICENSE.zh-CN.md': '测试应用 商业许可证', 'LICENSE.en.md': 'Test App Commercial License',
    '.gitignore': '/target/\n/release/\n/.release-clean.*\nCargo.lock\n',
    'docs/GUI_APP_PROFILE.md': `\`\`\`gui-initialization-config\nsystem_tray: disabled\nsystem_notification: ${systemNotification}\nautostart: disabled\nabout_page: enabled\nsponsor_page: disabled\nsingle_instance: disabled\ndeep_link: disabled\nglobal_shortcut: disabled\nsidebar_mode: detailed\n\`\`\`\n`,
    '.agents/skills/desktop-manage-version/scripts/version_gate.mjs': '// fixture version entry\n',
    'release-notes.json': JSON.stringify({ schemaVersion: 2, releases: [{ releaseDate: '2026-10-08', version: 'v0.1.0', featureOptimizations: [{ 'zh-CN': '新增 "日志"\n第二行', 'en-US': 'Added log\nSecond line' }], bugFixes: [] }] }),
    ...(withResources ? {'assets/skins/builtin/theme/manifest.toml':'name = "theme"','assets/skins/builtin/theme/preview.png':Buffer.from([0,1,127,255]),'assets/skins/generator/assets/template/theme.json':'{"tokens":{}}'} : {}),
  };
  for (const [relative,bytes] of Object.entries(files)) { const file = path.join(root,relative); fs.mkdirSync(path.dirname(file),{ recursive: true }); fs.writeFileSync(file,bytes); }
  for (const args of [['init','-b','main'],['config','user.name','Fixture'],['config','user.email','fixture@example.invalid'],['add','.'],['commit','-qm','fixture']]) {
    const result = spawnSync('git',args,{ cwd: root, encoding: 'utf8' }); assert.equal(result.status,0,result.stderr);
  }
  const tools = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'gpui-global-tools-test-'));
  t.after(() => fs.rmSync(tools,{recursive:true,force:true}));
  const tool = path.join(tools,'cargo-packager'); fs.writeFileSync(tool,'unit-test-tool',{mode:0o755});
  const metadata = { workspace_root: root, workspace_members: ['sample-id'], metadata: { 'agent-first-harness': { 'project-id': 'sample', interfaces: ['gui'], 'gui-framework': 'gpui', 'target-platforms': ['macos','windows'] } }, packages: [{ id: 'sample-id', name: 'sample_gui', version: '0.1.0', manifest_path: path.join(root,'sample_gui/Cargo.toml'), targets: [{ name: 'sample_gui', kind: ['bin'] }] }] };
  const commit = spawnSync('git',['rev-parse','HEAD'],{ cwd: root, encoding: 'utf8' }).stdout.trim();
  const review = { selection: 'disabled', status: 'Not run', reason: 'Fixture request', remainingRisk: 'Semantic review not selected.' };
  const context = { sourceCommit: commit, expectedTag: 'v0.1.0-20261008', releaseContextSha256: 'b'.repeat(64), releaseReview: review };
  return { root, config, metadata, tool, context };
}

/** 外部进程替身仍落真实文件，使路径、hash、原子集合与漂移断言可观察。 */
function fakeRun(f, overrides = {}) {
  const calls = [], appByZip = new Map();
  const run = (program,args,settings = {}) => {
    calls.push({ program,args,settings });
    const response = { stdout: '', stderr: '', status: 0 };
    if (overrides.before) overrides.before(program,args,settings);
    if (program === 'git') return spawnSync('git',args,{ cwd: f.root, encoding: 'utf8' });
    if (program === 'cargo' && args[0] === 'metadata') return { ...response, stdout: JSON.stringify(f.metadata) };
    if (program === 'cargo' && args.includes('--list')) return { ...response, stdout: overrides.zeroTests ? '0 tests\n' : 'sample::test: test\n1 test, 0 benchmarks\n' };
    if (program === 'cargo' && args[0] === 'test') { if (overrides.testFailure) throw new Error('cargo failed test'); return { ...response, stdout: 'test result: ok. 1 passed; 0 failed; 0 ignored;\n' }; }
    if (program === 'cargo' && args[0] === 'build') {
      const executable = path.join(settings.env.CARGO_TARGET_DIR,'aarch64-apple-darwin/release/sample_gui'); fs.mkdirSync(path.dirname(executable),{ recursive: true });
      const bytes = settings.env.HARNESS_GPUI_RELEASE_NOTES_RS ? fs.readFileSync(path.join(f.root,'release-notes.json')) : Buffer.alloc(0);
      fs.writeFileSync(executable,Buffer.concat([Buffer.from('unit-test-binary'),overrides.missingNotes ? Buffer.alloc(0) : bytes]));
      return { ...response, stdout: JSON.stringify({ reason: 'compiler-artifact',target: { name: 'sample_gui' },executable }) + '\n' };
    }
    if (program.endsWith('cargo-packager') && args[0] === '--version') return { ...response, stdout: 'cargo-packager 0.11.8\n' };
    if (program.endsWith('cargo-packager') && args[0] === '--config') {
      const config = JSON.parse(fs.readFileSync(args[1],'utf8')), app = path.join(config.outDir,`${config.productName}.app`);
      fs.mkdirSync(path.join(app,'Contents/Resources'),{ recursive: true }); fs.mkdirSync(path.join(app,'Contents/MacOS'),{ recursive: true });
      fs.writeFileSync(path.join(app,'Contents/Info.plist'),'fixture plist');
      fs.copyFileSync(path.join(config.binariesDir,'sample_gui'),path.join(app,'Contents/MacOS/sample_gui'));
      for (const file of config.resources) { const target=path.join(app,'Contents/Resources',file.target);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(file.src,target); }
      if (overrides.resourceTamper) fs.writeFileSync(path.join(app,'Contents/Resources/LICENSE.en.md'),'different');
      if (config.formats.includes('dmg')) {
        assert.ok(config.beforeEachPackageCommand, 'DMG must sign before sealing');
        const binary = path.join(app, 'Contents/MacOS/sample_gui');
        const work = path.dirname(args[1]);
        signLocalDmgApp(run, app, f.config.identifier, binary, sha256(binary), path.join(work, 'local-dmg-identity.json'), settings.env);
        fs.writeFileSync(path.join(config.outDir, `${config.productName}_0.1.0_aarch64.dmg`), 'unit-test-dmg');
      }
      return response;
    }
    if (program === 'ditto' && args.includes('-c')) { const source = args.at(-2), destination = args.at(-1); appByZip.set(destination,source); fs.writeFileSync(destination,'unit-test-archive'); return response; }
    if (program === 'ditto' && args.includes('-x')) { const source = appByZip.get(args.at(-2)); const target=path.join(args.at(-1),path.basename(source));fs.cpSync(source,target,{ recursive: true });if(overrides.finalResourceTamper)fs.writeFileSync(path.join(target,'Contents/Resources/skins/builtin/theme/preview.png'),'tampered');if(overrides.finalResourceRootLink){const resources=path.join(target,'Contents/Resources');fs.renameSync(resources,`${resources}.original`);fs.symlinkSync(`${resources}.original`,resources,'junction');}return response; }
    if (program === process.execPath) return response;
    if (program === 'plutil') return { ...response,stdout: ({ CFBundleIdentifier: f.config.identifier,CFBundleExecutable: f.config.binary,CFBundleShortVersionString: '0.1.0',LSUIElement: f.config.macos.agent ? 'true' : 'false' })[args[1]] + '\n' };
    if (program === '/usr/bin/codesign') {
      if (overrides.signingFailure) throw new Error('local signing failed');
      if (args[0] === '--force') fs.appendFileSync(path.join(args.at(-1), 'Contents/MacOS/sample_gui'), '-signed');
      return { ...response, stderr: `Identifier=${overrides.wrongIdentity ? 'foreign.app' : f.config.identifier}\nSignature=adhoc\nInfo.plist entries=14\nSealed Resources version=2\n` };
    }
    if (program === 'hdiutil') {
      if (args[0] === 'detach') return response;
      const mount = args.at(-2), work = path.dirname(mount), source = path.join(work, 'packages', `${f.config.productName}.app`);
      fs.cpSync(source, path.join(mount, path.basename(source)), { recursive: true });
      fs.writeFileSync(path.join(mount, '.DS_Store'), 'fixture layout');
      fs.mkdirSync(path.join(mount, '.background'));
      fs.copyFileSync(path.join(f.root, f.config.macos.background), path.join(mount, '.background', path.basename(f.config.macos.background)));
      fs.symlinkSync('/Applications', path.join(mount, 'Applications'));
      return response;
    }
    throw new Error(`unexpected test command ${program} ${args.join(' ')}`);
  };
  return { run,calls };
}

/** 流水线共用本机适配，测试无法经 CLI 打开宿主覆盖。 */
function hooks(f, fake) { return { run: fake.run, platform: 'darwin', arch: 'arm64', env: { PATH: path.dirname(f.tool), APPLE_SIGNING_IDENTITY: 'must-not-inherit' }, locks: () => {}, context: () => f.context }; }
function localOptions(root) { return { command: 'build', root, mode: 'local', target: 'aarch64-apple-darwin', format: 'app' }; }
function candidateOptions(root) { return { ...localOptions(root), mode: 'candidate', e2e: 'disabled', e2eReason: 'User selected no E2E', e2eRisk: 'Installation remains unverified' }; }

/** 真实嵌套字节贯穿暂存、app与最终归档核对，源码路径不冒充安装路径。 */
test('mapped nested resources survive the final application archive', t => {
  const f=fixture(t,{withResources:true}),fake=fakeRun(f);
  const output=executeBuild(localOptions(f.root),hooks(f,fake));
  assert.equal(output.bundledResources.length,5);
  const generated=JSON.parse(fs.readFileSync(fake.calls.find(call=>call.program===f.tool&&call.args[0]==='--config').args[1]));
  const preview=generated.resources.find(file=>file.target==='skins/builtin/theme/preview.png');
  assert.ok(preview);assert.deepEqual(fs.readFileSync(preview.src),Buffer.from([0,1,127,255]));
  const windows=collectResources(f.root,f.config,'windows',false);
  assert.ok(windows.some(file=>file.target==='resources/skins/builtin/theme/preview.png'));
  assert.ok(windows.some(file=>file.target==='LICENSE.en.md'));
  assert.throws(()=>executeBuild(localOptions(f.root),hooks(f,fakeRun(f,{finalResourceTamper:true}))),/bytes differ/);
  assert.throws(()=>executeBuild(localOptions(f.root),hooks(f,fakeRun(f,{finalResourceRootLink:true}))),/symbolic link/);
});

/** 缺失、符号链接、平台目标冲突和未知字段必须在编译前拒绝。 */
test('invalid resource mappings fail before compilation', t => {
  for (const mutation of [
    f=>{f.config.resources[0].source='absent';},
    f=>{fs.symlinkSync(f.root,path.join(f.root,'assets/skins/linked'),'junction');},
    f=>{f.config.resources.push({source:'LICENSE.en.md',macos:'SKINS/builtin/theme/preview.png',windows:'resources/skins/builtin/theme/preview.png'});},
    f=>{f.config.resources.push({source:'LICENSE.en.md',macos:'skins/builtin',windows:'resources/skins/builtin'});},
    f=>{f.config.resources[0].macos='LICENSE.en.md';},
    f=>{f.config.resources.push({source:'LICENSE.en.md',macos:'app.icns',windows:'extra.txt'});},
    f=>{f.config.resources[0].macos='unsafe/${name}';},
    f=>{f.config.resources[0].hook='forbidden';},
  ]) {
    const f=fixture(t,{withResources:true});mutation(f);fs.writeFileSync(path.join(f.root,'packaging/gpui.json'),JSON.stringify(f.config));
    const fake=fakeRun(f);assert.throws(()=>executeBuild(localOptions(f.root),hooks(f,fake)));
    assert.equal(fake.calls.some(call=>call.program==='cargo'&&call.args[0]==='build'),false);
    assert.equal(fs.existsSync(path.join(f.root,'release')),false);
  }
});

/** Git忽略的资源变化也不能绕过构建前后的清单和摘要复读。 */
test('ignored resource additions removals and byte changes invalidate the snapshot', t => {
  for (const mutation of [
    root=>fs.writeFileSync(path.join(root,'assets/skins/new.txt'),'new'),
    root=>fs.unlinkSync(path.join(root,'assets/skins/builtin/theme/preview.png')),
    root=>fs.writeFileSync(path.join(root,'assets/skins/builtin/theme/preview.png'),'changed'),
  ]) {
    const f=fixture(t,{withResources:true});
    spawnSync('git',['rm','-r','--cached','assets/skins'],{cwd:f.root});
    fs.appendFileSync(path.join(f.root,'.gitignore'),'/assets/skins/\n');
    spawnSync('git',['add','.gitignore'],{cwd:f.root});spawnSync('git',['commit','-qm','ignore fixture resources'],{cwd:f.root});
    const snapshot=collectResources(f.root,f.config,'macos',false);assertResourcesStable(f.root,f.config,'macos',false,snapshot);
    let changed=false;
    const fake=fakeRun(f,{before(program,args){if(!changed&&program==='cargo'&&args[0]==='test'&&!args.includes('--list')){changed=true;mutation(f.root);}}});
    assert.throws(()=>executeBuild(localOptions(f.root),hooks(f,fake)),/resources changed/);
    assert.equal(fake.calls.some(call=>call.program==='cargo'&&call.args[0]==='build'),false);
  }
});

/** NSIS最终解包的安装根和嵌套资源分别校验，篡改不能被源目录正确所掩盖。 */
test('final NSIS resources verify explicit Windows targets and reject nested tampering', t => {
  for (const tamper of [false,true]) for (const nestedBinary of ['different binary','binary']) {
    const f=fixture(t,{withResources:true});f.config.windows.extractor='7zr.exe';
    fs.writeFileSync(path.join(f.root,'assets/skins/generator/release-notes.json'),'nested generator log');
    fs.writeFileSync(path.join(f.root,'assets/skins/generator/sample_gui.exe'),nestedBinary);
    fs.writeFileSync(path.join(path.dirname(f.tool),'7zr.exe'),'test extractor',{mode:0o755});
    const resources=collectResources(f.root,f.config,'windows',true),binary=path.join(f.root,'binary.exe');fs.writeFileSync(binary,'binary');
    const destination=path.join(f.root,`extract-${tamper}`);
    const run=()=>{const install=path.join(destination,'$INSTDIR');fs.mkdirSync(install);for(const file of resources){const target=path.join(install,file.target);fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(f.root,file.source),target);}fs.copyFileSync(binary,path.join(install,'sample_gui.exe'));if(tamper)fs.writeFileSync(path.join(install,'resources/skins/builtin/theme/preview.png'),'changed');};
    const verify=()=>verifyNsis({run,root:f.root,config:f.config,artifact:'fixture.exe',destination,resources,binarySha256:sha256(binary),environment:{PATH:path.dirname(f.tool)}});
    if(tamper)assert.throws(verify,/bytes differ/);else assert.equal(verify(),'final-installer-byte-identical');
  }
});

test('Node runtime requires complete stable >=24.21.0 and accepts newer release lines', () => {
  for (const version of ['24.21.0','24.21.1','24.22.0','25.0.0','26.0.0']) assert.doesNotThrow(() => assertNodeRuntime(version));
  for (const version of ['22.99.99','23.99.99','24.20.99','24.21','24.21.0-beta.1','v24.21.0','024.21.0','24.021.0','24.21.00','unknown',null,24]) {
    assert.throws(() => assertNodeRuntime(version),/stable Node\.js >=24\.21\.0/u);
  }
});

test('strict configuration rejects duplicate, missing and unsafe fields', t => {
  const f = fixture(t); assert.equal(readConfig(f.root).package,'sample_gui');
  fs.writeFileSync(path.join(f.root,'packaging/gpui.json'),JSON.stringify(f.config).replace('"schemaVersion":1','"schemaVersion":1,"schemaVersion":1'));
  assert.throws(() => readConfig(f.root),/duplicate/);
  for (const unsafe of ['../x','a/../x','C:/x','a\\x','a//x','a/.','aux.txt','a/file.','x/${unsafe}']) {
    if (unsafe.includes('${')) continue;
    assert.throws(() => relativePath(unsafe),/unsafe/);
  }
  fs.writeFileSync(path.join(f.root,'packaging/gpui.json'),JSON.stringify({ ...f.config, nsis: { ...f.config.nsis, hook: 'bad' } }));
  assert.throws(() => readConfig(f.root),/fields/);
  fs.writeFileSync(path.join(f.root,'packaging/gpui.json'),JSON.stringify({ ...f.config, macos: { ...f.config.macos, agent: 'true' } }));
  assert.throws(() => readConfig(f.root),/macos.agent must be a boolean/);
});

test('Cargo facts reject Tauri, absent GUI, unsafe version and mismatched GUI member', t => {
  const f = fixture(t); assert.equal(projectFacts(f.root,f.metadata,f.config).guiRoot,'sample_gui');
  for (const mutation of [m => m.metadata['agent-first-harness']['gui-framework'] = 'tauri',m => m.metadata['agent-first-harness'].interfaces = ['cli'],m => m.packages[0].version = '0.100.0',m => m.metadata['agent-first-harness']['gui-root'] = 'packaging',m => m.metadata['agent-first-harness']['project-id'] = '../escape']) {
    const modified = structuredClone(f.metadata); mutation(modified); assert.throws(() => projectFacts(f.root,modified,f.config));
  }
});

test('project paths reject links and atomic publication refuses changed destinations', t => {
  const f = fixture(t), external = path.join(f.root,'external'); fs.mkdirSync(external); fs.symlinkSync(external,path.join(f.root,'linked'),'junction');
  assert.throws(() => safePath(f.root,'linked/file',{ missing: true }),/link/);
  fs.symlinkSync(external,path.join(f.root,'release'),'junction'); assert.throws(() => isolateRelease(f.root),/link/); fs.unlinkSync(path.join(f.root,'release'));
  isolateRelease(f.root); const stage = fs.mkdtempSync(path.join(f.root,'.release-clean.stage-')); fs.writeFileSync(path.join(stage,'artifact'),'bytes');
  fs.writeFileSync(path.join(f.root,'release','unexpected'),'do not overwrite'); assert.throws(() => publishDirectory(f.root,stage,['artifact']),/changed/);
  assert.equal(fs.readFileSync(path.join(f.root,'release/unexpected'),'utf8'),'do not overwrite');
});

test('local build runs complete nonempty suites and writes no release directory', t => {
  const f = fixture(t), fake = fakeRun(f); fs.writeFileSync(path.join(f.root,'sample_gui/src/main.rs'),'fn main() { /* local dirty */ }\n');
  const output = executeBuild(localOptions(f.root),hooks(f,fake));
  assert.equal(output.distribution,'local-test-only'); assert.equal(output.sourceDirty,true); assert.equal(output.unitTests.passed,1);
  assert.equal(output.runtimeVerification,'Unverified'); assert.equal(output.resourceVerification,'final-app-archive-byte-identical'); assert.equal(fs.existsSync(path.join(f.root,'release')),false);
  assert.equal(sha256(path.join(output.directory,output.artifact)),output.sha256);
  const testCall = fake.calls.find(item => item.program === 'cargo' && item.args[0] === 'test' && !item.args.includes('--list'));
  assert.ok(['--workspace','--all-targets','--all-features'].every(item => testCall.args.includes(item)));
  assert.equal(testCall.settings.env.APPLE_SIGNING_IDENTITY,undefined); assert.equal(testCall.settings.env.HARNESS_GPUI_RELEASE_NOTES_RS,undefined);
});

test('zero/failing tests, tampered package resources and absent release bytes block before publication', t => {
  for (const setting of [{ zeroTests: true },{ testFailure: true },{ resourceTamper: true },{ missingNotes: true }]) {
    const f = fixture(t), fake = fakeRun(f,setting);
    assert.throws(() => executeBuild(setting.missingNotes ? candidateOptions(f.root) : localOptions(f.root),hooks(f,fake)),/zero|failed|differ|lacks/);
    assert.ok(!fs.existsSync(path.join(f.root,'release')) || fs.readdirSync(path.join(f.root,'release')).length === 0);
  }
});

test('Linux candidate checks published context and clean source before compiler probes or release cleanup', t => {
  for(const dirty of [false,true]) {
    const f=fixture(t), fake=fakeRun(f);
    f.metadata.metadata['agent-first-harness']['target-platforms']=['linux'];
    const release=path.join(f.root,'release'); fs.mkdirSync(release);
    fs.writeFileSync(path.join(release,'previous'),'preserve previous candidate');
    if(dirty) fs.appendFileSync(path.join(f.root,'Cargo.toml'),'# source drift\n');
    const run=(program,args,settings)=>program==='rustc'?{stdout:'host: x86_64-unknown-linux-gnu\n'}:fake.run(program,args,settings);
    const context=()=>{if(!dirty)throw new Error('published context rejected');return f.context;};
    assert.throws(()=>executeBuild({...candidateOptions(f.root),target:'x86_64-unknown-linux-gnu',format:'deb'}, {...hooks(f,fake),run,platform:'linux',context}),dirty?/source must be clean/:/published context rejected/);
    assert.ok(fake.calls.every(({program,args})=>program==='git'||program==='cargo'&&args[0]==='metadata'));
    assert.equal(fs.existsSync(path.join(f.root,'target')),false);
    assert.equal(fs.readFileSync(path.join(release,'previous'),'utf8'),'preserve previous candidate');
  }
});

test('candidate manifest is pending and preserves release review plus explicit E2E risk', t => {
  const f = fixture(t), fake = fakeRun(f); const output = executeBuild(candidateOptions(f.root),hooks(f,fake));
  assert.equal(output.milestoneAcceptance,'pending'); assert.equal(output.e2eStatus,'Not run'); assert.equal(output.reviewStatus,'Not run'); assert.equal(output.reviewReason,f.context.releaseReview.reason);
  assert.match(output.buildRun,/^[0-9a-f-]{36}$/u); assert.equal(output.interface,'gui'); assert.equal(output.bundleFormat,'app'); assert.equal(output.artifactKind,'application-archive');
  assert.equal(output.releaseNotesVersion,'v0.1.0'); assert.equal(output.releaseNotesResourceVerification,'byte-identical'); assert.equal(output.sourceCommit,f.context.sourceCommit);
  assert.equal(output.macosSigningSelection,'disabled'); assert.equal(output.macosSigningSource,'not-requested'); assert.equal('notarizationEvidence' in output,false); assert.equal('reviewEvidence' in output,false);
  assert.deepEqual(fs.readdirSync(output.directory).sort(),[output.artifact,`${output.artifact}.manifest.json`,`${output.artifact}.sha256`].sort());
  assert.ok(fake.calls.find(item => item.program === 'cargo' && item.args[0] === 'build').settings.env.HARNESS_GPUI_RELEASE_NOTES_RS);
});

test('candidate requires fresh E2E choice and incomplete signing fails before tests', t => {
  const f = fixture(t), fake = fakeRun(f);
  assert.throws(() => executeBuild({ ...candidateOptions(f.root),e2e: undefined },hooks(f,fake)),/explicit/);
  assert.throws(() => executeBuild({ ...candidateOptions(f.root),signing: 'enabled',signingSource: 'requested' },hooks(f,fake)),/approved Developer ID/);
  assert.equal(fake.calls.some(item => item.program === 'cargo' && item.args[0] === 'test'),false);
  assert.throws(() => candidateSelection('disabled','',''),/reason/); assert.throws(() => signingChoice('enabled','not-requested'),/invalid/);
});

/** 通知要求签名的 macOS 候选在任何执行器或候选目录副作用前拒绝，E2E 选择不能绕过。 */
test('macos_notification_candidates_reject_unsigned_before_commands_and_preserve_existing_release', t => {
  for (const format of ['app','dmg']) for (const e2e of ['enabled','disabled']) for (const explicit of [false,true]) {
    const f = fixture(t,{ systemNotification: 'enabled' }), fake = fakeRun(f);
    const release = path.join(f.root,'release'); fs.mkdirSync(release);
    const previous = { 'previous.dmg': Buffer.from([0,1,2,255]), 'previous.dmg.manifest.json': Buffer.from('{"milestoneAcceptance":"pending"}\n'), 'previous.dmg.sha256': Buffer.from('previous checksum\n') };
    for (const [file,bytes] of Object.entries(previous)) fs.writeFileSync(path.join(release,file),bytes);
    const rootEntries = fs.readdirSync(f.root).sort(), releaseInode = fs.statSync(release).ino;
    const options = { ...candidateOptions(f.root),format,e2e,...(explicit ? { signing: 'disabled',signingSource: 'not-requested' } : {}) };
    assert.throws(() => executeBuild(options,hooks(f,fake)),/system_notification.*enabled.*signing/u);
    assert.ok(fake.calls.every(item => item.program === 'git' || (item.program === 'cargo' && item.args[0] === 'metadata')));
    assert.deepEqual(fs.readdirSync(f.root).sort(),rootEntries);
    assert.equal(fs.statSync(release).ino,releaseInode);
    assert.deepEqual(fs.readdirSync(release).sort(),Object.keys(previous).sort());
    for (const [file,bytes] of Object.entries(previous)) assert.deepEqual(fs.readFileSync(path.join(release,file)),bytes);
    assert.equal(fs.existsSync(path.join(f.root,'target/gpui-packaging')),false);
  }
});

/** 缺失、无效、重复或未闭合的通知配置不能被当成 disabled 绕过签名门禁。 */
test('macos_candidate_notification_profile_ambiguity_fails_before_tests', t => {
  const mutations = [
    source => source.replace('system_notification: enabled\n',''),
    source => source.replace('system_notification: enabled','system_notification: pending'),
    source => source.replace('system_notification: enabled','system_notification: enabled\n  system_notification : disabled'),
    source => source + source,
    source => source.slice(0,source.lastIndexOf('```')),
  ];
  for (const mutate of mutations) {
    const f = fixture(t,{ systemNotification: 'enabled' }), fake = fakeRun(f);
    const profile = path.join(f.root,'docs/GUI_APP_PROFILE.md'); fs.writeFileSync(profile,mutate(fs.readFileSync(profile,'utf8')));
    assert.throws(() => executeBuild(candidateOptions(f.root),hooks(f,fake)),/GUI profile/u);
    assert.ok(fake.calls.every(item => item.program === 'git' || (item.program === 'cargo' && item.args[0] === 'metadata')));
    assert.equal(fs.existsSync(path.join(f.root,'release')),false);
  }
});

/** 显式签名缺少批准身份仍失败关闭，不能把通知要求转换成假签名或 unsigned 回退。 */
test('macos_notification_candidate_enabled_signing_still_fails_closed', t => {
  const f = fixture(t,{ systemNotification: 'enabled' }), fake = fakeRun(f);
  assert.throws(() => executeBuild({ ...candidateOptions(f.root),signing: 'enabled',signingSource: 'requested' },hooks(f,fake)),/approved Developer ID/u);
  assert.ok(fake.calls.every(item => item.program === 'git' || (item.program === 'cargo' && item.args[0] === 'metadata')));
  assert.equal(fs.existsSync(path.join(f.root,'release')),false);
});

/** 本机试包绑定完整本地身份，但不把本地签名当成通知运行或候选验收。 */
test('local_macos_notification_package_binds_adhoc_identity_and_remains_unverified', t => {
  const f = fixture(t,{ systemNotification: 'enabled' }), fake = fakeRun(f);
  f.config.macos.agent = true;
  fs.writeFileSync(path.join(f.root, 'packaging/gpui.json'), JSON.stringify(f.config));
  const output = executeBuild(localOptions(f.root),hooks(f,fake));
  assert.equal(output.distribution,'local-test-only'); assert.equal(output.signingStatus,'ad-hoc');
  assert.equal(output.runtimeVerification,'Unverified'); assert.equal(output.unitTests.passed,1);
  assert.equal(fs.existsSync(path.join(f.root,'release')),false);
  const calls = fake.calls.filter(call => call.program === '/usr/bin/codesign');
  assert.equal(calls.filter(call => call.args[0] === '--force').length, 1);
  assert.ok(calls.some(call => call.args[0] === '--verify' && call.args.at(-1).includes('zip-resources')));
  const generated = JSON.parse(fs.readFileSync(fake.calls.find(call => call.program === f.tool && call.args[0] === '--config').args[1]));
  assert.match(fs.readFileSync(generated.macos.infoPlistPath, 'utf8'), /<key>LSUIElement<\/key><true\/>/);
  for (const overrides of [{ signingFailure: true }, { wrongIdentity: true }]) {
    assert.throws(() => executeBuild(localOptions(f.root), hooks(f, fakeRun(f, overrides))), /signing|identity/);
  }
});

/** DMG 封盘前签名产生新字节，封盘后的挂载校验只能验证，不能补签旁边 App。 */
test('local_dmg_signs_before_sealing_and_verifies_the_mounted_bundle', t => {
  const f = fixture(t), fake = fakeRun(f);
  const output = executeBuild({ ...localOptions(f.root), format: 'dmg' }, hooks(f, fake));
  assert.equal(output.signingStatus, 'ad-hoc');
  assert.equal(output.resourceVerification, 'final-volume-byte-identical');
  const sign = fake.calls.findIndex(call => call.program === '/usr/bin/codesign' && call.args[0] === '--force');
  const mount = fake.calls.findIndex(call => call.program === 'hdiutil' && call.args[0] === 'attach');
  assert.ok(sign >= 0 && sign < mount);
  assert.equal(fake.calls.filter(call => call.program === '/usr/bin/codesign' && call.args[0] === '--force').length, 1);
  assert.ok(fake.calls.some(call => call.program === '/usr/bin/codesign' && call.args[0] === '--verify' && call.args.at(-1).includes('/mount/')));
});

/** 缺少 plist/资源封印或身份不符都不能通过；签名前篡改也不得被重新签名掩盖。 */
test('local_bundle_identity_rejects_unbound_signature_and_presigning_tamper', t => {
  const f = fixture(t), good = `Identifier=${f.config.identifier}\nSignature=adhoc\nInfo.plist entries=14\nSealed Resources version=2\n`;
  for (const details of [good.replace('Info.plist entries=14', 'Info.plist=not bound'), good.replace('Sealed Resources version=2', 'Sealed Resources=none'), good.replace('Signature=adhoc', 'Signature=none')]) {
    assert.throws(() => verifyLocalBundle(() => ({ stdout: '', stderr: details }), f.root, f.config.identifier, {}), /bound ad-hoc identity/);
  }
  const binary = path.join(f.root, 'binary'); fs.writeFileSync(binary, 'before');
  assert.throws(() => signLocalDmgApp(() => assert.fail('tampered binary must not be signed'), f.root, f.config.identifier, binary, '0'.repeat(64), path.join(f.root, 'receipt.json'), {}), /differs before/);
});

/** 真实codesign只处理临时系统工具副本，验证生成hook和特殊路径，不启动应用或操作通知权限。 */
test('generated_dmg_hook_signs_a_complete_temporary_bundle_on_macos', { skip: process.platform !== 'darwin' }, t => {
  const f = fixture(t), work = path.join(f.root, "签名 ' $() space"), app = path.join(work, 'Sample.app');
  fs.mkdirSync(path.join(app, 'Contents/MacOS'), { recursive: true });
  fs.mkdirSync(path.join(app, 'Contents/Resources'));
  const binary = path.join(app, 'Contents/MacOS/sample_gui');
  fs.copyFileSync('/usr/bin/true', binary);
  fs.writeFileSync(path.join(app, 'Contents/Resources/resource.txt'), 'sealed fixture');
  fs.writeFileSync(path.join(app, 'Contents/Info.plist'), `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${f.config.identifier}</string><key>CFBundleExecutable</key><string>sample_gui</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string></dict></plist>`);
  const before = sha256(binary), script = localDmgSigningHook(work, app, f.config.identifier, 'sample_gui', before);
  const env = { ...process.env, CARGO_PACKAGER_FORMAT: 'app' };
  for (const key of Object.keys(env)) if (key.startsWith('APPLE_')) delete env[key];
  const run = commandRunner(f.root);
  run('/bin/sh', ['-c', script], { env });
  assert.equal(fs.existsSync(path.join(work, 'local-dmg-identity.json')), false);
  run('/bin/sh', ['-c', script], { env: { ...env, CARGO_PACKAGER_FORMAT: 'dmg' } });
  const receipt = JSON.parse(fs.readFileSync(path.join(work, 'local-dmg-identity.json')));
  assert.equal(receipt.before, before); assert.equal(receipt.after, sha256(binary));
  verifyLocalBundle(run, app, f.config.identifier, env);
  fs.appendFileSync(path.join(app, 'Contents/Resources/resource.txt'), 'tampered');
  assert.throws(() => verifyLocalBundle(run, app, f.config.identifier, env), /codesign failed/);
});

test('release-context drift after test execution prevents a candidate', t => {
  const f = fixture(t), fake = fakeRun(f); let reads = 0;
  assert.throws(() => executeBuild(candidateOptions(f.root),{ ...hooks(f,fake),context: () => ({ ...f.context,releaseContextSha256: (++reads > 1 ? 'c' : 'b').repeat(64) }) }),/changed/);
  assert.equal(fake.calls.some(item => item.program === 'cargo' && item.args[0] === 'build'),false);
});

test('tracked policy passes locked to full metadata/list/run commands', t => {
  const f = fixture(t); const facts = projectFacts(f.root,f.metadata,f.config); facts.lockPolicy = 'tracked';
  const fake = fakeRun(f); runWorkspaceTests(f.root,facts,fake.run,{});
  for (const call of fake.calls.filter(item => item.program === 'cargo')) assert.ok(call.args.includes('--locked'));
  const listing = fake.calls.find(item => item.args.includes('--list')); assert.ok(listing.args.indexOf('--locked') < listing.args.indexOf('--'));
});

test('tool setup reuses exact global version and refuses replacement', t => {
  const f = fixture(t), calls = [];
  assert.equal(setupPackager(f.root,(program,args) => { calls.push({ program,args }); return { stdout: 'cargo-packager 0.11.8\n',status: 0 }; },'darwin',{PATH:path.dirname(f.tool)}),f.tool);
  assert.equal(calls.filter(item => item.program !== 'git').length,1); assert.throws(() => setupPackager(f.root,() => ({ stdout: 'cargo-packager 9.0.0\n',status: 0 }),'darwin',{PATH:path.dirname(f.tool)}),/different version/);
});

test('release-note Rust generation escapes text and binary search covers chunk boundary', t => {
  const f = fixture(t); const bytes = Buffer.from('log-bytes'); const file = path.join(f.root,'binary'); fs.writeFileSync(file,Buffer.concat([Buffer.alloc(1024 * 1024 - 3),bytes]));
  assert.equal(containsBytes(file,bytes),true); assert.equal(containsBytes(file,Buffer.from('missing')),false);
  const rust = renderReleaseNotesRust({ document: JSON.parse(fs.readFileSync(path.join(f.root,'release-notes.json'),'utf8')),bytes });
  assert.ok(rust.includes('HARNESS') === false); assert.ok(rust.includes('\\u{a}')); assert.ok(rust.includes('\\"日志\\"')); assert.ok(rust.includes('("2026-10-08", "v0.1.0"'));
});

test('PNG conversion creates valid multi-size ICNS/ICO and rejects corrupt sources', () => {
  const pixels = Buffer.alloc(256 * 256 * 4,255), source = encodePng(256,256,pixels), icons = renderPlatformIcons(source);
  assert.equal(decodePng(source).width,256); assert.equal(icons.macos.toString('ascii',0,4),'icns'); assert.equal(icons.macos.readUInt32BE(4),icons.macos.length);
  assert.equal(icons.windows.readUInt16LE(2),1); assert.equal(icons.windows.readUInt16LE(4),5); assert.deepEqual(renderPlatformIcons(source),icons);
  const corrupt = Buffer.from(source); corrupt[40] ^= 1; assert.throws(() => renderPlatformIcons(corrupt),/checksum/);
  const background = renderDmgBackground(); assert.equal(background.readUInt32BE(16),660); assert.equal(background.readUInt32BE(20),440);
});

test('unsigned packager config has no signing hook and Windows signing failure cannot downgrade', t => {
  const f = fixture(t), facts = projectFacts(f.root,f.metadata,f.config), signing = signingChoice('disabled');
  const generated = packagerConfig({ root: f.root,work: f.root,config: f.config,facts,target: 'aarch64-apple-darwin',format: 'app',resources: [{source:'LICENSE.en.md',target:'LICENSE.en.md',sha256:sha256(path.join(f.root,'LICENSE.en.md'))}],signing });
  assert.equal('signingIdentity' in generated.macos,false); assert.equal('beforePackagingCommand' in generated,false);
  const dmg = packagerConfig({ root: f.root,work: f.root,config: f.config,facts,target: 'aarch64-apple-darwin',format: 'dmg',resources: [{source:'LICENSE.en.md',target:'LICENSE.en.md',sha256:sha256(path.join(f.root,'LICENSE.en.md'))}],signing });
  assert.deepEqual(dmg.dmg.appFolderPosition,{ x: 480,y: 220 }); assert.equal('applicationFolderPosition' in dmg.dmg,false);
  const nsis = packagerConfig({ root: f.root,work: f.root,config: f.config,facts,target: 'x86_64-pc-windows-msvc',format: 'nsis',resources: [{ source: 'source-only/license.txt', target: 'resources/windows/nested/license.txt' }],signing });
  assert.equal(nsis.productName, f.config.productName);
  assert.match(fs.readFileSync(nsis.nsis.template,'utf8'), /!define SHORTCUTNAME "Test App"/u);
  const uninstall = fs.readFileSync(nsis.nsis.template,'utf8').split('Section Uninstall\n')[1];
  assert.ok(uninstall.includes('  RMDir "$INSTDIR\\resources\\windows\\nested"'));
  assert.equal(uninstall.includes('source-only'), false);
  const configured = { ...f.config,windows: { signing: { certificateThumbprint: '1'.repeat(40),timestampUrl: 'https://example.invalid' },extractor: null } };
  assert.throws(() => finalizeSigning({ run: () => { throw new Error('signing failed'); },config: configured,signing: signingChoice('enabled','configured'),format: 'nsis',artifact: 'installer.exe',env: {} }),/signing failed/);
  assert.throws(() => verifyNsis({ run: () => {},root: f.root,config: f.config,artifact: 'installer.exe',destination: path.join(f.root,'extract'),resources: [] }),/global/);
});

test('resource verification compares final bytes and rejects linked resources', t => {
  const f = fixture(t), copied = path.join(f.root,'copied'); fs.mkdirSync(copied); fs.copyFileSync(path.join(f.root,'LICENSE.en.md'),path.join(copied,'LICENSE.en.md'));
  assert.equal(verifyResources(copied,f.root,[{source:'LICENSE.en.md',target:'LICENSE.en.md',sha256:sha256(path.join(f.root,'LICENSE.en.md'))}]),'byte-identical'); fs.renameSync(copied,`${copied}.original`); fs.symlinkSync(`${copied}.original`,copied,'junction');
  assert.throws(() => verifyResources(copied,f.root,[{source:'LICENSE.en.md',target:'LICENSE.en.md',sha256:sha256(path.join(f.root,'LICENSE.en.md'))}]),/link/);
});

test('nonignored local build directory fails before writing', t => {
  const f = fixture(t), fake = fakeRun(f);
  fs.writeFileSync(path.join(f.root,'.gitignore'),'release/\n.release-clean.*\n');
  assert.throws(() => executeBuild(localOptions(f.root),hooks(f,fake)),/Git-ignored/);
  assert.equal(fs.existsSync(path.join(f.root,'target/gpui-packaging')),false);
});

test('local initial dirty bytes are allowed but drift during tests blocks', t => {
  const f = fixture(t); const file = path.join(f.root,'sample_gui/src/main.rs'); fs.writeFileSync(file,'fn main() { /* first dirty bytes */ }\n');
  const fake = fakeRun(f,{ before: (program,args) => { if (program === 'cargo' && args[0] === 'test' && !args.includes('--list')) fs.writeFileSync(file,'fn main() { /* different dirty bytes */ }\n'); } });
  assert.throws(() => executeBuild(localOptions(f.root),hooks(f,fake)),/worktree bytes changed/);
  assert.equal(fake.calls.some(item => item.program === 'cargo' && item.args[0] === 'build'),false);
});

test('about-disabled candidate keeps packaged log without requiring UI or embedded log', t => {
  const f = fixture(t), profile = path.join(f.root,'docs/GUI_APP_PROFILE.md');
  fs.writeFileSync(profile,fs.readFileSync(profile,'utf8').replace('about_page: enabled','about_page: disabled'));
  spawnSync('git',['add','.'],{ cwd: f.root }); spawnSync('git',['commit','-qm','test: disable about page'],{ cwd: f.root });
  const fake = fakeRun(f,{ missingNotes: true }); const output = executeBuild(candidateOptions(f.root),hooks(f,fake));
  assert.equal(output.milestoneAcceptance,'pending'); assert.equal(output.releaseNotesResourceVerification,'byte-identical');
  assert.equal(fake.calls.find(item => item.program === 'cargo' && item.args[0] === 'build').settings.env.HARNESS_GPUI_RELEASE_NOTES_RS,undefined);
});

/** 设置页日志入口不依赖独立关于页，缺少二进制真实日志字节仍须阻断。 */
test('settings release-notes UI embeds and verifies logs with the about page disabled', t => {
  for (const missingNotes of [false,true]) {
    const f=fixture(t),profile=path.join(f.root,'docs/GUI_APP_PROFILE.md');
    fs.writeFileSync(profile,fs.readFileSync(profile,'utf8').replace('about_page: enabled','about_page: disabled'));
    f.config.releaseNotesUi=true;
    fs.writeFileSync(path.join(f.root,'packaging/gpui.json'),JSON.stringify(f.config));
    spawnSync('git',['add','.'],{cwd:f.root});spawnSync('git',['commit','-qm','test: settings release notes'],{cwd:f.root});
    const fake=fakeRun(f,{missingNotes});
    if(missingNotes) assert.throws(()=>executeBuild(candidateOptions(f.root),hooks(f,fake)),/lacks.*update-log bytes/);
    else {
      const output=executeBuild(candidateOptions(f.root),hooks(f,fake));
      assert.equal(output.releaseNotesResourceVerification,'byte-identical');
    }
    assert.ok(fake.calls.find(item=>item.program==='cargo'&&item.args[0]==='build').settings.env.HARNESS_GPUI_RELEASE_NOTES_RS);
  }
  const f=fixture(t);
  fs.writeFileSync(path.join(f.root,'packaging/gpui.json'),JSON.stringify({...f.config,releaseNotesUi:'true'}));
  assert.throws(()=>readConfig(f.root),/must be a boolean/);
});
