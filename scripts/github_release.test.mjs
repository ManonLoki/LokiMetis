/** 渠道上传前校验四个目标的六份安装包及实际字节，避免混入历史或不完整候选。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { validateArtifacts } from './github_release.mjs';

/** 使用独立目录形成完整六包，任一失败不得影响真实发布目录。 */
function fixture(t) {
  const dir=fs.mkdtempSync(path.join(os.tmpdir(),'github-release-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));
  const context={sourceCommit:'a'.repeat(40),version:'0.7.5',expectedTag:'v0.7.5-20261022',releaseContextSha256:'b'.repeat(64)};
  const combinations=[['windows','x86_64','nsis'],['macos','aarch64','dmg'],...['x86_64','aarch64'].flatMap(arch=>['deb','tar.gz'].map(format=>['linux',arch,format]))];
  const names=[];
  for (const [platform,arch,format] of combinations) {
    const artifact=`fixture-${platform}-${arch}.${format}`,bytes=Buffer.from(artifact),sha256=createHash('sha256').update(bytes).digest('hex');
    const item={...context,platform,arch,format,artifact,checksum:`${artifact}.sha256`,sha256,unitTests:{status:'passed',passed:1},e2eSelection:'disabled',e2eStatus:'Not run',resourceVerification:'final-package-byte-identical',milestoneAcceptance:'pending',buildProvider:{name:'github-actions',repository:'ManonLoki/LokiMetis',runId:'123'},signingStatus:'signed',notarizationStatus:'accepted-stapled',notarizationEvidence:{gatekeeper:'accepted'}};
    fs.writeFileSync(path.join(dir,artifact),bytes);fs.writeFileSync(path.join(dir,item.checksum),`${sha256}  ${artifact}\n`);fs.writeFileSync(path.join(dir,`${artifact}.manifest.json`),JSON.stringify(item));names.push(artifact);
  }
  return {dir,context,names};
}

test('complete current run is accepted and byte tampering is rejected',t=>{
  const f=fixture(t);assert.equal(validateArtifacts(f.dir,f.context,'123').length,18);
  fs.appendFileSync(path.join(f.dir,f.names[0]),'tampered');
  assert.throws(()=>validateArtifacts(f.dir,f.context,'123'),/checksum/);
});

test('missing platform historical run extra file and incomplete signing block upload',t=>{
  for (const mutate of [
    f=>fs.unlinkSync(path.join(f.dir,`${f.names[0]}.manifest.json`)),
    f=>fs.writeFileSync(path.join(f.dir,'old.exe'),'old'),
    f=>{const p=path.join(f.dir,`${f.names[1]}.manifest.json`),item=JSON.parse(fs.readFileSync(p));item.notarizationStatus='not-run';fs.writeFileSync(p,JSON.stringify(item));},
    f=>{const p=path.join(f.dir,`${f.names[0]}.manifest.json`),item=JSON.parse(fs.readFileSync(p));item.buildProvider.runId='older';fs.writeFileSync(p,JSON.stringify(item));},
    f=>{const p=path.join(f.dir,`${f.names[0]}.manifest.json`),item=JSON.parse(fs.readFileSync(p));item.e2eSelection='enabled';item.e2eStatus='pending';fs.writeFileSync(p,JSON.stringify(item));}
  ]) {const f=fixture(t);mutate(f);assert.throws(()=>validateArtifacts(f.dir,f.context,'123'));}
});
