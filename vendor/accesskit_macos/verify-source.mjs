/** 仅用 Node 标准库验证原样上游及官方许可文本的 SHA-256 来源清单。 */
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const packageRoot = dirname(fileURLToPath(import.meta.url));
const provenance = JSON.parse(readFileSync(resolve(packageRoot, 'provenance.json'), 'utf8'));

/** 对实际文件字节生成 SHA-256，不经换行或编码归一化。 */
function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

/** 递归收集原样文件，拒绝清单以外的文件或符号链接混入来源目录。 */
function sourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    if (!entry.isFile()) throw new Error(`Unexpected non-file source: ${path}`);
    return [path];
  });
}

/** 验证清单本身和其中每个唯一相对路径，阻止路径越界或重复条目。 */
function verifyManifest(name, expectedManifestHash, base) {
  const bytes = readFileSync(resolve(packageRoot, name));
  if (sha256(bytes) !== expectedManifestHash) throw new Error(`Manifest drift: ${name}`);
  const paths = new Set();
  for (const line of bytes.toString('utf8').trimEnd().split('\n')) {
    const match = /^([0-9a-f]{64})  (.+)$/.exec(line);
    if (!match) throw new Error(`Invalid checksum line: ${name}`);
    const path = resolve(base, match[2]);
    if (!path.startsWith(`${base}${sep}`) || paths.has(path)) {
      throw new Error(`Invalid or duplicate source path: ${match[2]}`);
    }
    if (sha256(readFileSync(path)) !== match[1]) throw new Error(`Source drift: ${match[2]}`);
    paths.add(path);
  }
  return paths;
}

const upstreamRoot = resolve(packageRoot, 'upstream');
const original = verifyManifest(
  provenance.crate_content_manifest,
  provenance.crate_content_manifest_sha256,
  upstreamRoot,
);
const actual = sourceFiles(upstreamRoot);
if (original.size !== provenance.crate_archive_regular_file_count
    || actual.length !== original.size || actual.some((path) => !original.has(path))) {
  throw new Error('Official crate file inventory changed');
}
const all = verifyManifest(
  provenance.source_manifest,
  provenance.source_manifest_sha256,
  packageRoot,
);
if (all.size !== original.size + Object.keys(provenance.supplemental_licenses).length) {
  throw new Error('Source and license inventory changed');
}
console.log(`Verified ${original.size} original crate files and ${all.size - original.size} official license files.`);
