/** Linux包回归使用真实系统ELF、tar和dpkg-deb，不将夹具记为产品构建证据。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {spawnSync} from 'node:child_process';
import {linuxEnvironment,checkLinuxToolchain,probeLinuxLinker} from './gpui_linux_build.mjs';
import {verifyElf,makeLinuxPackage,treeSnapshot} from './gpui_linux_package.mjs';
import {TARGETS,renderDefaultConfig,validateConfig} from './gpui_config.mjs';
import {collectResources} from './gpui_resources.mjs';
import {sha256} from './gpui_filesystem.mjs';

/** 外部工具保留真实stdout和非零退出，避免把打包失败当成通过。 */
function run(program,args,options={}) {
  const result=spawnSync(program,args,{encoding:'utf8',...options});
  if(result.error || result.status!==0) throw new Error(`${program}: ${result.error?.message ?? result.stderr}`);
  return result;
}

test('cross environments isolate target compiler and pkg-config from inherited host overrides',()=>{
  const env=linuxEnvironment('x86_64-unknown-linux-gnu','aarch64-unknown-linux-gnu',{PKG_CONFIG_PATH:'/host-only',PKG_CONFIG_LIBDIR:'/host-only',PKG_CONFIG_aarch64_unknown_linux_gnu:'wrong',HARNESS_GPUI_RELEASE_NOTES_RS:'/old-notes'});
  assert.equal(env.CC_x86_64_unknown_linux_gnu,'x86_64-linux-gnu-gcc');
  assert.equal(env.CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER,'x86_64-linux-gnu-gcc');
  assert.equal(env.PKG_CONFIG_LIBDIR,'/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig');
  assert.equal(env.PKG_CONFIG_PATH,''); assert.equal(env.PKG_CONFIG_aarch64_unknown_linux_gnu,undefined);
  assert.equal(env.HARNESS_GPUI_RELEASE_NOTES_RS,undefined);
  assert.throws(()=>linuxEnvironment('x86_64-pc-windows-msvc','aarch64-unknown-linux-gnu'),/Linux GNU/);
});

test('cross toolchain rejects a host library returned by pkg-config',()=>{
  const fake=(program,args)=>({stdout:program==='rustup'?'x86_64-unknown-linux-gnu\n':args[0]==='-dumpmachine'?'x86_64-linux-gnu\n':program==='pkg-config' && args[0]==='--variable=libdir'?'/usr/lib/aarch64-linux-gnu\n':'1.0\n'});
  assert.throws(()=>checkLinuxToolchain(fake,'x86_64-unknown-linux-gnu','aarch64-unknown-linux-gnu',{}),/another architecture/);
});

test('ELF header rejects another architecture or truncated executable',t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gpui-elf-')); t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const file=path.join(root,'binary'), bytes=Buffer.alloc(64);
  bytes.set([127,69,76,70,2,1]); bytes.writeUInt16LE(62,18); fs.writeFileSync(file,bytes);
  assert.equal(verifyElf(file,TARGETS['x86_64-unknown-linux-gnu']),'verified-elf64');
  assert.throws(()=>verifyElf(file,TARGETS['aarch64-unknown-linux-gnu']),/architecture/);
  fs.writeFileSync(file,'tiny'); assert.throws(()=>verifyElf(file,TARGETS['x86_64-unknown-linux-gnu']),/truncated/);
});

test('Linux resources require an explicit mapping and reject binary collisions',t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gpui-resources-')); t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  for(const name of ['LICENSE.en.md','LICENSE.zh-CN.md','source']) fs.writeFileSync(path.join(root,name),'bytes');
  const config=renderDefaultConfig({projectId:'sample',nameZh:'示例',nameEn:'Sample',owner:'Owner'});
  config.resources=[{source:'source',macos:'skin',windows:'skin'}];
  assert.throws(()=>collectResources(root,config,'linux',false),/explicit linux/);
  config.resources[0].linux=config.binary;
  assert.throws(()=>collectResources(root,config,'linux',false),/collision/);
  config.resources[0].linux='resources/skins/source';
  validateConfig(config); assert.equal(collectResources(root,config,'linux',false).length,3);
});

for(const format of ['deb','tar.gz']) test(`real ${format} preserves final binary, resources, executable modes and Debian identity`,{skip:process.platform!=='linux'},t=>{
  const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'gpui-linux-package-')); t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const config=renderDefaultConfig({projectId:'sample',nameZh:'示例',nameEn:'Sample',owner:'Owner'});
  config.icons.source='icon.png'; config.resources=[{source:'skin',macos:'skins',windows:'resources/skins',linux:'resources/skins'}];
  for(const [name,bytes] of Object.entries({'LICENSE.en.md':'License en','LICENSE.zh-CN.md':'License zh','icon.png':'fixture-icon','skin/theme.txt':'theme'})) {
    const file=path.join(root,name); fs.mkdirSync(path.dirname(file),{recursive:true}); fs.writeFileSync(file,bytes);
  }
  const resources=collectResources(root,config,'linux',false), work=path.join(root,'work'); fs.mkdirSync(work);
  const info=TARGETS[process.arch==='arm64'?'aarch64-unknown-linux-gnu':'x86_64-unknown-linux-gnu'];
  const binary=fs.realpathSync('/usr/bin/true');
  const result=makeLinuxPackage({root,work,config,facts:{project:'sample',version:'0.1.0'},info,binary,resources,run,format});
  assert.equal(result.binarySha256,sha256(binary));
  assert.equal(result.resourceVerification,'final-linux-package-byte-identical');
  assert.ok(fs.statSync(result.artifact).size>0);
  const files=treeSnapshot(path.join(work,'extracted'));
  assert.ok(files.some(item=>item.path.endsWith('/resources/skins/theme.txt')));
  assert.ok(files.some(item=>item.path.endsWith('/sample_gui') && (item.mode & 0o111)===0o111));
  if(format==='deb') {
    assert.ok(files.some(item=>item.path==='usr/share/applications/com.sample.app.desktop'));
    const fields=run('dpkg-deb',['--field',result.artifact,'Depends']).stdout;
    assert.match(fields,/libc6 \(>= \d+\.\d+\)/);
  }
});

for(const target of ['aarch64-unknown-linux-gnu','x86_64-unknown-linux-gnu']) test(`installed ${target} links real graphics libraries and packages both formats without executing target code`,{skip:process.platform!=='linux'},t=>{
  const compiler=`${target.replace('-unknown','')}-g++`;
  if(spawnSync(compiler,['--version']).status!==0) {t.skip('target compiler is not installed on this test host'); return;}
  const root=fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()),'gpui-cross-toolchain-')); t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  fs.writeFileSync(path.join(root,'.gitignore'),'/target/\n');
  run('git',['init','-q',root]);
  const localRun=(program,args,options={})=>run(program,args,{cwd:root,...options});
  const host=run('rustc',['-vV']).stdout.match(/^host: (.+)$/mu)[1];
  const toolchain=checkLinuxToolchain(localRun,target,host);
  const probe=probeLinuxLinker(root,localRun,target,toolchain.env);
  assert.equal(probe.status,'linked-and-elf-verified');
  const config=renderDefaultConfig({projectId:'sample',nameZh:'示例',nameEn:'Sample',owner:'Owner'}); config.icons.source='icon.png';
  for(const name of ['LICENSE.en.md','LICENSE.zh-CN.md','icon.png']) fs.writeFileSync(path.join(root,name),'fixture');
  for(const format of ['deb','tar.gz']) {
    const work=path.join(root,format); fs.mkdirSync(work);
    const result=makeLinuxPackage({root,work,config,facts:{project:'sample',version:'0.1.0'},info:TARGETS[target],binary:probe.binary,resources:collectResources(root,config,'linux',false),run:localRun,format});
    assert.equal(result.resourceVerification,'final-linux-package-byte-identical');
  }
});
