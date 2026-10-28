// 隔离夹具沿真实验证入口覆盖分组补丁；不编译、不生成 OUT_DIR、不修改原包。
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { verify } from './verify-source.mjs';
import { readPatchInputs } from './patch-inputs.mjs';

/** 摘要来自夹具真实字节，用于断言补丁顺序而非绕过生产摘要核验。 */
function hash(value) { return createHash('sha256').update(value).digest('hex'); }

/** 构造共享函数、宏常量、字面量常量及内联补丁共同修改的最小完整原包。 */
function fixture(t, { finalText = 'd\n' } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'loki-gpui-source-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const directory = join(root, 'package');
  /** 写入独占临时路径；测试所有故障都不会触及仓库输入。 */
  const put = (path, text) => {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  };
  const originals = { 'src/lib.rs': '#![allow(dead_code)]\na\n', 'src/scene.rs': 'scene-a\n' };
  const manifest = Object.entries(originals).map(([path, text]) => `${hash(text)}  ${path}\n`).join('');
  for (const [path, text] of Object.entries(originals)) put(`package/upstream/${path}`, text);
  put('package/source-content.sha256', manifest);
  put('package/provenance.json', JSON.stringify({ package: 'fixture', source_manifest_sha256: hash(manifest),
    upstream_file_count: 2, upstream_bytes: Object.values(originals).reduce((sum, value) => sum + Buffer.byteLength(value), 0) }));
  put('package/patches/custom-root.before', '#![allow(dead_code)]\n');
  for (const [name, before, after] of [['first', 'a\n', 'b\n'], ['second', 'b\n', 'c\n'], ['inline', 'c\n', finalText]]) {
    put(`package/patches/${name}.before`, before);
    put(`package/patches/${name}.after`, after);
  }
  put('support/scene/scene.before', 'scene-a\n');
  put('support/scene/scene.after', 'scene-b\n');
  put('support/scene_patches.rs', `pub const fn all(path: &'static str) -> [Patch; 1] {
    [patch!("scene/", "scene", path)]
  }`);
  put('package/src/first.rs', `pub const PATCHES: &[Patch] = &[
    patch!("../patches/", "first", "src/lib.rs"),
  ];`);
  put('package/src/second.rs', `pub const PATCHES: &[Patch] = &[
    Patch { name: "second", path: "src/lib.rs",
      before: include_str!("../patches/second.before"),
      after: include_str!("../patches/second.after"), },
  ];`);
  put('package/build.rs', `#[path = "../support/scene_patches.rs"] mod scene;
    #[path = "src/first.rs"] mod first;
    #[path = "src/second.rs"] mod second;
    fn main() {
      let patches: Vec<Patch> = [
        &scene::all("src/scene.rs")[..], first::PATCHES, second::PATCHES,
        &[Patch { name: "inline", path: "src/lib.rs",
          before: include_str!("patches/inline.before"), after: include_str!("patches/inline.after"), }],
      ].concat();
      let spec = Spec { entry: "src/lib.rs", root_prefix: include_str!("patches/custom-root.before"),
        source_manifest_sha256: "${hash(manifest)}", patches: &patches,
        generated_hashes: &[("src/lib.rs", "${hash(finalText)}"), ("src/scene.rs", "${hash('scene-b\n')}")], };
    }`);
  /** 对指定维护输入做故障注入，保留未涉及的清单及摘要。 */
  const change = (path, transform) => put(path, transform(readFileSync(join(root, path), 'utf8')));
  return { directory, root, put, change, build: () => readFileSync(join(directory, 'build.rs'), 'utf8') };
}

// 成功必须复现跨组同文件依赖，并按声明文件自身目录定位 include_str 输入。
test('expands ordered shared, macro, literal and inline groups with the declared root prefix', t => {
  const f = fixture(t);
  assert.deepEqual(readPatchInputs(f.directory, f.build()).patches.map(patch => patch.name), ['scene', 'first', 'second', 'inline']);
  assert.equal(verify(f.directory).patches, 4);
});

// 原有直接内联列表不依赖 concat，保持未拆组后端的入口兼容。
test('keeps direct Spec patch arrays supported', t => {
  const f = fixture(t);
  f.change('package/build.rs', build => {
    const begin = build.indexOf('[\n        &scene');
    const end = build.indexOf('].concat();', begin);
    return build.replace('patches: &patches,', `patches: &${build.slice(begin, end + 1)},`);
  });
  assert.equal(verify(f.directory).patches, 4);
});

// 真实模板替换与 Rust replacen 一样按字面字节写入，不解释 JavaScript 的美元替换序列。
test('preserves literal dollar replacement bytes', t => {
  const f = fixture(t, { finalText: '$& $$ replacement\n' });
  assert.equal(verify(f.directory).patches, 4);
});

// 同文件后续片段依赖前组输出，反转或者漏掉实际组均须失败。
test('rejects reordered or missing groups', t => {
  for (const replacement of ['second::PATCHES, first::PATCHES,', 'first::PATCHES,']) {
    const f = fixture(t);
    f.change('package/build.rs', text => text.replace('first::PATCHES, second::PATCHES,', replacement));
    assert.throws(() => verify(f.directory), /ambiguous patch/);
  }
});

// 未来改用未实现的 Rust 表达式时失败关闭，不能像旧脚本一样只收集它认识的部分。
test('rejects unknown assembly and malformed patch expressions', t => {
  for (const replacement of ['first::patches()', 'missing::PATCHES', 'first::PATCHES.iter()']) {
    const f = fixture(t);
    f.change('package/build.rs', text => text.replace('first::PATCHES', replacement));
    assert.throws(() => verify(f.directory), /unsupported patch expression|missing patch module/);
  }
  const f = fixture(t);
  f.change('package/src/second.rs', text => text.replace('after: include_str!("../patches/second.after")', 'after: computed()'));
  assert.throws(() => verify(f.directory), /unsupported patch expression/);
});

// 注释和未被拼装引用的常量不构成有效补丁，即便包含看起来完整的声明。
test('ignores commented examples and unused declarations', t => {
  const f = fixture(t);
  f.change('package/src/first.rs', text => `/* outer /* patch!("missing/", "bad", "bad"); */ end */\n${text}
    // Patch { name: "bad", path: "bad", before: include_str!("missing"), after: include_str!("missing") }
    pub const UNUSED: &[Patch] = &[patch!("missing/", "unused", "bad")];`);
  assert.equal(verify(f.directory).patches, 4);
});

// 共享函数的参数与数组长度均是生产声明的一部分，不可另猜目标路径或数量。
test('rejects shared group length and unbound target drift', t => {
  const f = fixture(t);
  f.change('support/scene_patches.rs', text => text.replace('[Patch; 1]', '[Patch; 2]'));
  assert.throws(() => verify(f.directory), /patch group length drift/);
  const g = fixture(t);
  g.change('support/scene_patches.rs', text => text.replace('"scene", path', '"scene", wrong'));
  assert.throws(() => verify(g.directory), /unbound patch target/);
});

// 宏和字面量模板任一字节错误均沿完整生产路径被拒绝，而非只校验分组数量。
test('rejects changed group fragments and missing includes', t => {
  const f = fixture(t);
  f.put('package/patches/first.before', 'absent\n');
  assert.throws(() => verify(f.directory), /ambiguous patch: first/);
  const g = fixture(t);
  g.put('package/patches/inline.after', 'changed\n');
  assert.throws(() => verify(g.directory), /generated output drift: src\/lib.rs/);
  const h = fixture(t);
  rmSync(join(h.directory, 'patches/second.after'));
  assert.throws(() => verify(h.directory), /ENOENT/);
});

// 组目标不能逃出原包，也不能靠一个不存在的目标绕开最终文件集校验。
test('rejects unsafe and missing generated targets', t => {
  for (const [path, error] of [['../outside.rs', /unsafe source path/], ['src/missing.rs', /missing patch target/]]) {
    const f = fixture(t);
    f.change('package/src/second.rs', text => text.replace('path: "src/lib.rs"', `path: "${path}"`));
    assert.throws(() => verify(f.directory), error);
  }
});

// 允许真实共享维护目录，但 include 或模块不能借父路径越过 vendor 根。
test('rejects patch input traversal outside the shared root', t => {
  const f = fixture(t);
  f.change('package/src/second.rs', text => text.replace('../patches/second.after', '../../../outside.after'));
  assert.throws(() => verify(f.directory), /unsafe patch input/);
});

// 链接字节正确也不能替代登记的实际文件或目录，避免 include 来源被重定向。
test('rejects symlink fragment and module parents', { skip: process.platform === 'win32' }, t => {
  const f = fixture(t);
  const path = join(f.directory, 'patches/first.after');
  rmSync(path);
  symlinkSync('second.after', path);
  assert.throws(() => verify(f.directory), /real directories and regular files/);
  const g = fixture(t);
  rmSync(join(g.directory, 'src'), { recursive: true });
  mkdirSync(join(g.root, 'other'));
  symlinkSync('../other', join(g.directory, 'src'));
  assert.throws(() => verify(g.directory), /real directories and regular files/);
});

// 修改文件必须与固定生成摘要一一对应，补丁覆盖扩大不能只靠部分摘要通过。
test('rejects omitted output hashes and original byte drift', t => {
  const f = fixture(t);
  f.change('package/build.rs', text => text.replace(`, ("src/scene.rs", "${hash('scene-b\n')}")`, ''));
  assert.throws(() => verify(f.directory), /generated file set drift/);
  const g = fixture(t);
  g.put('package/upstream/src/scene.rs', 'scene-x\n');
  assert.throws(() => verify(g.directory), /source byte drift/);
});

// 仓库的全部五个包沿真实 CLI 独立验证，覆盖共享 scene 与已拆分的所有现行组。
test('verifies all real package manifests, patch groups and generated hashes', () => {
  const support = dirname(fileURLToPath(import.meta.url));
  const names = ['gpui-pre', 'gpui-pre-apple', 'gpui-pre-windows', 'gpui-pre-wgpu', 'gpui-pre-macos'];
  const output = execFileSync(process.execPath, [join(support, 'verify-source.mjs'), ...names.map(name => resolve(support, '..', name))], { encoding: 'utf8' });
  const reports = output.trim().split('\n').map(line => JSON.parse(line));
  assert.deepEqual(reports.map(({ package: name, patches }) => [name, patches]), names.map((name, i) => [name, [174, 63, 9, 9, 23][i]]));
  assert.equal(reports.find(report => report.package === 'gpui-pre-wgpu').font_fixtures.files, 5);
});
