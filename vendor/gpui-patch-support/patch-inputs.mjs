// 只解释本地维护入口实际使用的补丁声明子集；未知表达式必须显式失败，不能漏掉补丁。
import { readFileSync, lstatSync } from 'node:fs';
import { resolve, dirname, relative, sep, isAbsolute } from 'node:path';

/** 删除注释并保留字符串内容，避免注释中的补丁示例变成真实登记。 */
function withoutComments(source) {
  let result = '';
  for (let i = 0; i < source.length;) {
    if (source[i] === '"') {
      const start = i++;
      while (i < source.length && source[i] !== '"') i += source[i] === '\\' ? 2 : 1;
      if (i === source.length) throw new Error('unterminated patch source string');
      result += source.slice(start, ++i);
    } else if (source.startsWith('//', i)) {
      const end = source.indexOf('\n', i);
      i = end < 0 ? source.length : end;
      result += ' ';
    } else if (source.startsWith('/*', i)) {
      let depth = 1;
      i += 2;
      while (i < source.length && depth) {
        if (source.startsWith('/*', i)) { depth++; i += 2; }
        else if (source.startsWith('*/', i)) { depth--; i += 2; }
        else i++;
      }
      if (depth) throw new Error('unterminated patch source comment');
      result += ' ';
    } else result += source[i++];
  }
  return result;
}

/** 按括号及字符串边界切分真实数组成员，不把片段字段中的逗号视为组边界。 */
function arrayAt(source, start) {
  if (source[start] !== '[') throw new Error('expected patch array');
  const stack = [']'];
  const items = [];
  let begin = start + 1;
  for (let i = begin; i < source.length; i++) {
    const char = source[i];
    if (char === '"') {
      while (++i < source.length && source[i] !== '"') if (source[i] === '\\') i++;
    } else if ('[({'.includes(char)) stack.push({ '[': ']', '(': ')', '{': '}' }[char]);
    else if ('])}'.includes(char)) {
      if (stack.pop() !== char) throw new Error('unbalanced patch array');
      if (!stack.length) {
        if (source.slice(begin, i).trim()) items.push(source.slice(begin, i).trim());
        return { items, end: i + 1 };
      }
    } else if (char === ',' && stack.length === 1) {
      const item = source.slice(begin, i).trim();
      if (!item) throw new Error('empty patch array member');
      items.push(item);
      begin = i + 1;
    }
  }
  throw new Error('unterminated patch array');
}

/** 读取声明文件自身相对的 include 输入，仅允许 vendor 根内真实普通文件。 */
function inputReader(directory) {
  const root = dirname(resolve(directory));
  return (owner, name) => {
    if (!name || isAbsolute(name) || name.includes('\\') || name.includes(':')) throw new Error(`unsafe patch input: ${name}`);
    const path = resolve(dirname(owner), name);
    const parts = relative(root, path).split(sep);
    if (parts[0] === '..' || parts.some(part => !part || part === '.')) throw new Error(`unsafe patch input: ${name}`);
    let current = root;
    for (const [index, part] of parts.entries()) {
      current = resolve(current, part);
      const stat = lstatSync(current);
      if (stat.isSymbolicLink() || (index === parts.length - 1 ? !stat.isFile() : !stat.isDirectory())) {
        throw new Error(`patch input must use real directories and regular files: ${name}`);
      }
    }
    return { path, text: readFileSync(path, 'utf8') };
  };
}

/** 依照 build.rs 的实际拼装顺序展开共享函数、模块常量、宏和内联片段。 */
export function readPatchInputs(directory, build) {
  const owner = resolve(directory, 'build.rs');
  const read = inputReader(directory);
  const source = withoutComments(build);
  const modules = new Map();
  for (const [, path, name] of source.matchAll(/#\[\s*path\s*=\s*"([^"\\]+)"\s*\]\s*(?:#\[[^\]]+\]\s*)*mod\s+(\w+)\s*;/g)) {
    if (modules.has(name)) throw new Error(`duplicate patch module: ${name}`);
    modules.set(name, path);
  }
  /** 每个数组成员须完整匹配，任何新增 Rust 表达式都必须先扩充验证器。 */
  function expand(item, file, bindings = {}) {
    let match = item.match(/^Patch\s*\{\s*name:\s*"([^"\\]+)",\s*path:\s*"([^"\\]+)",\s*before:\s*include_str!\("([^"\\]+)"\),\s*after:\s*include_str!\("([^"\\]+)"\),?\s*\}$/);
    if (match) return [{ name: match[1], path: match[2], before: read(file, match[3]).text, after: read(file, match[4]).text }];
    match = item.match(/^patch!\(\s*"([^"\\]+)",\s*"([^"\\]+)",\s*(?:"([^"\\]+)"|(\w+))\s*,?\s*\)$/);
    if (match) {
      const [, prefix, name, literal, parameter] = match;
      const path = literal ?? bindings[parameter];
      if (!path) throw new Error(`unbound patch target: ${parameter}`);
      return [{ name, path, before: read(file, `${prefix}${name}.before`).text, after: read(file, `${prefix}${name}.after`).text }];
    }
    if (item.startsWith('&[')) {
      const list = arrayAt(item, 1);
      if (item.slice(list.end).trim()) throw new Error('unsupported inline patch array');
      return list.items.flatMap(value => expand(value, file, bindings));
    }
    match = item.match(/^(\w+)::PATCHES$|^&(\w+)::all\(\s*"([^"\\]+)"\s*\)\s*\[\.\.\]$/);
    if (!match || file !== owner) throw new Error(`unsupported patch expression: ${item.slice(0, 100)}`);
    const module = match[1] ?? match[2];
    if (!modules.has(module)) throw new Error(`missing patch module: ${module}`);
    const loaded = read(owner, modules.get(module));
    const text = withoutComments(loaded.text);
    const declaration = match[1]
      ? text.match(/\bpub\s+const\s+PATCHES:\s*&\[Patch\]\s*=\s*&\s*/)
      : text.match(/\bpub\s+const\s+fn\s+all\s*\(\s*(\w+):\s*&'static\s+str\s*\)\s*->\s*\[Patch;\s*(\d+)\]\s*\{\s*/);
    if (!declaration) throw new Error(`unsupported patch group: ${module}`);
    const list = arrayAt(text, declaration.index + declaration[0].length);
    if (!text.slice(list.end).match(match[1] ? /^\s*;/ : /^\s*\}/)) throw new Error(`unsupported patch group tail: ${module}`);
    const patches = list.items.flatMap(value => expand(value, loaded.path, match[1] ? {} : { [declaration[1]]: match[3] }));
    if (!match[1] && patches.length !== Number(declaration[2])) throw new Error(`patch group length drift: ${module}`);
    return patches;
  }
  const field = [...source.matchAll(/\bpatches:\s*&\s*/g)];
  if (field.length !== 1) throw new Error('expected one Spec patches field');
  let start = field[0].index + field[0][0].length;
  let list;
  if (source[start] === '[') list = arrayAt(source, start);
  else {
    if (!/^patches\s*,/.test(source.slice(start))) throw new Error('unsupported Spec patches value');
    const declaration = source.match(/\blet\s+patches:\s*Vec<Patch>\s*=\s*/);
    if (!declaration) throw new Error('missing patch assembly');
    start = declaration.index + declaration[0].length;
    list = arrayAt(source, start);
    if (!/^\s*\.concat\(\)\s*;/.test(source.slice(list.end))) throw new Error('unsupported patch assembly tail');
  }
  const prefix = source.match(/\broot_prefix:\s*include_str!\("([^"\\]+)"\)/)?.[1];
  if (!prefix) throw new Error('unsupported root prefix');
  return { patches: list.items.flatMap(item => expand(item, owner)), rootPrefix: read(owner, prefix).text };
}
