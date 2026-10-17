/** Linux同系统双架构构建；宿主测试、交叉编译和目标运行验收分别记证。 */
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {randomUUID} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import {safeRoot,safePath,sha256,exactFiles,isolateRelease,publishDirectory} from './gpui_filesystem.mjs';
import {readConfig,projectFacts,TARGETS} from './gpui_config.mjs';
import {collectResources,assertResourcesStable} from './gpui_resources.mjs';
import {releasedContext,candidateSelection,releaseNotes,aboutPageEnabled,renderReleaseNotesRust,containsBytes,candidateManifest} from './gpui_evidence.mjs';
import {gitIdentity,assertSourceStable,assertIgnored,cargoArguments,runWorkspaceTests} from './build_gpui_release.mjs';
import {readDependencyLockPolicy,assertDependencyLocks} from '../../desktop-implement-change/scripts/project_lock_policy.mjs';
import {makeLinuxPackage,verifyElf} from './gpui_linux_package.mjs';

/** 两种GNU目标使用显式目标编译器、头文件和库路径；不接受宿主库代替交叉库。 */
export function linuxEnvironment(target, host, environment=process.env) {
  if (TARGETS[target]?.platform !== 'linux' || TARGETS[host]?.platform !== 'linux') throw new Error('Linux GNU x64/ARM64 host and target required');
  const tuple=target.replace('-unknown',''), key=target.replaceAll('-','_');
  const env={...environment};
  for (const name of Object.keys(env)) if (name.startsWith('PKG_CONFIG') || name==='HARNESS_GPUI_RELEASE_NOTES_RS' || name==='CARGO_BUILD_TARGET') delete env[name];
  return {...env, CARGO_BUILD_TARGET:target, CARGO_BUILD_JOBS:environment.CARGO_BUILD_JOBS ?? '1',
    [`CARGO_TARGET_${key.toUpperCase()}_LINKER`]:`${tuple}-gcc`, [`CC_${key}`]:`${tuple}-gcc`, [`CXX_${key}`]:`${tuple}-g++`,
    [`AR_${key}`]:`${tuple}-ar`, PKG_CONFIG_ALLOW_CROSS:'1', PKG_CONFIG_SYSROOT_DIR:'/', PKG_CONFIG_PATH:'',
    PKG_CONFIG_LIBDIR:`/usr/lib/${tuple}/pkgconfig:/usr/share/pkgconfig`};
}

/** 只读复探已请求的Linux工具链；安装需要独立的真实sudo交互入口。 */
export function checkLinuxToolchain(run,target,host,environment=process.env) {
  const env=linuxEnvironment(target,host,environment), tuple=target.replace('-unknown','');
  const installed=run('rustup',['target','list','--installed']).stdout.split(/\r?\n/u);
  if (!installed.includes(target)) throw new Error(`missing Rust target: rustup target add ${target}`);
  if (run(`${tuple}-gcc`,['-dumpmachine']).stdout.trim() !== tuple) throw new Error('Linux C compiler target differs');
  run(`${tuple}-g++`,['--version']); run(`${tuple}-ar`,['--version']);
  const tools={gcc:run(`${tuple}-gcc`,['--version']).stdout.split('\n')[0], dpkg:run('dpkg-deb',['--version']).stdout.split('\n')[0], tar:run('tar',['--version']).stdout.split('\n')[0]};
  run('readelf',['--version']);
  const libraries={};
  for (const library of ['fontconfig','freetype2','xkbcommon','xkbcommon-x11','wayland-client','x11','x11-xcb','xcb']) {
    libraries[library]=run('pkg-config',['--modversion',library],{env}).stdout.trim();
    const directory=run('pkg-config',['--variable=libdir',library],{env}).stdout.trim();
    if (path.resolve(directory)!==`/usr/lib/${tuple}`) throw new Error(`pkg-config resolved another architecture for ${library}: ${directory}`);
  }
  return {env, evidence:{target,host,buildMode:target===host?'native':'cross-architecture',tools,libraries,status:'passed'}};
}

/** 实际链接目标C++及全部图形库，只检查ELF而绝不执行异架构探针。 */
export function probeLinuxLinker(root,run,target,env) {
  assertIgnored(run,'target/gpui-packaging/.gpui-private');
  const parent=safePath(root,'target/gpui-packaging',{missing:true}); fs.mkdirSync(parent,{recursive:true});
  const work=fs.mkdtempSync(path.join(parent,'toolchain-')), source=path.join(work,'probe.cc'), binary=path.join(work,'probe');
  fs.writeFileSync(source,`#include <string>\n#include <fontconfig/fontconfig.h>\n#include <ft2build.h>\n#include FT_FREETYPE_H\n#include <xkbcommon/xkbcommon.h>\n#include <xkbcommon/xkbcommon-x11.h>\n#include <wayland-client.h>\n#include <X11/Xlib.h>\n#include <X11/Xlib-xcb.h>\n#include <xcb/xcb.h>\nint main() { volatile auto a=&FcInit; volatile auto b=&FT_Init_FreeType; volatile auto c=&xkb_context_new; volatile auto d=&xkb_x11_get_core_keyboard_device_id; volatile auto e=&wl_display_connect; volatile auto f=&XOpenDisplay; volatile auto g=&XGetXCBConnection; volatile auto h=&xcb_connect; return std::string("toolchain").empty() || !(a && b && c && d && e && f && g && h); }\n`,{flag:'wx'});
  const flags=run('pkg-config',['--cflags','--libs','fontconfig','freetype2','xkbcommon','xkbcommon-x11','wayland-client','x11','x11-xcb','xcb'],{env}).stdout.trim();
  if (/[\\'"\r\n]/u.test(flags)) throw new Error('pkg-config flags require unsupported shell escaping');
  run(`${target.replace('-unknown','')}-g++`,[source,'-o',binary,...flags.split(/\s+/u).filter(Boolean)],{env});
  verifyElf(binary,TARGETS[target]);
  return {status:'linked-and-elf-verified',target,binary,sha256:sha256(binary),runtimeVerification:'Not run (compile/link probe only)'};
}

/** Linux沿用默认主分支/日志/测试/资源门禁，local不写release或发布元数据。 */
export function executeLinuxBuild(options,hooks) {
  const root=safeRoot(options.root), run=hooks.run, environment=hooks.env ?? process.env;
  const identity=gitIdentity(root,run), config=readConfig(root), lockPolicy=readDependencyLockPolicy(root);
  const metadata=JSON.parse(run('cargo',cargoArguments(['metadata','--no-deps','--format-version','1'],lockPolicy)).stdout);
  const facts=projectFacts(root,metadata,config), host=run('rustc',['-vV']).stdout.match(/^host: (.+)$/mu)?.[1];
  const target=options.target ?? host, info=TARGETS[target], format=options.format ?? 'deb';
  if (info?.platform!=='linux' || !info.formats.includes(format) || !facts.platforms.includes('linux') || TARGETS[host]?.platform!=='linux') throw new Error('unselected Linux GNU x64/ARM64 target or unsupported format');
  if (!['local','candidate'].includes(options.mode)) throw new Error('--mode must be local or candidate');
  if ((options.signing ?? 'disabled')!=='disabled' || (options.signingSource ?? 'not-requested')!=='not-requested') throw new Error('Linux package signing is not implemented; cannot silently downgrade');
  const candidate=options.mode==='candidate', buildRun=randomUUID();
  const plan={buildRun,mode:options.mode,project:facts.project,version:facts.version,target,format,platform:'linux',arch:info.arch,interface:'gui',guiFramework:'gpui',lockPolicy,package:config.package,binary:config.binary,hostTarget:host,buildMode:target===host?'native':'cross-architecture',sourceCommit:identity.commit,dirty:identity.dirty};
  if (options.command==='preview') return {...plan,tools:['rustup',`${target.replace('-unknown','')}-gcc`,'pkg-config','readelf','tar','dpkg-deb'],toolchain:'unchecked'};
  const toolchain=checkLinuxToolchain(run,target,host,environment);
  toolchain.evidence.compilerProbe=probeLinuxLinker(root,run,target,toolchain.env);
  if (options.command==='setup') return {target,toolchain:toolchain.evidence,installation:'not-needed; uses system GNU/binutils/tar/dpkg tools'};
  const selections=candidate?candidateSelection(options.e2e,options.e2eReason,options.e2eRisk):null;
  const context=candidate?(hooks.context ?? releasedContext)(root,facts):null;
  if (candidate && identity.dirty) throw new Error('candidate source must be clean');
  if (candidate) for (const [file,name] of [['LICENSE.zh-CN.md',config.productName],['LICENSE.en.md',config.productNameEn]]) {
    if (!fs.readFileSync(safePath(root,file,{type:'file'}),'utf8').includes(name)) throw new Error(`license product identity differs: ${file}`);
  }
  const resources=collectResources(root,config,'linux',candidate), notes=candidate?releaseNotes(root,facts.version):null;
  const embedNotes=candidate && (aboutPageEnabled(root) || config.releaseNotesUi===true);
  assertIgnored(run,'target/gpui-packaging/.gpui-private');
  run(process.execPath,[safePath(root,'.agents/skills/desktop-manage-version/scripts/version_gate.mjs',{type:'file'}),'check','--project-root',root,'--phase','build']);
  if (candidate) { assertIgnored(run,'release/.gpui-candidate'); assertIgnored(run,'.release-clean.gpui-stage'); isolateRelease(root); }
  (hooks.locks ?? assertDependencyLocks)(root,{rustTestManifests:facts.manifests});
  const workRoot=safePath(root,'target/gpui-packaging',{missing:true}); fs.mkdirSync(workRoot,{recursive:true});
  const work=fs.mkdtempSync(path.join(workRoot,'linux-build-'));
  const cargoDirectory=safePath(root,'target',{type:'directory'});
  const env={...toolchain.env,CARGO_TARGET_DIR:cargoDirectory};
  // 测试始终运行当前真实宿主，禁止执行交叉目标或将其记录成目标运行证据。
  const testEnv={...linuxEnvironment(host,host,environment),CARGO_TARGET_DIR:cargoDirectory,CARGO_PROFILE_TEST_DEBUG:'0'};
  delete testEnv.CARGO_BUILD_TARGET;
  delete testEnv[`CARGO_TARGET_${host.replaceAll('-','_').toUpperCase()}_LINKER`];
  testEnv.TMPDIR=path.join(work,'test-tmp'); fs.mkdirSync(testEnv.TMPDIR,{mode:0o700});
  const unitTests={...runWorkspaceTests(root,facts,run,testEnv),target:host,scope:'host-workspace'};
  if (embedNotes) {
    env.HARNESS_GPUI_RELEASE_NOTES_RS=path.join(work,'release_notes.rs');
    fs.writeFileSync(env.HARNESS_GPUI_RELEASE_NOTES_RS,renderReleaseNotesRust(notes),{flag:'wx'});
  }
  assertSourceStable(root,run,identity); assertResourcesStable(root,config,'linux',candidate,resources);
  const output=run('cargo',cargoArguments(['build','--release','--package',config.package,'--bin',config.binary,'--target',target,'--message-format','json-render-diagnostics'],lockPolicy),{env});
  const executables=output.stdout.split(/\r?\n/u).filter(Boolean).flatMap(line=>{
    let item; try { item=JSON.parse(line); } catch { return []; }
    return item.reason==='compiler-artifact' && item.target?.name===config.binary && item.executable?[item.executable]:[];
  });
  if (executables.length!==1) throw new Error('Cargo did not report exactly one GUI executable');
  const binary=safePath(cargoDirectory,path.relative(cargoDirectory,executables[0]).split(path.sep).join('/'),{type:'file'});
  if (embedNotes && !containsBytes(binary,notes.bytes)) throw new Error('Linux executable lacks release notes bytes');
  assertSourceStable(root,run,identity); assertResourcesStable(root,config,'linux',candidate,resources);
  const packaged=makeLinuxPackage({root,work,config,facts,info,binary,resources,run,format});
  assertSourceStable(root,run,identity); assertResourcesStable(root,config,'linux',candidate,resources);
  if (!isDeepStrictEqual(readConfig(root),config)) throw new Error('Linux config changed during build');
  if (candidate && !isDeepStrictEqual((hooks.context ?? releasedContext)(root,facts),context)) throw new Error('release context changed during build');
  const signing={selection:'disabled',source:'not-requested'}, signingResult={signingStatus:'unsigned',notarizationStatus:'not-applicable',signingEvidence:{...signing,status:'unsigned'}};
  const digest=sha256(packaged.artifact), filename=packaged.filename;
  const manifest={...(candidate?candidateManifest({facts,config,context,target,targetInfo:info,format,artifact:filename,artifactSha256:digest,unitTests,selections,signing,signingResult,notes,resourceVerification:packaged.resourceVerification,host:{platform:'linux',arch:hooks.arch ?? process.arch,release:os.release()},buildRun}):plan),
    packager:{name:'GNU tar / dpkg-deb',versions:toolchain.evidence.tools},artifactKind:format==='tar.gz'?'application-archive':'installer',buildMode:plan.buildMode,hostTarget:host,toolchain:toolchain.evidence,
    binarySha256:packaged.binarySha256,artifact:filename,sha256:digest,checksum:`${filename}.sha256`,unitTests,...signingResult,resourceVerification:packaged.resourceVerification,
    runtimeVerification:'Unverified',milestoneAcceptance:'pending',bundledResources:resources.map(({target,sha256})=>({target,sha256})),
    ...(!candidate?{distribution:'local-test-only',sourceDirty:identity.dirty,sourceSnapshotSha256:identity.sourceSha256}:{})};
  const directory=candidate?fs.mkdtempSync(path.join(root,'.release-clean.gpui-stage-')):fs.mkdtempSync(path.join(workRoot,'local-linux-'));
  fs.copyFileSync(packaged.artifact,path.join(directory,filename));
  fs.writeFileSync(path.join(directory,`${filename}.sha256`),`${digest}  ${filename}\n`,{flag:'wx'});
  fs.writeFileSync(path.join(directory,`${filename}.manifest.json`),JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
  const files=[filename,`${filename}.sha256`,`${filename}.manifest.json`]; exactFiles(directory,files);
  if (sha256(path.join(directory,filename))!==digest) throw new Error('Linux final copy hash differs');
  if (candidate) publishDirectory(root,directory,files);
  return {...manifest,directory:candidate?path.join(root,'release'):directory,...(selections?.e2eSelection==='enabled'?{next:'$desktop-verify-delivery'}:{})};
}
