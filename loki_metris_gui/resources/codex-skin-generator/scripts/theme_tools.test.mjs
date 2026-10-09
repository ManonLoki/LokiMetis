import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { auditSkin } from "./audit_component_map.mjs";
import { buildPreview } from "./render_preview.mjs";
import { parseModeThemeCss } from "./theme_css.mjs";
import { validateSkin } from "./validate_skin.mjs";

const scriptsDirectory = path.dirname(fileURLToPath(import.meta.url));
const skillDirectory = path.resolve(scriptsDirectory, "..");
const repositoryDirectory = path.resolve(skillDirectory, "../../..");
const resourceDirectory = path.join(repositoryDirectory, "loki_metis_gui/resources");
const backgroundPath = path.join(resourceDirectory, "theme-template/background.png");
const runtimeDirectory = path.join(resourceDirectory, "theme-runtime");

// 所有目录均由当前测试独立拥有，退出时清理，不依赖用户资源库状态。
async function fixture(t, modes = "both", name = "测试主题") {
  const root = await mkdtemp(path.join(os.tmpdir(), "loki-skin-tools-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const directory = path.join(root, "test-skin");
  const result = spawnSync(process.execPath, [
    path.join(scriptsDirectory, "create_skin.mjs"), "--output", directory,
    "--id", "test-skin", "--name", name, "--description", "独立测试主题",
    "--author", "测试作者", "--hero-image", backgroundPath, "--color-modes", modes,
  ], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return { root, directory };
}

// 兼容皮肤仍消费其包内 CSS 与脚本，图片只取仓库内既有真实资源。
async function legacyFixture(t) {
  const root = await mkdtemp(path.join(os.tmpdir(), "loki-legacy-tools-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const directory = path.join(root, "test-skin");
  await mkdir(directory);
  const manifest = JSON.parse(await readFile(path.join(skillDirectory, "assets/skin-template/theme.json"), "utf8"));
  Object.assign(manifest, { id: "test-skin", name: "测试主题", description: "兼容测试主题", author: "测试作者" });
  await writeFile(path.join(directory, "theme.json"), JSON.stringify(manifest));
  for (const file of ["dream-skin.css", "renderer-inject.js"]) await copyFile(path.join(skillDirectory, "assets/skin-template", file), path.join(directory, file));
  for (const file of ["qq2007-sky.png", "avatar.png", "qqshow.jpg"]) await copyFile(path.join(resourceDirectory, "builtin-skins/minecraft", file), path.join(directory, file));
  return { root, directory, manifest };
}

// 只改当前测试的清单，保留生成器建立的其他真实文件。
async function changeManifest(directory, change) {
  const target = path.join(directory, "theme.json");
  const manifest = JSON.parse(await readFile(target, "utf8"));
  change(manifest);
  await writeFile(target, JSON.stringify(manifest));
}

test("generates_and_validates_each_supported_color_mode", async (t) => {
  for (const modes of ["both", "light", "dark"]) {
    const { directory } = await fixture(t, modes);
    assert.deepEqual((await validateSkin(directory)).errors, []);
    const errors = await auditSkin(skillDirectory, directory);
    assert.deepEqual(errors, []);
  }
});

test("generator_counts_unicode_scalars_like_the_host", async (t) => {
  const { directory } = await fixture(t, "both", "🌱".repeat(80));
  assert.deepEqual((await validateSkin(directory)).errors, []);
});

test("rejects_non_decimal_or_missing_CSS_integer_values", () => {
  for (const value of ["px", "0x10px", "1e1px", "1.0px", "-1px", "33px"]) {
    assert.throws(() => parseModeThemeCss(`html.codex-dream-skin[data-dream-shell="light"] { --skin-radius: ${value}; }`, "light"));
  }
  for (const value of ["0px", "32px"]) {
    assert.equal(parseModeThemeCss(`html.codex-dream-skin[data-dream-shell="light"] { --skin-radius: ${value}; }`, "light").declarations["--skin-radius"], value);
  }
});

test("rejects_unsafe_image_names_and_unclosed_mode_comments", () => {
  for (const image of ["bad:name.png", "bad\0name.png", "../background.png", "bad\nname.png"]) {
    assert.throws(() => parseModeThemeCss(`html.codex-dream-skin[data-dream-shell="light"] { --skin-background-image: url("${image}"); }`, "light"));
  }
  assert.throws(() => parseModeThemeCss('html.codex-dream-skin[data-dream-shell="light"] { --skin-radius: 2px; } /*', "light"), /注释/);
});

test("validates_effective_mode_palettes_with_incremental_overrides", async (t) => {
  const { directory } = await fixture(t);
  await writeFile(path.join(directory, "theme.dark.css"), 'html.codex-dream-skin[data-dream-shell="dark"] { --skin-text: #E7F4F5; }');
  assert.ok((await validateSkin(directory)).errors.some((error) => error.includes("dark 配色") && error.includes("对比度")));
});

test("marks_transparent_palette_contrast_for_preview_review", async (t) => {
  const { directory } = await fixture(t);
  const target = path.join(directory, "theme.dark.css");
  const css = await readFile(target, "utf8");
  await writeFile(target, css.replace("#F4FAFF", "#FFFFFF00"));
  const report = await validateSkin(directory);
  assert.deepEqual(report.errors, []);
  assert.ok(report.warnings.some((warning) => warning.includes("透明颜色") && warning.includes("预览")));
});

test("reports_null_requirements_without_losing_the_validation_report", async (t) => {
  const { directory } = await fixture(t);
  await changeManifest(directory, (manifest) => { manifest.appearance.requirements = { light: null }; });
  const report = await validateSkin(directory);
  assert.ok(report.errors.some((error) => error.includes("appearance.requirements.light 必须是对象")));
});

test("rejects_invalid_UTF8_before_JSON_replacement_can_hide_it", async (t) => {
  const { directory } = await fixture(t);
  const target = path.join(directory, "theme.json");
  const bytes = await readFile(target);
  const marker = bytes.indexOf(Buffer.from("测试主题"));
  assert.ok(marker >= 0);
  bytes[marker] = 0xff;
  await writeFile(target, bytes);
  assert.ok((await validateSkin(directory)).errors.some((error) => error.includes("有效 UTF-8")));
});

test("rejects_extra_mode_files", async (t) => {
  const { directory } = await fixture(t, "light");
  await writeFile(path.join(directory, "theme.dark.css"), 'html.codex-dream-skin[data-dream-shell="dark"] { --skin-radius: 2px; }');
  assert.ok((await validateSkin(directory)).errors.some((error) => error.includes("未声明文件") && error.includes("theme.dark.css")));
});

test("rejects_symlink_assets_when_the_host_allows_test_links", async (t) => {
  const { directory } = await fixture(t, "light");
  await rm(path.join(directory, "background.png"));
  try { await symlink(backgroundPath, path.join(directory, "background.png")); }
  catch (error) {
    if (process.platform === "win32" && ["EPERM", "EACCES"].includes(error.code)) {
      t.skip("当前 Windows 用户不能创建符号链接；需要开发者模式或相应宿主权限。");
      return;
    }
    throw error;
  }
  assert.ok((await validateSkin(directory)).errors.some((error) => error.includes("链接")));
});

test("preview_respects_supported_modes_and_rejects_invalid_packages", async (t) => {
  const { directory } = await fixture(t, "light");
  const html = await buildPreview(directory);
  assert.match(html, /data-dream-shell="light"/);
  assert.ok(!html.includes('data-mode="dark"'));
  assert.ok(!html.includes('id="codex-dream-skin-chrome"'));
  await assert.rejects(() => buildPreview(directory, "chat", "dark"), /不支持 dark/);
  await writeFile(path.join(directory, "unexpected.js"), "throw new Error('invalid package');");
  await assert.rejects(() => buildPreview(directory), /正式校验/);
});

test("historical_v3_preview_defaults_to_its_light_capability", async () => {
  const html = await buildPreview(path.join(resourceDirectory, "theme-template"));
  assert.match(html, /data-dream-shell="light"/);
  assert.ok(!html.includes('data-mode="dark"'));
});

test("preview_selects_a_mode_background_and_preserves_other_surfaces", async (t) => {
  const { directory } = await fixture(t);
  await copyFile(backgroundPath, path.join(directory, "dark.png"));
  const target = path.join(directory, "theme.dark.css");
  await writeFile(target, (await readFile(target, "utf8")).replace("{", '{ --skin-background-image: url("dark.png");'));
  const html = await buildPreview(directory, "settings", "dark");
  assert.match(html, /data-dream-surface="settings"/);
  assert.match(html, /"modeBackgrounds":\{"dark":"data:image\/png;base64,/);
  assert.match(html, /data-surface="pull-requests"/);
});

test("component_audit_keeps_legacy_and_pure_runtime_paths_distinct", async (t) => {
  const { directory } = await fixture(t);
  assert.deepEqual(await auditSkin(skillDirectory, directory, { runtimeDirectory }), []);
  const legacy = await legacyFixture(t);
  assert.deepEqual(await auditSkin(skillDirectory, legacy.directory), []);
  await assert.rejects(() => auditSkin(skillDirectory, directory, { runtimeDirectory: path.join(directory, "missing-runtime") }), /--runtime-dir/);
});

test("component_audit_rejects_direct_plugin_region_fill_but_accepts_children", async (t) => {
  const { directory } = await legacyFixture(t);
  const target = path.join(directory, "dream-skin.css");
  const original = await readFile(target, "utf8");
  await writeFile(target, `${original}\nhtml.codex-dream-skin [id^="plugins-search-"] > [role="listitem"] { background: #112233; }`);
  assert.deepEqual(await auditSkin(skillDirectory, directory), []);
  await writeFile(target, `${original}\nhtml.codex-dream-skin [id^="plugins-search-"] { background: #112233; }`);
  assert.ok((await auditSkin(skillDirectory, directory)).some((error) => error.includes("分区锚点不得直接设置")));
});

test("legacy_preview_escapes_case_insensitive_HTML_end_tags", async (t) => {
  const { directory, manifest } = await legacyFixture(t);
  const target = path.join(directory, "dream-skin.css");
  await writeFile(target, `${await readFile(target, "utf8")}\n/* </STYLE><script id="escape-style"> */`);
  manifest.colors.dark.line = '</SCRIPT><script id="escape-script">';
  await writeFile(path.join(directory, "theme.json"), JSON.stringify(manifest));
  const html = await buildPreview(directory);
  assert.equal([...html.matchAll(/<\/style/gi)].length, 1);
  assert.equal([...html.matchAll(/<\/script/gi)].length, 1);
  assert.ok(html.includes('\\u003c/SCRIPT>'));
});

test("pure_theme_component_audit_CLI_executes_in_a_new_process", async (t) => {
  const { directory } = await fixture(t);
  const result = spawnSync(process.execPath, [path.join(scriptsDirectory, "audit_component_map.mjs"), directory], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /选择器 Map 审计通过/);
});
