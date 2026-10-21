/** GitHub Actions 发布编排：精确远端身份、独立候选集合、全部成功后上传渠道。 */
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { checkContext } from '../.agents/skills/desktop-prepare-release/scripts/release_context.mjs';

const root = path.resolve(fileURLToPath(new URL('..', import.meta.url)));
const repository = 'ManonLoki/LokiMetis';
const combinations = ['windows/x86_64/nsis','macos/aarch64/dmg','linux/x86_64/deb','linux/x86_64/tar.gz','linux/aarch64/deb','linux/aarch64/tar.gz'];
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');

/** 远端数据从 argv 传给 Git/gh，不经 shell；失败保持候选和草稿以便诊断。 */
function run(program, args, options = {}) {
  const result = spawnSync(program, args, { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, timeout: 180 * 60_000, ...options });
  if (result.error || result.status !== 0) throw new Error(`${program} failed: ${(result.stderr ?? result.error?.message ?? '').slice(-2000)}`);
  return result.stdout?.trim();
}

/** 不把 tag 输入、分支移动或当前工作区状态替代已推送三引用的身份核验。 */
export function prepare(tag, execute = run) {
  if (!/^v\d+\.\d+\.\d+-\d{8}$/u.test(tag ?? '')) throw new Error('A precise release tag is required');
  const context = JSON.parse(fs.readFileSync(path.join(root,'.harness/release-context.json'),'utf8'));
  const head = execute('git',['rev-parse',`${tag}^{commit}`]);
  if (context.expectedTag !== tag || execute('git',['rev-parse','HEAD']) !== head) throw new Error('Checkout and release context must match the requested tag');
  const refs = execute('git',['ls-remote','--refs','origin',`refs/heads/${context.defaultBranch}`,'refs/heads/release',`refs/tags/${tag}`]).split(/\r?\n/u);
  const actual = new Map(refs.map(line => line.split(/\s+/u)).map(([oid,ref]) => [ref,oid]));
  for (const ref of [`refs/heads/${context.defaultBranch}`,'refs/heads/release',`refs/tags/${tag}`]) if (actual.get(ref) !== head) throw new Error(`Remote release identity differs: ${ref}`);
  execute('git',['checkout','-B',context.defaultBranch,head]);
  return checkContext({ projectRoot: root, expectedHead: head }, { published: true });
}

/** 每个格式完成后立即保存到项目外目录，不让下一次 candidate 原子刷新删除前一集合。 */
function build(tag, target, formats, output) {
  prepare(tag);
  if (!output || !path.isAbsolute(output) || fs.existsSync(output)) throw new Error('Build staging must be a new absolute directory');
  fs.mkdirSync(output,{ recursive: true });
  for (const format of formats.split(',')) {
    const selection = process.env.E2E_SELECTION;
    if (!['enabled','disabled'].includes(selection)) throw new Error('This run requires an explicit E2E selection');
    const args = ['.agents/skills/desktop-build-gpui-release/scripts/build_gpui_release.mjs','build','--root',root,'--mode','candidate','--target',target,'--format',format,'--e2e',selection];
    if (selection === 'disabled') args.push('--e2e-reason','User explicitly disabled E2E for this run','--e2e-risk','Installation and interactive runtime on target systems remain unverified');
    if (process.platform === 'darwin') args.push('--signing','enabled','--signing-source','requested');
    run(process.execPath,args,{ stdio: 'inherit' });
    for (const name of fs.readdirSync(path.join(root,'release'))) {
      const source = path.join(root,'release',name);
      if (!fs.lstatSync(source).isFile() || fs.existsSync(path.join(output,name))) throw new Error('Unexpected or duplicate candidate output');
      fs.copyFileSync(source,path.join(output,name),fs.constants.COPYFILE_EXCL);
    }
  }
}

/** 发布集合必须是当前运行的完整六包，每份摘要、源码、测试、签名和相邻证据均精确一致。 */
export function validateArtifacts(directory, context, runId = process.env.GITHUB_RUN_ID) {
  const entries = fs.readdirSync(directory), expected = new Set(), seen = new Set();
  const manifests = entries.filter(name => name.endsWith('.manifest.json'));
  if (manifests.length !== combinations.length) throw new Error('Release requires exactly six candidate manifests');
  for (const name of manifests) {
    const file = path.join(directory,name);
    if (!fs.lstatSync(file).isFile()) throw new Error('Candidate manifest must be a regular file');
    const item = JSON.parse(fs.readFileSync(file,'utf8'));
    const key = `${item.platform}/${item.arch}/${item.format}`;
    if (!combinations.includes(key) || seen.has(key)) throw new Error('Unexpected or duplicate platform/format');
    seen.add(key);
    if (item.sourceCommit !== context.sourceCommit || item.version !== context.version || item.expectedTag !== context.expectedTag || item.releaseContextSha256 !== context.releaseContextSha256) throw new Error('Candidate source identity differs');
    if (item.unitTests?.status !== 'passed' || !(item.unitTests.passed > 0) || item.resourceVerification === 'Unverified' || item.milestoneAcceptance !== 'pending') throw new Error('Candidate build evidence is incomplete');
    if (!['enabled','disabled'].includes(item.e2eSelection) || (item.e2eSelection === 'enabled' && item.e2eStatus !== 'passed')) throw new Error('Required E2E acceptance remains incomplete');
    if (item.buildProvider?.name !== 'github-actions' || item.buildProvider.repository !== repository || String(item.buildProvider.runId) !== String(runId)) throw new Error('Candidate belongs to another provider or run');
    if (item.platform === 'macos' && (item.signingStatus !== 'signed' || item.notarizationStatus !== 'accepted-stapled' || item.notarizationEvidence?.gatekeeper !== 'accepted')) throw new Error('macOS signing/notarization is incomplete');
    if (typeof item.artifact !== 'string' || path.basename(item.artifact) !== item.artifact || name !== `${item.artifact}.manifest.json` || item.checksum !== `${item.artifact}.sha256`) throw new Error('Unsafe candidate filename');
    for (const candidate of [item.artifact,item.checksum,name]) {
      if (!fs.lstatSync(path.join(directory,candidate)).isFile()) throw new Error('Candidate must be a regular file');
      expected.add(candidate);
    }
    if (hash(path.join(directory,item.artifact)) !== item.sha256 || fs.readFileSync(path.join(directory,item.checksum),'utf8') !== `${item.sha256}  ${item.artifact}\n`) throw new Error('Candidate checksum differs');
  }
  if (entries.length !== expected.size || entries.some(name => !expected.has(name))) throw new Error('Unexpected files in release set');
  return [...expected].sort();
}

/** 先创建草稿、上传并下载复算每个附件，全部精确后才公开；不自动清理旧 Releases。 */
function publish(tag, directory) {
  const context = prepare(tag), files = validateArtifacts(directory,context);
  const candidates = files.filter(name => name.endsWith('.manifest.json')).map(name => JSON.parse(fs.readFileSync(path.join(directory,name),'utf8')));
  if (new Set(candidates.map(item => item.e2eSelection)).size !== 1) throw new Error('Candidate E2E selections differ');
  const e2eDescription = candidates[0].e2eSelection === 'disabled' ? 'Not run (user selected disabled). Target installation and interactive runtime: Unverified.' : 'Passed on the final candidates; see each manifest for evidence.';
  const body = path.join(process.env.RUNNER_TEMP,'loki-release-notes.md');
  const notes = JSON.parse(fs.readFileSync(path.join(root,'release-notes.json'),'utf8')).releases[0];
  const section = (locale,heading) => `${heading}\n\n${[...notes.featureOptimizations,...notes.bugFixes].map(item => `- ${item[locale]}`).join('\n')}\n`;
  fs.writeFileSync(body,`${section('zh-CN',`LokiMetris v${context.version}`)}\n${section('en-US','Changes')}\nWindows x64 · macOS Apple Silicon (Developer ID signed and notarized) · Linux AMD64/ARM64 (deb and tar.gz).\n\nE2E: ${e2eDescription}\n\nSource: ${context.sourceCommit}\nBuild: https://github.com/${repository}/actions/runs/${process.env.GITHUB_RUN_ID}\n`);
  const query = spawnSync('gh',['release','view',tag,'--repo',repository,'--json','isDraft'],{ encoding:'utf8' });
  if (query.status === 0) {
    if (!JSON.parse(query.stdout).isDraft) throw new Error('Refusing to replace an already published release');
  } else run('gh',['release','create',tag,'--repo',repository,'--verify-tag','--draft','--title',`LokiMetris v${context.version} · Windows / macOS / Linux`,'--notes-file',body]);
  run('gh',['release','upload',tag,...files.map(name => path.join(directory,name)),'--repo',repository,'--clobber']);
  const assets = JSON.parse(run('gh',['release','view',tag,'--repo',repository,'--json','assets'])).assets;
  if (assets.length !== files.length || assets.some(asset => !files.includes(asset.name))) throw new Error('Remote release asset set differs');
  const download = fs.mkdtempSync(path.join(process.env.RUNNER_TEMP,'loki-release-verify-'));
  run('gh',['release','download',tag,'--repo',repository,'--dir',download]);
  for (const name of files) if (hash(path.join(download,name)) !== hash(path.join(directory,name))) throw new Error('Remote release attachment checksum differs');
  prepare(tag);
  run('gh',['release','edit',tag,'--repo',repository,'--draft=false','--latest','--notes-file',body]);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command,tag,...args] = process.argv.slice(2);
    if (command === 'prepare') console.log(JSON.stringify(prepare(tag)));
    else if (command === 'build') build(tag,...args);
    else if (command === 'publish') publish(tag,...args);
    else throw new Error('Expected prepare, build or publish');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
