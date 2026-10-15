#!/usr/bin/env node
// 独立核验原包完整文件集、固定摘要和唯一模板，不写 upstream 或生成目录。
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, lstatSync } from 'node:fs';
import { resolve, join, relative, sep } from 'node:path';

/** 对实际字节做 SHA-256，清单换行也属于固定输入。 */
function hash(bytes) { return createHash('sha256').update(bytes).digest('hex'); }

/** 上游只允许真实目录和普通文件，拒绝任何链接或特殊节点。 */
function files(root, directory = root, found = new Map()) {
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    const stat = lstatSync(path);
    if (stat.isSymbolicLink()) throw new Error(`upstream link: ${path}`);
    if (stat.isDirectory()) files(root, path, found);
    else if (stat.isFile()) found.set(relative(root, path).split(sep).join('/'), readFileSync(path));
    else throw new Error(`upstream special file: ${path}`);
  }
  return found;
}

/** 严格相对路径同时适用于 macOS 与 Windows，避免清单或模板逃出来源树。 */
function safePath(path) {
  if (!path || path.startsWith('/') || path.includes('\\') || path.includes(':')
      || path.split('/').some(part => !part || part === '.' || part === '..')) {
    throw new Error(`unsafe source path: ${path}`);
  }
  return path;
}

/** 仅登记这次 WGPU 官方测试字体，不能借目录名放行其它第三方或手写内容。 */
function verifyWgpuFontFixtures(directory, sourceProvenance, build) {
  if (sourceProvenance.package !== 'gpui-pre-wgpu') return null;
  const revision = '93d745668805029476e6d7fbb19652116fed51de';
  const manifestSha256 = '9fd0d42f6f6fcd62a12d6968f37d16d76b04aa048d1dbfe01814181526b8f81b';
  const declared = build.match(/const FONT_FIXTURE_MANIFEST_SHA256:\s*&str\s*=\s*"([a-f0-9]{64})"/)?.[1];
  if (sourceProvenance.version !== '0.3.9' || sourceProvenance.source_revision !== revision
      || declared !== manifestSha256) throw new Error('font fixture registration drift');
  const manifestPath = join(directory, 'test-fixtures-content.sha256');
  const provenancePath = join(directory, 'test-fixtures-provenance.json');
  for (const path of [manifestPath, provenancePath]) {
    const stat = lstatSync(path);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('font fixture metadata must be a regular file');
  }
  const manifest = readFileSync(manifestPath);
  if (hash(manifest) !== manifestSha256) throw new Error('font fixture manifest drift');
  const provenance = JSON.parse(readFileSync(provenancePath));
  if (provenance.repository !== 'https://github.com/zed-industries/zed' || provenance.revision !== revision
      || provenance.fixture_root !== 'test-fixtures' || provenance.file_count !== 5 || provenance.total_bytes !== 596542
      || provenance.verification?.sha256_manifest_file !== 'test-fixtures-content.sha256'
      || provenance.verification?.sha256_manifest_digest !== manifestSha256) throw new Error('font fixture provenance drift');
  const root = join(directory, 'test-fixtures');
  const rootStat = lstatSync(root);
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) throw new Error('font fixture root link');
  const actual = files(root);
  const expected = new Set();
  for (const line of manifest.toString('utf8').trimEnd().split('\n')) {
    const match = line.match(/^([a-f0-9]{64})  (.+)$/);
    if (!match) throw new Error('malformed font fixture manifest');
    const [, digest, path] = match;
    safePath(path);
    if (expected.has(path)) throw new Error(`duplicate font fixture: ${path}`);
    expected.add(path);
    if (!actual.has(path) || hash(actual.get(path)) !== digest) throw new Error(`font fixture byte drift: ${path}`);
  }
  if (expected.size !== 5 || actual.size !== 5) throw new Error('font fixture file set drift');
  if (!Array.isArray(provenance.files) || provenance.files.length !== 5) throw new Error('font fixture provenance file set drift');
  const reported = new Set();
  for (const record of provenance.files) {
    if (!expected.has(record.path) || reported.has(record.path)) throw new Error('font fixture provenance path drift');
    reported.add(record.path);
    const bytes = actual.get(record.path);
    const blobSha1 = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
    if (record.size_bytes !== bytes.length || record.sha256 !== hash(bytes) || record.git_blob_sha1 !== blobSha1
        || record.source_url !== `https://raw.githubusercontent.com/zed-industries/zed/${revision}/${record.path}`) {
      throw new Error(`font fixture provenance byte drift: ${record.path}`);
    }
  }
  return { files: actual.size, bytes: [...actual.values()].reduce((sum, bytes) => sum + bytes.length, 0), manifest_sha256: manifestSha256 };
}

/** 从维护入口读取固定常量，并独立按模板顺序复现生成结果及其 SHA。 */
function verify(directory) {
  const provenance = JSON.parse(readFileSync(join(directory, 'provenance.json')));
  const build = readFileSync(join(directory, 'build.rs'), 'utf8');
  const expectedManifest = build.match(/source_manifest_sha256:\s*"([a-f0-9]{64})"/)?.[1];
  const manifestBytes = readFileSync(join(directory, 'source-content.sha256'));
  if (!expectedManifest || hash(manifestBytes) !== expectedManifest
      || provenance.source_manifest_sha256 !== expectedManifest) throw new Error('source manifest drift');
  const upstream = join(directory, 'upstream');
  if (!lstatSync(upstream).isDirectory() || lstatSync(upstream).isSymbolicLink()) throw new Error('upstream root link');
  const originals = files(upstream);
  const expected = new Set();
  for (const line of manifestBytes.toString('utf8').trimEnd().split('\n')) {
    const match = line.match(/^([a-f0-9]{64})  (.+)$/);
    if (!match) throw new Error('malformed source manifest');
    const [, digest, path] = match;
    safePath(path);
    if (expected.has(path)) throw new Error(`duplicate source: ${path}`);
    expected.add(path);
    if (!originals.has(path) || hash(originals.get(path)) !== digest) throw new Error(`source byte drift: ${path}`);
  }
  if (expected.size !== originals.size || provenance.upstream_file_count !== originals.size) throw new Error('source file set drift');
  const bytes = [...originals.values()].reduce((total, value) => total + value.length, 0);
  if (provenance.upstream_bytes !== bytes) throw new Error('source byte count drift');
  const generated = new Map(originals);
  const entry = build.match(/entry:\s*"([^"]+)"/)?.[1];
  safePath(entry);
  const prefix = readFileSync(join(directory, 'patches/root-prefix.before'), 'utf8');
  const entryText = generated.get(entry).toString('utf8');
  if (!entryText.startsWith(prefix)) throw new Error('entry prefix drift');
  generated.set(entry, Buffer.from(entryText.slice(prefix.length)));
  const changed = new Set([entry]);
  const patches = [...build.matchAll(/Patch\s*\{\s*name:\s*"([^"]+)",\s*path:\s*"([^"]+)",\s*before:\s*include_str!\("([^"]+)"\),\s*after:\s*include_str!\("([^"]+)"\),?\s*\}/g)];
  for (const [, name, path, beforePath, afterPath] of patches) {
    safePath(path); safePath(beforePath); safePath(afterPath);
    const before = readFileSync(join(directory, beforePath), 'utf8');
    const after = readFileSync(join(directory, afterPath), 'utf8');
    const text = generated.get(path).toString('utf8');
    if (!before || text.split(before).length !== 2) throw new Error(`ambiguous patch: ${name}`);
    generated.set(path, Buffer.from(text.replace(before, after)));
    changed.add(path);
  }
  const checked = new Set();
  for (const [, path, digest] of build.matchAll(/\("([^"]+)",\s*"([a-f0-9]{64})"\)/g)) {
    if (checked.has(path) || !generated.has(path) || hash(generated.get(path)) !== digest) throw new Error(`generated output drift: ${path}`);
    checked.add(path);
  }
  if (changed.size !== checked.size || [...changed].some(path => !checked.has(path))) throw new Error('generated file set drift');
  const fontFixtures = verifyWgpuFontFixtures(directory, provenance, build);
  return { package: provenance.package, files: originals.size, bytes, patches: patches.length, manifest_sha256: expectedManifest,
    ...(fontFixtures ? { font_fixtures: fontFixtures } : {}) };
}

const paths = process.argv.slice(2);
if (!paths.length) throw new Error('pass one or more GPUI vendor package directories');
for (const path of paths) console.log(JSON.stringify(verify(resolve(path))));
