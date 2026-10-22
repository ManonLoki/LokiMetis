/** 用独立真实Git仓库验证同版替换的归档、审查、精确旧身份和原子推送。 */
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { lifecycleFixture } from '../.agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle_test_support.mjs';
import { resolveRepository, statePath } from '../.agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle_core.mjs';
import { replaceLocal, pushReplacement } from './reissue_v090.mjs';
const sha = bytes => createHash('sha256').update(bytes).digest('hex');

/** 建立已发布旧版和已启用审查的新候选，全程不访问真实仓库或网络。 */
function fixture(t, reviewSelection='enabled') {
  const item=lifecycleFixture(); t.after(()=>item.cleanup());
  const {repository,bare}=item.initializeRepository({remote:true});
  fs.mkdirSync(path.join(repository,'docs'));
  fs.copyFileSync(new URL('../docs/AGENT_POLICY.md',import.meta.url),path.join(repository,'docs/AGENT_POLICY.md'));
  item.git(repository,'add','docs/AGENT_POLICY.md'); item.git(repository,'commit','-qm','record policy');
  item.helper(repository,['start','--summary','old-release']);
  const oldContext=item.prepareReleaseContext(repository,{version:'0.9.0',date:'20261022'});
  item.helper(repository,['release','--version','0.9.0','--date','20261022','--release-context-sha256',oldContext.digest]);
  item.helper(repository,['push-release','--remote','origin']);
  const previous={...item.state(repository).lastRelease,releaseId:1};
  item.helper(repository,['start','--summary','replacement']); item.commitFile(repository,'replacement.txt','new source\n');
  const context=item.prepareReleaseContext(repository,{version:'0.9.0',date:'20261022',reviewSelection});
  const repo=resolveRepository(repository);
  return {item,repository,bare,repo,previous,args:{releaseContextSha256:context.digest,expectedStateSha256:sha(fs.readFileSync(statePath(repo)))}};
}

test('same_version_replacement_preserves_archive_and_resource_inventory',async t=>{
  const f=fixture(t), before=f.item.state(f.repository);
  const result=await replaceLocal(f.repo,f.args,f.previous);
  const after=f.item.state(f.repository);
  assert.notEqual(result.head,f.previous.head);
  assert.equal(f.item.git(f.repository,'rev-parse',result.tag).stdout.trim(),result.head);
  assert.equal(f.item.git(f.repository,'rev-parse',result.archiveTag).stdout.trim(),f.previous.head);
  assert.deepEqual(after.releasedResources[0].branches,before.releasedResources[0].branches);
  assert.equal(after.releasedResources[0].tag,result.archiveTag);
  assert.equal(after.releasedResources[1].head,result.head);
  assert.deepEqual(JSON.parse(fs.readFileSync(result.journal)).oldState,before);
  const pushed=pushReplacement(f.repo,f.previous);
  assert.equal(pushed.status,'replacement-pushed');
  for (const ref of ['refs/heads/main','refs/heads/release',`refs/tags/${result.tag}`]) assert.equal(f.item.git(f.bare,'rev-parse',ref).stdout.trim(),result.head);
  assert.equal(f.item.git(f.bare,'rev-parse',`refs/tags/${result.archiveTag}`).stdout.trim(),f.previous.head);
  assert.equal(pushReplacement(f.repo,f.previous).status,'already-pushed');
  await assert.rejects(()=>replaceLocal(f.repo,f.args,f.previous),/journal/);
});

test('stale_snapshot_and_disabled_review_have_no_tag_or_state_side_effects',async t=>{
  const f=fixture(t,'disabled'), before=fs.readFileSync(statePath(f.repo));
  await assert.rejects(()=>replaceLocal(f.repo,{...f.args,expectedStateSha256:'0'.repeat(64)},f.previous),/snapshot/);
  await assert.rejects(()=>replaceLocal(f.repo,f.args,f.previous),/Replacement requires/);
  assert.deepEqual(fs.readFileSync(statePath(f.repo)),before);
  assert.equal(f.item.git(f.repository,'rev-parse',f.previous.tag).stdout.trim(),f.previous.head);
});

test('remote_tag_drift_stops_all_pushes_and_normal_release_still_requires_new_version',async t=>{
  const f=fixture(t);
  const denied=f.item.helper(f.repository,['release','--version','0.9.0','--date','20261022','--release-context-sha256',f.args.releaseContextSha256],{success:false});
  assert.equal(denied.payload.code,'release-version-not-newer');
  const result=await replaceLocal(f.repo,f.args,f.previous);
  const main=f.item.git(f.bare,'rev-parse','main').stdout.trim();
  const drift=f.item.git(f.repository,'rev-parse',`${f.previous.head}^`).stdout.trim();
  f.item.git(f.bare,'update-ref',`refs/tags/${result.tag}`,drift);
  assert.throws(()=>pushReplacement(f.repo,f.previous),/Remote replacement identity/);
  assert.equal(f.item.git(f.bare,'rev-parse','main').stdout.trim(),main);
  assert.equal(f.item.git(f.bare,'rev-parse','release').stdout.trim(),f.previous.head);
  assert.equal(f.item.gitUnchecked(f.bare,'rev-parse',`refs/tags/${result.archiveTag}`).status,128);
});

test('server_rejection_keeps_all_remote_refs_unchanged',async t=>{
  const f=fixture(t), result=await replaceLocal(f.repo,f.args,f.previous);
  fs.writeFileSync(path.join(f.bare,'hooks/pre-receive'),'#!/bin/sh\nexit 1\n',{mode:0o755});
  assert.throws(()=>pushReplacement(f.repo,f.previous));
  for (const ref of ['main','release',`refs/tags/${result.tag}`]) assert.equal(f.item.git(f.bare,'rev-parse',ref).stdout.trim(),f.previous.head);
  assert.equal(f.item.gitUnchecked(f.bare,'rev-parse',`refs/tags/${result.archiveTag}`).status,128);
});
