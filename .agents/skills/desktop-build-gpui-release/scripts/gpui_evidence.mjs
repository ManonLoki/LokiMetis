/** 把既有发布事实投影到 GPUI 候选，不创建发布上下文或提升版本。 */
import fs from 'node:fs';
import path from 'node:path';
import { checkContext } from '../../desktop-prepare-release/scripts/release_context.mjs';
import { loadDocument } from '../../desktop-prepare-release/scripts/release_notes.mjs';
import { safePath, sha256 } from './gpui_filesystem.mjs';

/** 每次构建独立记录选择；关闭也必须给出可复核原因与风险。 */
export function candidateSelection(e2e, reason, risk) {
  if (!['enabled','disabled'].includes(e2e)) throw new Error('candidate build requires this request\'s explicit --e2e enabled|disabled');
  if (e2e === 'disabled' && [reason,risk].some(value => typeof value !== 'string' || !value.trim() || /[\x00-\x1f]/u.test(value))) throw new Error('disabled E2E requires --e2e-reason and --e2e-risk');
  return { e2eSelection: e2e, e2eStatus: e2e === 'enabled' ? 'pending' : 'Not run', ...(e2e === 'disabled' ? { e2eReason: reason, e2eRemainingRisk: risk } : {}) };
}

/** 上下文通用门禁同时证明默认分支、tag、clean 和生命周期登记。 */
export function releasedContext(root, facts) {
  return checkContext({ projectRoot: root, expectedVersion: facts.version }, { published: true });
}

/** 验证已发布日志并保留原始字节用于资源及二进制绑定。 */
export function releaseNotes(root, version) {
  const file = safePath(root, 'release-notes.json', { type: 'file' });
  const document = loadDocument(file);
  if (document.releases[0].version !== `v${version}`) throw new Error('release notes latest version does not match Cargo');
  return { document, bytes: fs.readFileSync(file), sha256: sha256(file) };
}

/** 关于页禁用时不要求日志 UI 或二进制嵌入，但包内独立日志资源仍核对。 */
export function aboutPageEnabled(root) {
  const source = fs.readFileSync(safePath(root,'docs/GUI_APP_PROFILE.md',{ type: 'file' }),'utf8');
  const blocks = [...source.matchAll(/```gui-initialization-config\r?\n([\s\S]*?)```/gu)];
  if (blocks.length !== 1) throw new Error('GUI profile requires one initialization configuration block');
  const fields = [...blocks[0][1].matchAll(/^about_page:[ \t]*(enabled|disabled)[ \t]*\r?$/gmu)];
  if (fields.length !== 1 || [...blocks[0][1].matchAll(/^about_page:/gmu)].length !== 1) throw new Error('GUI profile requires one explicit about_page choice');
  return fields[0][1] === 'enabled';
}

/** Rust Unicode 转义不同于 JSON；把所有控制字符编码成合法 Rust 字面量。 */
function rustString(value) {
  return '"' + [...value].map(character => character === '"' ? '\\"' : character === '\\' ? '\\\\' : character.codePointAt(0) < 32 || character.codePointAt(0) === 127 ? `\\u{${character.codePointAt(0).toString(16)}}` : character).join('') + '"';
}

/** GPUI build.rs 只复制此可信生成文件；生成物不写入源码或受跟踪记忆。 */
export function renderReleaseNotesRust(notes) {
  const pairs = items => '&[' + items.map(item => `(${rustString(item['zh-CN'])}, ${rustString(item['en-US'])})`).join(', ') + ']';
  const entries = notes.document.releases.map(item => `    (${rustString(item.releaseDate)}, ${rustString(item.version)}, ${pairs(item.featureOptimizations)}, ${pairs(item.bugFixes)}),`).join('\n');
  return `// 构建临时生成；来源为已验证的根 release-notes.json。\npub const RELEASE_NOTES_JSON: &[u8] = &[${[...notes.bytes].join(',')}];\npub const RELEASE_NOTES: &[(&str, &str, &[(&str, &str)], &[(&str, &str)])] = &[\n${entries}\n];\n`;
}

/** 含跨块重叠的流式搜索，证明实际二进制保留了本次日志字节。 */
export function containsBytes(file, bytes) {
  const descriptor = fs.openSync(file, 'r');
  let tail = Buffer.alloc(0);
  try {
    const buffer = Buffer.alloc(1024 * 1024);
    let length;
    while ((length = fs.readSync(descriptor,buffer,0,buffer.length,null))) {
      const sample = Buffer.concat([tail,buffer.subarray(0,length)]);
      if (sample.includes(bytes)) return true;
      tail = sample.subarray(Math.max(0,sample.length - bytes.length + 1));
    }
  } finally { fs.closeSync(descriptor); }
  return false;
}

/** manifest 使用现有验收字段，构建只创建 pending，不提升为 accepted。 */
export function candidateManifest({ facts, config, context, target, targetInfo, format, artifact, artifactSha256, unitTests, selections, signing, signingResult, notes, resourceVerification, host, buildRun }) {
  const review = context.releaseReview;
  const reviewFields = review.selection === 'enabled'
    ? { reviewedSourceCommit: review.reviewedSourceCommit, reviewEvidence: review }
    : { reviewReason: review.reason, reviewRemainingRisk: review.remainingRisk };
  return {
    schemaVersion: 1, project: facts.project, product: config.productName,
    version: facts.version, sourceCommit: context.sourceCommit,
    expectedTag: context.expectedTag, releaseContextSha256: context.releaseContextSha256,
    guiFramework: 'gpui', packager: { name: 'cargo-packager', version: '0.11.8' },
    buildRun, interface: 'gui', artifactKind: format === 'app' ? 'application-archive' : 'installer', bundleFormat: format,
    buildMode: 'native', buildIdentity: { package: config.package, binary: config.binary, identifier: config.identifier },
    runtimeIdentity: { productName: config.productName, identifier: config.identifier },
    platform: targetInfo.platform, arch: targetInfo.arch, target, host,
    format, artifact, sha256: artifactSha256, checksum: `${artifact}.sha256`, unitTests,
    ...selections, candidateSelections: { e2eSelection: selections.e2eSelection, signingSelection: signing.selection, signingSource: signing.source },
    releaseReview: review, reviewSelection: review.selection, reviewStatus: review.status, ...reviewFields,
    signingSelection: signing.selection, signingSource: signing.source, ...signingResult,
    ...(targetInfo.platform === 'macos' ? { macosSigningSelection: signing.selection, macosSigningSource: signing.source } : {}),
    releaseNotesVersion: notes.document.releases[0].version, releaseNotesSha256: notes.sha256,
    releaseNotesPath: 'release-notes.json', releaseNotesResourceVerification: 'byte-identical',
    resourceVerification, licenses: ['LICENSE.zh-CN.md','LICENSE.en.md'],
    runtimeVerification: 'Unverified', milestoneAcceptance: 'pending',
  };
}
