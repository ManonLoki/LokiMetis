/** 仅承接用户于2026-10-22明确批准的v0.9.0替换；普通发布仍禁止移动旧tag和同版重发。 */
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import {
  resolveRepository, requireClean, currentHead, currentBranchOrNone, loadState,
  saveState, statePath, localTagTarget, runGit, withLifecycleStateLock,
} from '../.agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle_core.mjs';
import { prepareReviewedReleaseHead, inspectReleasePushReferences } from '../.agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle_publication.mjs';
import { inspect as inspectPostReleaseAction } from '../.agents/skills/desktop-switch-post-release-action/scripts/post_release_action.mjs';

/** 此固定身份是一次性授权边界，不能通过CLI传入另一旧发布。 */
export const approved = Object.freeze({
  tag: 'v0.9.0-20261022', version: '0.9.0', date: '20261022',
  head: '66bbf827b5eb5894d1fa3273977cd35978ed2d98',
  releaseContextSha256: 'c9ce87699909f205550d54bc6a0d53ea1a6e122d315dd767444bae3e420aaa55',
  releaseId: 420353378,
});
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const archiveTag = previous => `archive/${previous.tag}/${previous.head}`;
const journalPath = repository => path.join(repository.commonDir, 'agent-first-harness/v090-replacement.json');

/** 副作用前验证精确旧发布、原始tag对象和周期状态；拒绝猜测或使用恢复命令绕过。 */
export function assertPrevious(repository, state, previous, expectedStateSha256) {
  if (currentBranchOrNone(repository) !== 'main') throw new Error('Integrate on main before replacement');
  for (const branch of state.cycle?.branches ?? []) {
    if (runGit(repository.root,['merge-base','--is-ancestor',`refs/heads/${branch.name}`,'HEAD'],{check:false}).returncode !== 0) throw new Error('Integrate every registered branch before replacement');
  }
  if (sha(fs.readFileSync(statePath(repository))) !== expectedStateSha256) throw new Error('Lifecycle snapshot changed');
  const last = state.lastRelease;
  if (!last || ['tag','version','date','head','releaseContextSha256'].some(key => last[key] !== previous[key]) ||
      last.defaultBranch !== 'main' || state.pendingPublish !== null || state.cycle?.pendingRelease) throw new Error('Previous release identity differs');
  if (localTagTarget(repository, previous.tag) !== previous.head ||
      runGit(repository.root, ['rev-parse', `refs/tags/${previous.tag}`]).stdout.trim() !== previous.head) throw new Error('Previous lightweight tag differs');
  const oldContext = runGit(repository.root, ['show', `${previous.head}:.harness/release-context.json`], { bytes: true }).stdout;
  if (sha(oldContext) !== previous.releaseContextSha256) throw new Error('Previous context differs');
  if (runGit(repository.root, ['merge-base','--is-ancestor',previous.head,'HEAD'], { check:false }).returncode !== 0) throw new Error('Replacement is not a successor');
  if (!state.releasedResources.some(item => item.tag === previous.tag && item.head === previous.head)) throw new Error('Previous resource inventory missing');
  if (localTagTarget(repository, archiveTag(previous)) !== null) throw new Error('Archive tag already exists');
}

/** 旧状态与新身份先排他落盘，再以Git事务替换本地tag并保留原始资源登记。 */
export async function replaceLocal(repository, args, previous = approved) {
  requireClean(repository);
  if (fs.existsSync(journalPath(repository))) throw new Error('Replacement journal already exists; inspect it before any retry');
  const oldBytes = fs.readFileSync(statePath(repository)), state = loadState(repository);
  assertPrevious(repository, state, previous, args.expectedStateSha256);
  if (inspectPostReleaseAction(repository.root,{requireConfigured:true}).post_release_action !== 'push_release_branch') throw new Error('Replacement requires push_release_branch');
  const next = await prepareReviewedReleaseHead(repository,state,{ version:previous.version,date:previous.date,releaseContextSha256:args.releaseContextSha256 });
  if (next.head === previous.head || next.branch !== 'main' || next.postReleaseAction !== 'push_release_branch') throw new Error('Replacement identity or action invalid');
  const backup = archiveTag(previous);
  const journal = { schemaVersion:1, authorization:'User requested publishing and replacing the previous 0.9.0 without a version bump on 2026-10-22', previous,
    archiveTag:backup, expectedStateSha256:args.expectedStateSha256, oldState:JSON.parse(oldBytes), next };
  fs.writeFileSync(journalPath(repository),`${JSON.stringify(journal,null,2)}\n`,{flag:'wx',mode:0o600});
  runGit(repository.root,['update-ref','--stdin'],{ input:`start\ncreate refs/tags/${backup} ${previous.head}\nupdate refs/tags/${previous.tag} ${next.head} ${previous.head}\nprepare\ncommit\n` });
  for (const item of state.releasedResources) if (item.tag === previous.tag) item.tag = backup;
  state.releasedResources.push({tag:previous.tag,head:next.head,branches:state.cycle?.branches ?? [],worktrees:state.cycle?.worktrees ?? []});
  state.lastRelease = {tag:next.tag,date:next.date,version:next.version,head:next.head,defaultBranch:next.branch,postReleaseAction:next.postReleaseAction,releaseContextSha256:next.releaseContextSha256};
  state.cycle = null;
  saveState(repository,state);
  return {status:'replaced-locally',head:next.head,tag:next.tag,archiveTag:backup,journal:journalPath(repository)};
}

/** 只替换精确旧tag，主分支和release保持非强制；服务器不支持原子推送时直接失败。 */
export function pushReplacement(repository, expected = approved) {
  requireClean(repository);
  const journal = JSON.parse(fs.readFileSync(journalPath(repository),'utf8'));
  if (JSON.stringify(journal.previous) !== JSON.stringify(expected)) throw new Error('Unexpected replacement journal');
  const {previous,next,archiveTag:backup} = journal, last = loadState(repository).lastRelease;
  if (last.head !== next.head || last.releaseContextSha256 !== next.releaseContextSha256 || currentHead(repository) !== next.head || currentBranchOrNone(repository) !== 'main' ||
      localTagTarget(repository,previous.tag) !== next.head || localTagTarget(repository,backup) !== previous.head) throw new Error('Local replacement changed');
  if (inspectReleasePushReferences(repository,'origin',previous.tag).defaultBranch !== 'main') throw new Error('Remote default branch changed');
  const local = runGit(repository.root,['rev-parse','--verify','refs/heads/release'],{check:false});
  const old = local.returncode===0 ? local.stdout.trim() : '0'.repeat(40);
  if (![previous.head,next.head,'0'.repeat(40)].includes(old) || runGit(repository.root,['worktree','list','--porcelain']).stdout.split(/\r?\n/u).includes('branch refs/heads/release')) throw new Error('Unexpected local release branch');
  const confirmLocal = () => runGit(repository.root,['update-ref','refs/heads/release',next.head,old]);
  const names = ['refs/heads/main','refs/heads/release',`refs/tags/${previous.tag}`,`refs/tags/${backup}`];
  const read = () => new Map(runGit(repository.root,['ls-remote','--refs','origin',...names]).stdout.trim().split(/\r?\n/u).filter(Boolean).map(line=>line.split(/\s+/u).reverse()));
  const before = read();
  if (before.get(names[2]) === next.head && before.get(names[0]) === next.head && before.get(names[1]) === next.head && before.get(names[3]) === previous.head) {
    confirmLocal();
    return {status:'already-pushed',head:next.head};
  }
  if (before.get(names[2]) !== previous.head || before.get(names[1]) !== previous.head || before.has(names[3])) throw new Error('Remote replacement identity changed');
  if (runGit(repository.root,['merge-base','--is-ancestor',before.get(names[0]),next.head],{check:false}).returncode !== 0) throw new Error('Remote main is not an ancestor');
  runGit(repository.root,['push','--atomic',`--force-with-lease=${names[2]}:${previous.head}`,'origin',`${next.head}:${names[0]}`,`${next.head}:${names[1]}`,`refs/tags/${previous.tag}:${names[2]}`,`refs/tags/${backup}:${names[3]}`]);
  const after = read();
  if (names.some((name,index)=>after.get(name) !== (index===3 ? previous.head : next.head))) throw new Error('Remote replacement verification failed');
  const confirmed = inspectReleasePushReferences(repository,'origin',previous.tag);
  if (confirmed.defaultBranch !== 'main' || confirmed.defaultHead !== next.head || confirmed.branch !== next.head || confirmed.tag !== next.head) throw new Error('Remote default or release identity changed');
  confirmLocal();
  return {status:'replacement-pushed',head:next.head,tag:next.tag,archiveTag:backup};
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const repository = resolveRepository(path.resolve(fileURLToPath(new URL('..',import.meta.url))));
    const [command,releaseContextSha256,expectedStateSha256] = process.argv.slice(2);
    if (!['local','push'].includes(command) || (command==='local' && ![releaseContextSha256,expectedStateSha256].every(value=>/^[a-f0-9]{64}$/u.test(value??'')))) throw new Error('Expected local <context-sha256> <state-sha256> or push');
    console.log(JSON.stringify(await withLifecycleStateLock(repository,()=>command==='local' ? replaceLocal(repository,{releaseContextSha256,expectedStateSha256}) : pushReplacement(repository))));
  } catch (error) { console.error(error.message); process.exitCode=1; }
}
