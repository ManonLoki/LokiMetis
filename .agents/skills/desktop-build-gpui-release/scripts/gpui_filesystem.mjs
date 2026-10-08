/** GPUI 打包只允许项目内普通路径，并以目录重命名交付完整集合。 */
import fs from 'node:fs';
import path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';

/** 拒绝越界、Windows 别名、glob 与不规范路径组件。 */
export function relativePath(value) {
  if (typeof value !== 'string' || !value || /[\\:*?<>|\x00-\x1f\x7f]/u.test(value) || path.isAbsolute(value)
      || value.split('/').some(part => !part || part === '.' || part === '..' || /[. ]$/u.test(part) || /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/iu.test(part))) {
    throw new Error(`unsafe project-relative path: ${value}`);
  }
  return value;
}

/** 逐组件检查，缺失仅可发生在允许创建的尾部。 */
export function safePath(root, relative, { missing = false, type = null } = {}) {
  relativePath(relative);
  let cursor = root;
  const parts = relative.split('/');
  for (let index = 0; index < parts.length; index += 1) {
    cursor = path.join(cursor, parts[index]);
    let stat;
    try { stat = fs.lstatSync(cursor); }
    catch (error) { if (missing && error.code === 'ENOENT') continue; throw error; }
    if (stat.isSymbolicLink()) throw new Error(`symbolic link/reparse path is forbidden: ${relative}`);
    if (index < parts.length - 1 && !stat.isDirectory()) throw new Error(`ancestor must be a directory: ${relative}`);
    if (index === parts.length - 1 && type && !(type === 'file' ? stat.isFile() : stat.isDirectory())) throw new Error(`expected ${type}: ${relative}`);
  }
  return cursor;
}

/** 要求根路径及所有真实祖先不经符号链接。 */
export function safeRoot(value) {
  const root = path.resolve(value);
  let cursor = path.parse(root).root;
  for (const part of root.slice(cursor.length).split(path.sep).filter(Boolean)) {
    cursor = path.join(cursor, part);
    if (fs.lstatSync(cursor).isSymbolicLink()) throw new Error('project root must not traverse a symbolic link');
  }
  if (!fs.lstatSync(root).isDirectory()) throw new Error('project root must be a directory');
  return root;
}

/** 流式摘要避免大安装包占满内存。 */
export function sha256(file) {
  if (!fs.lstatSync(file).isFile() || fs.lstatSync(file).isSymbolicLink()) throw new Error('hash input must be a regular file');
  const hash = createHash('sha256');
  const descriptor = fs.openSync(file, 'r');
  try {
    const buffer = Buffer.allocUnsafe(1024 * 1024);
    let length;
    while ((length = fs.readSync(descriptor, buffer, 0, buffer.length, null))) hash.update(buffer.subarray(0, length));
  } finally { fs.closeSync(descriptor); }
  return hash.digest('hex');
}

/** 只允许普通文件集合；拒绝链接、隐藏打包中间物与空文件。 */
export function exactFiles(directory, expected) {
  const actual = fs.readdirSync(directory).sort();
  if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) throw new Error('artifact file set is not exact');
  for (const file of actual) {
    const stat = fs.lstatSync(path.join(directory, file));
    if (!stat.isFile() || stat.isSymbolicLink() || !stat.size) throw new Error(`artifact is not a nonempty regular file: ${file}`);
  }
}

/** 先隔离旧候选，失败时保留隔离目录而不把旧字节冒充本次产物。 */
export function isolateRelease(root) {
  const release = safePath(root, 'release', { missing: true });
  if (fs.existsSync(release)) {
    if (!fs.lstatSync(release).isDirectory()) throw new Error('release must be an ordinary directory');
    const previous = safePath(root, `.release-clean.${randomUUID()}`, { missing: true });
    fs.renameSync(release, previous);
  }
  fs.mkdirSync(release);
  return release;
}

/** 目标须仍为空；不递归删除活动目录、不覆盖既有文件。 */
export function publishDirectory(root, stage, expected) {
  exactFiles(stage, expected);
  const release = safePath(root, 'release', { type: 'directory' });
  if (fs.readdirSync(release).length) throw new Error('release changed while building');
  fs.rmdirSync(release);
  try { fs.renameSync(stage, release); }
  catch (error) { if (!fs.existsSync(release)) fs.mkdirSync(release); throw error; }
  exactFiles(release, expected);
}
