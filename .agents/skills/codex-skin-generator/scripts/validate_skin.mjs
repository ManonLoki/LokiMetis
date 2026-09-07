#!/usr/bin/env node
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import vm from "node:vm";
import { SKILL_DIR, SURFACES, parseArgs } from "./lib.mjs";
import { auditSkin } from "./audit_component_map.mjs";
import { parseModeThemeCss, parseThemeCss } from "./theme_css.mjs";
import { analyzeCssPerformance, formatPerformanceMetrics } from "./css_performance.mjs";

const LEGACY_REQUIRED_FILES = new Set(["theme.json", "dream-skin.css", "renderer-inject.js", "qq2007-sky.png", "avatar.png", "qqshow.jpg"]);
const PLACEHOLDERS = new Map([
  ["__DREAM_SKIN_CSS_JSON__", '""'],
  ["__DREAM_SKIN_ART_JSON__", '"data:image/png;base64,"'],
  ["__DREAM_SKIN_AVATAR_JSON__", '"data:image/png;base64,"'],
  ["__DREAM_SKIN_FRIENDS_JSON__", '"data:image/jpeg;base64,"'],
  ["__DREAM_SKIN_THEME_JSON__", "{}"],
  ["__DREAM_SKIN_VERSION_JSON__", '"validation"'],
]);
const ID_PATTERN = /^[a-z0-9_-]{1,64}$/;
const COLOR_KEYS = ["background", "panel", "panelAlt", "accent", "accentAlt", "text", "muted", "line"];
const PANEL_ANCHORS = new Set(["left-top", "right-top", "left-bottom", "right-bottom"]);
const PANEL_SURFACES = new Set(SURFACES);

class Report {
  errors = [];
  warnings = [];
  notes = [];
  error(message) { this.errors.push(message); }
  warn(message) { this.warnings.push(message); }
  note(message) { this.notes.push(message); }
}

async function textFile(target, report) {
  try {
    const info = await stat(target);
    if (!info.isFile() || info.size === 0 || info.size > 2 * 1024 * 1024) report.error(`文本文件必须非空且不超过 2 MiB：${path.basename(target)}`);
    return await readFile(target, "utf8");
  } catch (error) {
    report.error(`无法以 UTF-8 读取 ${path.basename(target)}：${error.message}`);
    return "";
  }
}

function validText(value, maximum) {
  return typeof value === "string" && Boolean(value.trim()) && value.length <= maximum;
}

function parseHex(value) {
  const match = typeof value === "string" ? value.trim().match(/^#([0-9a-f]{6})(?:[0-9a-f]{2})?$/i) : null;
  if (!match) return null;
  return [0, 2, 4].map((index) => Number.parseInt(match[1].slice(index, index + 2), 16));
}

function luminance(rgb) {
  const channels = rgb.map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function contrast(first, second) {
  const firstRgb = parseHex(first);
  const secondRgb = parseHex(second);
  if (!firstRgb || !secondRgb) return null;
  const values = [luminance(firstRgb), luminance(secondRgb)].sort((a, b) => b - a);
  return (values[0] + 0.05) / (values[1] + 0.05);
}

function validatePalette(name, palette, report) {
  if (!palette || typeof palette !== "object" || Array.isArray(palette)) {
    report.error(`colors.${name} 必须是颜色角色对象。`);
    return;
  }
  for (const key of COLOR_KEYS) if (!validText(palette[key], 200)) report.error(`colors.${name}.${key} 必须是非空 CSS 颜色。`);
  for (const [foreground, background, label, minimum] of [["text", "background", "正文/背景", 4.5], ["text", "panel", "正文/面板", 4.5]]) {
    const ratio = contrast(palette[foreground], palette[background]);
    if (ratio !== null && ratio < minimum) report.error(`${name} 配色的${label}对比度为 ${ratio.toFixed(2)}:1，低于 ${minimum}:1。`);
  }
  for (const [foreground, background, label] of [["muted", "panel", "弱文本/面板"], ["accent", "panel", "强调色/面板"]]) {
    const ratio = contrast(palette[foreground], palette[background]);
    if (ratio !== null && ratio < 3) report.warn(`${name} 配色的${label}对比度仅 ${ratio.toFixed(2)}:1，请在预览中复核。`);
  }
}

function validateColors(colors, report) {
  if (!colors || typeof colors !== "object" || Array.isArray(colors)) {
    report.error("colors 必须是颜色角色对象。");
    return;
  }
  if ("dark" in colors || "light" in colors) {
    validatePalette("dark", colors.dark, report);
    validatePalette("light", colors.light, report);
  } else validatePalette("default", colors, report);
}

function validNumber(value, minimum, maximum) {
  return typeof value === "number" && Number.isFinite(value) && value >= minimum && value <= maximum;
}

async function validatePanels(extensions, directory, report) {
  if (extensions === undefined || extensions === null) return;
  if (typeof extensions !== "object" || Array.isArray(extensions)) {
    report.error("extensions 必须是对象。");
    return;
  }
  const panels = extensions.photoPanels ?? [];
  if (!Array.isArray(panels) || panels.length > 2) {
    report.error("extensions.photoPanels 必须是最多包含两个对象的数组。");
    return;
  }
  const ids = new Set();
  const slots = new Set();
  for (const [index, panel] of panels.entries()) {
    const prefix = `extensions.photoPanels[${index}]`;
    if (!panel || typeof panel !== "object" || Array.isArray(panel)) {
      report.error(`${prefix} 必须是对象。`);
      continue;
    }
    if (typeof panel.id !== "string" || !ID_PATTERN.test(panel.id) || ids.has(panel.id)) report.error(`${prefix}.id 必须是唯一的小写标识。`);
    else ids.add(panel.id);
    if (typeof panel.enabled !== "boolean") report.error(`${prefix}.enabled 必须是布尔值。`);
    if (!["profile", "gallery"].includes(panel.assetSlot) || slots.has(panel.assetSlot)) report.error(`${prefix}.assetSlot 必须是唯一的 profile 或 gallery。`);
    else slots.add(panel.assetSlot);
    if (!PANEL_ANCHORS.has(panel.anchor)) report.error(`${prefix}.anchor 不是支持的锚点。`);
    if (!["contain", "cover"].includes(panel.fit)) report.error(`${prefix}.fit 必须是 contain 或 cover。`);
    if (!panel.offset || !["x", "y"].every((key) => validNumber(panel.offset[key], 0, 2000))) report.error(`${prefix}.offset 必须包含 0 到 2000 的 x 和 y。`);
    if (!panel.size || !["width", "height"].every((key) => validNumber(panel.size[key], 80, 1200))) report.error(`${prefix}.size 必须包含 80 到 1200 的 width 和 height。`);
    if (!Array.isArray(panel.visibleOn) || panel.visibleOn.length === 0 || panel.visibleOn.some((surface) => !PANEL_SURFACES.has(surface))) report.error(`${prefix}.visibleOn 必须是受支持页面标识的非空数组。`);
    if (!validNumber(panel.minViewportWidth, 320, 4000)) report.error(`${prefix}.minViewportWidth 必须在 320 到 4000 之间。`);
    for (const [field, maximum] of [["title", 200], ["caption", 500]]) if (typeof panel[field] !== "string" || panel[field].length > maximum) report.error(`${prefix}.${field} 必须是不超过 ${maximum} 个字符的字符串。`);
    if (panel.enabled && ["profile", "gallery"].includes(panel.assetSlot)) {
      const asset = path.join(directory, panel.assetSlot === "profile" ? "avatar.png" : "qqshow.jpg");
      const info = await stat(asset).catch(() => null);
      if (info?.isFile() && info.size < 256) report.warn(`${prefix} 已启用，但资源 ${path.basename(asset)} 看起来仍是占位图片。`);
    }
  }
}

function validatePreview(preview, report) {
  if (!preview || typeof preview !== "object" || Array.isArray(preview)) {
    report.error("previewContent 必须是对象，集中保存预览展示文本。");
    return;
  }
  const required = ["navNewTask", "navProjects", "navSettings", "homeTitle", "homeSubtitle", "suggestionOne", "suggestionTwo", "projectName", "userMessage", "assistantMessage", "composerPlaceholder"];
  const optional = ["navPullRequests", "navSites", "navAutomations", "navPlugins", "pullRequestsTitle", "sitesTitle", "automationsTitle", "pluginsTitle", "settingsTitle", "emptyStateTitle", "emptyStateAction"];
  for (const field of required) if (!validText(preview[field], 500)) report.error(`previewContent.${field} 必须是 1 到 500 个字符的非空字符串。`);
  for (const field of optional) if (preview[field] !== undefined && !validText(preview[field], 500)) report.error(`previewContent.${field} 存在时必须是 1 到 500 个字符的非空字符串。`);
}

async function validateManifest(directory, report) {
  const source = await textFile(path.join(directory, "theme.json"), report);
  let manifest;
  try { manifest = JSON.parse(source); } catch (error) { report.error(`theme.json 不是有效 JSON：${error.message}`); return {}; }
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) { report.error("theme.json 顶层必须是对象。"); return {}; }
  if (manifest.schemaVersion === 2) {
    report.error("schemaVersion 2 过渡主题已停用，请转换为 schemaVersion 3 新主题。");
    return manifest;
  }
  if (manifest.schemaVersion === 3) {
    await validateCssVariableTheme(directory, manifest, report);
    if (typeof manifest.id === "string" && path.basename(directory) !== manifest.id) report.warn(`目录名“${path.basename(directory)}”与 id“${manifest.id}”不同；导入后目录会以 id 命名。`);
    return manifest;
  }
  if (manifest.schemaVersion !== 1) report.error("schemaVersion 必须为旧皮肤的 1 或新主题的 3。");
  if (typeof manifest.id !== "string" || !ID_PATTERN.test(manifest.id)) report.error("id 只能包含 1 到 64 个小写字母、数字、连字符或下划线。");
  for (const field of ["name", "author"]) if (!validText(manifest[field], 80)) report.error(`${field} 必须是 1 到 80 个字符的非空字符串。`);
  if (!validText(manifest.description, 500)) report.error("description 必须是 1 到 500 个字符的非空字符串。");
  if (manifest.image !== "qq2007-sky.png") report.error("image 必须为 qq2007-sky.png。");
  if (!manifest.friendCards || typeof manifest.friendCards !== "object") report.error("friendCards 必须是对象。");
  else {
    if (manifest.friendCards.profileImage !== "avatar.png") report.error("friendCards.profileImage 必须为 avatar.png。");
    if (manifest.friendCards.listImage !== "qqshow.jpg") report.error("friendCards.listImage 必须为 qqshow.jpg。");
  }
  validatePreview(manifest.previewContent, report);
  validateColors(manifest.colors, report);
  await validatePanels(manifest.extensions, directory, report);
  if (typeof manifest.id === "string" && path.basename(directory) !== manifest.id) report.warn(`目录名“${path.basename(directory)}”与 id“${manifest.id}”不同；导入后目录会以 id 命名。`);
  return manifest;
}

function exactKeys(value, expected, label, report, optional = []) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    report.error(`${label} 必须是对象。`);
    return false;
  }
  const actual = Object.keys(value).sort();
  const required = new Set(expected);
  const allowed = new Set([...expected, ...optional]);
  const missing = expected.filter((key) => !actual.includes(key));
  const extras = actual.filter((key) => !allowed.has(key));
  if (missing.length || extras.length) {
    report.error(`${label} 必填字段为：${[...required].sort().join(", ")}；仅额外允许：${optional.join(", ") || "无"}。`);
    return false;
  }
  return true;
}

function validateComment(value, label, report) {
  if (value !== undefined && !validText(value, 2000)) report.error(`${label} 必须是 1 到 2000 个字符的非空字符串。`);
}

async function validateCssVariableTheme(directory, manifest, report) {
  exactKeys(manifest, ["schemaVersion", "type", "id", "name", "description", "author"], "theme.json", report, ["$comment", "appearance"]);
  validateComment(manifest.$comment, "theme.json.$comment", report);
  if (manifest.type !== "theme") report.error('type 必须为 "theme"。');
  if (typeof manifest.id !== "string" || !ID_PATTERN.test(manifest.id)) report.error("id 只能包含 1 到 64 个小写字母、数字、连字符或下划线。");
  for (const field of ["name", "author"]) if (!validText(manifest[field], 80)) report.error(`${field} 必须是 1 到 80 个字符的非空字符串。`);
  if (!validText(manifest.description, 500)) report.error("description 必须是 1 到 500 个字符的非空字符串。");
  const modes = validateAppearance(manifest.appearance, report);
  try {
    manifest.__themeCss = parseThemeCss(await textFile(path.join(directory, "theme.css"), report), { explicitAppearance: manifest.appearance !== undefined });
    manifest.__modeCssFiles = [];
    manifest.__modeBackgrounds = [];
    if (manifest.appearance !== undefined) {
      for (const mode of modes) {
        const file = `theme.${mode}.css`;
        const parsed = parseModeThemeCss(await textFile(path.join(directory, file), report), mode, file);
        if (parsed.background) manifest.__modeBackgrounds.push(parsed.background);
        manifest.__modeCssFiles.push(file);
      }
    }
  } catch (error) {
    report.error(error.message);
  }
}

function validateAppearance(appearance, report) {
  if (appearance === undefined) return ["light"];
  if (!exactKeys(appearance, ["supportedColorModes"], "appearance", report, ["requirements"])) return [];
  const modes = appearance.supportedColorModes;
  if (!Array.isArray(modes) || modes.length < 1 || modes.length > 2 || new Set(modes).size !== modes.length || modes.some((mode) => !["light", "dark"].includes(mode))) {
    report.error("appearance.supportedColorModes 必须是不重复且非空的 light、dark 列表。");
    return [];
  }
  if (appearance.requirements !== undefined) {
    exactKeys(appearance.requirements, [], "appearance.requirements", report, ["light", "dark"]);
    for (const mode of ["light", "dark"]) {
      const requirement = appearance.requirements?.[mode];
      if (requirement === undefined) continue;
      if (!modes.includes(mode)) report.error(`appearance.requirements.${mode} 只能用于已声明支持的模式。`);
      validateAppearanceRequirement(requirement, `appearance.requirements.${mode}`, report);
    }
  }
  return modes;
}

function validateAppearanceRequirement(requirement, label, report) {
  const fields = ["codeThemeId", "accent", "surface", "ink", "contrast", "opaqueWindows", "uiFont", "codeFont", "semanticColors"];
  exactKeys(requirement, [], label, report, fields);
  for (const field of ["codeThemeId", "uiFont", "codeFont"]) {
    if (requirement[field] !== undefined && !validText(requirement[field], 160)) report.error(`${label}.${field} 必须是 1 到 160 个字符的非空字符串。`);
  }
  for (const field of ["accent", "surface", "ink"]) {
    if (requirement[field] !== undefined && !/^#[0-9a-f]{6}$/i.test(requirement[field])) report.error(`${label}.${field} 必须为 #RRGGBB。`);
  }
  if (requirement.contrast !== undefined && (!Number.isInteger(requirement.contrast) || requirement.contrast < 0 || requirement.contrast > 100)) report.error(`${label}.contrast 必须为 0 到 100 的整数。`);
  if (requirement.opaqueWindows !== undefined && typeof requirement.opaqueWindows !== "boolean") report.error(`${label}.opaqueWindows 必须为布尔值。`);
  if (requirement.semanticColors !== undefined) {
    exactKeys(requirement.semanticColors, [], `${label}.semanticColors`, report, ["diffAdded", "diffRemoved", "skill"]);
    for (const field of ["diffAdded", "diffRemoved", "skill"]) {
      const value = requirement.semanticColors?.[field];
      if (value !== undefined && !/^#[0-9a-f]{6}$/i.test(value)) report.error(`${label}.semanticColors.${field} 必须为 #RRGGBB。`);
    }
  }
}

async function validateImage(target, expected, report) {
  try {
    const info = await stat(target);
    if (!info.isFile() || info.size === 0 || info.size > 16 * 1024 * 1024) report.error(`图片必须非空且不超过 16 MiB：${path.basename(target)}`);
    const header = (await readFile(target)).subarray(0, 12);
    if (expected === "png" && !header.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))) report.error(`${path.basename(target)} 不是 PNG 数据。`);
    if (expected === "jpeg" && !header.subarray(0, 3).equals(Buffer.from([0xff, 0xd8, 0xff]))) report.error(`${path.basename(target)} 不是 JPEG 数据。`);
  } catch (error) { report.error(`无法读取图片 ${path.basename(target)}：${error.message}`); }
}

function balancedCss(css) {
  let depth = 0;
  let quote = "";
  for (let index = 0; index < css.length; index += 1) {
    const char = css[index];
    if (!quote && char === "/" && css[index + 1] === "*") {
      const end = css.indexOf("*/", index + 2);
      if (end < 0) return false;
      index = end + 1;
    } else if (quote) {
      if (char === "\\") index += 1;
      else if (char === quote) quote = "";
    } else if (char === '"' || char === "'") quote = char;
    else if (char === "{") depth += 1;
    else if (char === "}" && --depth < 0) return false;
  }
  return depth === 0 && !quote;
}

async function validateCss(directory, report) {
  const css = await textFile(path.join(directory, "dream-skin.css"), report);
  if (!balancedCss(css)) {
    report.error("dream-skin.css 的注释、引号或花括号不平衡。");
    return;
  }
  if (!css.includes("html.codex-dream-skin")) report.error("CSS 必须使用 html.codex-dream-skin 限定宿主覆盖范围。");
  if (/(?:\/Users\/|[A-Za-z]:\\Users\\|file:\/\/)/.test(css)) report.error("CSS 包含本机绝对路径或 file URL。");
  const performance = analyzeCssPerformance(css);
  report.note(formatPerformanceMetrics(performance.metrics));
  for (const error of performance.errors) report.error(error);
  for (const warning of performance.warnings) report.warn(warning);
}

async function validateJavaScript(directory, report) {
  let source = await textFile(path.join(directory, "renderer-inject.js"), report);
  for (const [placeholder, replacement] of PLACEHOLDERS) {
    if (!source.includes(placeholder)) report.error(`renderer-inject.js 缺少载荷占位符 ${placeholder}。`);
    source = source.replaceAll(placeholder, replacement);
  }
  const unknown = [...new Set(source.match(/__DREAM_SKIN_[A-Z_]+__/g) ?? [])].sort();
  if (unknown.length) report.error(`存在未知载荷占位符：${unknown.join(", ")}`);
  for (const required of ["__CODEX_DREAM_SKIN_STATE__", "cleanup", "codex-dream-skin-style"]) if (!source.includes(required)) report.error(`renderer-inject.js 缺少生命周期标记：${required}`);
  if (/\b(?:eval|Function)\s*\(/.test(source)) report.error("renderer-inject.js 不得使用 eval 或 Function 构造器。");
  if (/\b(?:fetch|XMLHttpRequest|WebSocket)\s*\(/.test(source)) report.warn("注入脚本包含网络 API；确认皮肤没有远程依赖或遥测。");
  if (/(?:\/Users\/|[A-Za-z]:\\Users\\|file:\/\/)/.test(source)) report.error("注入脚本包含本机绝对路径或 file URL。");
  try { new vm.Script(source, { filename: "renderer-inject.js" }); } catch (error) { report.error(`renderer-inject.js 语法检查失败：${error.message.split("\n")[0]}`); }
}

async function main() {
  const { positional } = parseArgs(process.argv.slice(2));
  if (positional.length !== 1) throw new Error("用法：node scripts/validate_skin.mjs <皮肤目录>");
  const directory = path.resolve(positional[0]);
  const info = await stat(directory).catch(() => null);
  if (!info?.isDirectory()) throw new Error(`目录不存在：${directory}`);
  const report = new Report();
  const entries = await readdir(directory, { withFileTypes: true });
  const actual = new Set(entries.filter((entry) => entry.isFile()).map((entry) => entry.name));
  const manifest = await validateManifest(directory, report);
  if (manifest.schemaVersion === 3 && manifest.type === "theme") {
    if (entries.some((entry) => !entry.isFile())) report.error("CSS 变量主题目录只能包含根目录文件，不能包含子目录或链接。");
    const referenced = [manifest.__themeCss?.preview, manifest.__themeCss?.background, ...(manifest.__modeBackgrounds ?? [])]
      .filter((value) => typeof value === "string");
    const expected = new Set(["theme.json", "theme.css", ...(manifest.__modeCssFiles ?? []), ...referenced]);
    const missing = [...expected].filter((name) => !actual.has(name)).sort();
    const extras = [...actual].filter((name) => !expected.has(name)).sort();
    if (missing.length) report.error(`缺少主题变量引用文件：${missing.join(", ")}`);
    if (extras.length) report.error(`CSS 变量主题包含未声明文件：${extras.join(", ")}`);
    await Promise.all(
      [...new Set(referenced)]
        .filter((name) => actual.has(name))
        .map((file) => validateImage(path.join(directory, file), /\.png$/i.test(file) ? "png" : "jpeg", report)),
    );
  } else {
    const missing = [...LEGACY_REQUIRED_FILES].filter((name) => !actual.has(name)).sort();
    if (missing.length) report.error(`缺少必需文件：${missing.join(", ")}`);
    const extras = [...actual].filter((name) => !LEGACY_REQUIRED_FILES.has(name)).sort();
    if (extras.length) report.warn(`存在非运行必需文件：${extras.join(", ")}`);
    if (!missing.length) {
    const [, , , , , auditResult] = await Promise.allSettled([
      validateImage(path.join(directory, "qq2007-sky.png"), "png", report),
      validateImage(path.join(directory, "avatar.png"), "png", report),
      validateImage(path.join(directory, "qqshow.jpg"), "jpeg", report),
      validateCss(directory, report),
      validateJavaScript(directory, report),
      auditSkin(SKILL_DIR, directory),
    ]);
    if (auditResult.status === "fulfilled") {
      for (const error of auditResult.value) report.error(error);
    } else {
      report.error(`无法完成选择器 Map 审计：${auditResult.reason.message}`);
    }
    }
  }
  for (const note of report.notes) console.log(note);
  for (const warning of report.warnings) console.log(`警告：${warning}`);
  for (const error of report.errors) console.error(`错误：${error}`);
  if (report.errors.length) throw new Error(`验证失败：${report.errors.length} 个错误，${report.warnings.length} 个警告。`);
  console.log(`验证通过：${directory}`);
  console.log(`结果：0 个错误，${report.warnings.length} 个警告。`);
}

main().catch((error) => {
  console.error(`验证失败：${error.message}`);
  process.exitCode = 1;
});
