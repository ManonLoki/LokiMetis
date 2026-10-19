/** 真实文件快照证明预览成功、文档失败和参数拒绝均不会改写日志。 */
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { loadDocument, renderDocument } from "../../desktop-prepare-release/scripts/release_notes.mjs";
import { main, parseArguments } from "./inspect_release_notes.mjs";

const script = fileURLToPath(new URL("./inspect_release_notes.mjs", import.meta.url));
const document = { schemaVersion: 2, releases: [{ version: "v1.2.3", releaseDate: "2026-10-09", featureOptimizations: [{ "zh-CN": "读取日志", "en-US": "Read notes" }], bugFixes: [] }] };

/** 每个场景使用独立目录，比较内容与修改元数据而不受访问时间影响。 */
function fixture(t, text = JSON.stringify(document)) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "inspect-notes-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const file = path.join(root, "release-notes.json");
  fs.writeFileSync(file, text);
  const snapshot = () => fs.readdirSync(root).sort().map(name => { const target = path.join(root, name); const stat = fs.lstatSync(target); return { name, bytes: fs.readFileSync(target).toString("base64"), mtime: stat.mtimeMs, inode: stat.ino, mode: stat.mode }; });
  return { root, file, snapshot };
}

/** 注入输出仅收集文本，读取仍走真实原始库。 */
function invoke(args) { let out = "", error = ""; const status = main(args, { stdout: { write: text => { out += text; } }, stderr: { write: text => { error += text; } } }); return { status, out, error }; }

test("check and both rendered locales preserve files and match original library", t => {
  const f = fixture(t); const before = f.snapshot();
  assert.deepEqual(invoke(["check", "--file", f.file, "--expected-version", "1.2.3"]), { status: 0, out: "release-notes.valid=true retained=1\n", error: "" });
  for (const locale of ["zh-CN", "en-US"]) assert.deepEqual(invoke(["render", "--file", f.file, "--locale", locale]), { status: 0, out: `${renderDocument(loadDocument(f.file), locale)}\n`, error: "" });
  assert.deepEqual(f.snapshot(), before);
});

test("write commands and malformed arguments are rejected before document access", t => {
  const f = fixture(t); const before = f.snapshot();
  for (const args of [
    ["upsert", "--file", f.file], ["check", "--file", f.file, "--output", f.file],
    ["check", "--file", f.file, "--file", f.file], ["check", "--file"],
    ["check", "--file", f.file, "extra"], ["render", "--file", f.file],
    ["render", "--file", f.file, "--locale", "fr-FR"], ["render", "--file", f.file, "--expected-version", "1.2.3"],
  ]) { assert.throws(() => parseArguments(args)); assert.equal(invoke(args).status, 2); }
  assert.deepEqual(f.snapshot(), before);
});

test("version mismatch and original parser failures preserve source bytes", t => {
  const f = fixture(t); let before = f.snapshot();
  assert.equal(invoke(["check", "--file", f.file, "--expected-version", "1.2.4"]).status, 1); assert.deepEqual(f.snapshot(), before);
  for (const text of [
    '{"schemaVersion":2,"schemaVersion":2,"releases":[]}',
    JSON.stringify({ ...document, releases: [{ ...document.releases[0], featureOptimizations: [{ "zh-CN": "缺少翻译" }] }] }),
    JSON.stringify({ ...document, releases: [{ ...document.releases[0], version: "1.2.3" }] }),
    " ".repeat(1024 * 1024 + 1),
  ]) { fs.writeFileSync(f.file, text); before = f.snapshot(); assert.equal(invoke(["check", "--file", f.file]).status, 1); assert.deepEqual(f.snapshot(), before); }
});

test("missing files stay absent and actual CLI keeps read-only exit codes", t => {
  const f = fixture(t); const before = f.snapshot(); const missing = path.join(f.root, "missing.json");
  const run = args => spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
  assert.equal(run(["check", "--file", f.file]).status, 0);
  assert.equal(run(["check", "--file", missing]).status, 1);
  assert.equal(run(["upsert", "--file", missing]).status, 2);
  assert.equal(fs.existsSync(missing), false); assert.deepEqual(f.snapshot(), before);
});

test("symlink source is refused without touching either path", t => {
  const f = fixture(t); const link = path.join(f.root, "link.json");
  try { fs.symlinkSync(f.file, link); } catch (error) { if (["EPERM", "EACCES", "ENOSYS"].includes(error.code)) return t.skip(`symlink unavailable: ${error.code}`); throw error; }
  const before = f.snapshot(); assert.equal(invoke(["check", "--file", link]).status, 1); assert.deepEqual(f.snapshot(), before);
});

/** 脚本别名必须实际执行校验，不能静默退出 0。 */
test("script symlink invokes the read-only CLI instead of bypassing validation", t => {
  const f = fixture(t); const alias = path.join(f.root, "inspect-alias.mjs");
  try { fs.symlinkSync(script, alias); } catch (error) { if (["EPERM", "EACCES", "ENOSYS"].includes(error.code)) return t.skip("symlink unavailable: " + error.code); throw error; }
  const before = f.snapshot();
  const run = args => spawnSync(process.execPath, [alias, ...args], { encoding: "utf8" });
  assert.equal(run(["check", "--file", f.file, "--expected-version", "9.9.9"]).status, 1);
  assert.equal(run(["upsert", "--file", f.file]).status, 2);
  assert.match(run(["check", "--file", f.file]).stdout, /release-notes.valid=true/u);
  assert.deepEqual(f.snapshot(), before);
});
