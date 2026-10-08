/** GPUI 只使用 Node 标准库工程工具；纯渲染与受限字段合并不触碰目标文件。 */
export const GPUI_ENGINES = Object.freeze({ node: '>=24.21.0', pnpm: '>=12.4.1' });
export const GPUI_SCRIPTS = Object.freeze({
  validate: 'node .agents/skills/desktop-add-gpui-adapter/scripts/gpui_validate.mjs --root .',
  test: 'cargo test --workspace --all-targets --all-features',
  'release:inspect': 'node .agents/skills/desktop-prepare-release/scripts/release_git.mjs inspect --project-root .',
  'release:notes': 'node .agents/skills/desktop-prepare-release/scripts/release_notes.mjs',
  'release:context': 'node .agents/skills/desktop-prepare-release/scripts/release_context.mjs',
  'release:git': 'node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs release --project-root .',
  'gpui:package': 'node .agents/skills/desktop-build-gpui-release/scripts/build_gpui_release.mjs',
});

/** JSON 对象与数组分开判断；用户的其他字段和值按原语义保留。 */
function object(value) { return value !== null && typeof value === 'object' && !Array.isArray(value); }

/** 合法 JSON 仍须拒绝重复键，避免解析覆盖用户字段后错误地报告安全合并。 */
function parsePackage(source) {
  const value = JSON.parse(source);
  const frames = [];
  for (const match of source.matchAll(/"(?:\\.|[^"\\])*"|[{}\[\],:]|true|false|null|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/gu)) {
    const token = match[0], current = frames.at(-1);
    if (token === '{') frames.push({ object: true, key: true, keys: new Set() });
    else if (token === '[') frames.push({ object: false });
    else if (token === '}' || token === ']') frames.pop();
    else if (token === ',' && current?.object) current.key = true;
    else if (token === ':' && current?.object) current.key = false;
    else if (token.startsWith('"') && current?.object && current.key) {
      const key = JSON.parse(token);
      if (current.keys.has(key)) throw new Error(`duplicate package.json field: ${key}`);
      current.keys.add(key);
    }
  }
  if (!object(value)) throw new Error('package.json must contain an object');
  return value;
}

/** 新文件默认为 private；已有可见性保持，engine 与脚本只添加缺失项。 */
export function renderGpuiPackageJson(existingBytes = null) {
  let original = null;
  if (existingBytes !== null) {
    const bytes = typeof existingBytes === 'string' ? Buffer.from(existingBytes) : existingBytes;
    if (!(bytes instanceof Uint8Array) || bytes.length > 1024 * 1024) throw new Error('package.json must be bounded UTF-8 text');
    original = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  }
  const document = original === null ? {} : parsePackage(original);
  if (Object.hasOwn(document, 'private') && typeof document.private !== 'boolean') throw new Error('package.json private must be a boolean');
  for (const field of ['engines', 'scripts']) {
    if (Object.hasOwn(document, field) && (!object(document[field]) || Object.values(document[field]).some(value => typeof value !== 'string'))) {
      throw new Error(`package.json ${field} must be an object of strings`);
    }
  }
  let changed = original === null;
  const merged = original === null ? { private: true } : { ...document };
  for (const [field, expected] of [['engines', GPUI_ENGINES], ['scripts', GPUI_SCRIPTS]]) {
    merged[field] = { ...document[field] };
    for (const [key, value] of Object.entries(expected)) {
      if (Object.hasOwn(merged[field], key)) {
        if (merged[field][key] !== value) throw new Error(`conflicting package.json ${field}.${key}; merge explicitly without lowering requirements`);
      } else { merged[field][key] = value; changed = true; }
    }
  }
  const content = original !== null && !changed ? original : `${JSON.stringify(merged, null, 2)}\n`;
  if (Buffer.byteLength(content) > 1024 * 1024) throw new Error('merged package.json exceeds bounded size');
  return content;
}
