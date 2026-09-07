#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import path from "node:path";
import { SKILL_DIR, SURFACES, parseArgs } from "./lib.mjs";

const ROUTE_SURFACES = new Set(["pull-requests", "sites", "automations", "plugins", "settings"]);
const SELECTOR_LINE = /^\s+- selector:\s+("(?:[^"\\]|\\.)*")\s*$/gm;
const SURFACE_LINE = /^\s+surface:\s+([a-z-]+)\s*$/gm;
const CSS_PRELUDE = /([^{}]+)\{/g;
const CSS_RULE = /([^{}]+)\{([^{}]*)\}/g;
const SELECTOR_ATOM = /#[A-Za-z_][\w-]*|\.[A-Za-z_][\w\\/-]*|\[[^\]]+\]/g;
const HOST_MAP_VALUE = /^\s+[A-Za-z][A-Za-z0-9]*:\s+"([^"]+)",?\s*$/gm;

function normalizeAtom(value) {
  return value.trim().replaceAll('"', "'").replace(/\s*([~|^$*]?=)\s*/g, "$1");
}

function atoms(value) {
  return new Set([...value.matchAll(SELECTOR_ATOM)].map((match) => normalizeAtom(match[0])));
}

function registered(atom, registeredAtoms) {
  if (registeredAtoms.has(atom)) return true;
  const attribute = atom.match(/^\[([A-Za-z_:][\w:.-]*)/);
  return Boolean(attribute && registeredAtoms.has(`[${attribute[1]}]`));
}

function mapSelectors(mapText) {
  return [...mapText.matchAll(SELECTOR_LINE)].map((match) => JSON.parse(match[1]));
}

function cssSelectorText(css) {
  const output = [];
  for (const match of css.matchAll(CSS_PRELUDE)) {
    const prelude = match[1].trim();
    if (prelude.startsWith("@") || /(?:from|to)$/.test(prelude) || /^\d+%$/.test(prelude)) continue;
    output.push(prelude);
  }
  return output.join("\n");
}

function runtimeSelectorText(source) {
  const start = source.indexOf("const HOST_SELECTORS");
  const end = source.indexOf("});", start);
  if (start < 0 || end < 0) return "";
  return [...source.slice(start, end).matchAll(HOST_MAP_VALUE)].map((match) => match[1]).join("\n");
}

export async function auditSkin(skillDir, skinDir) {
  const mapText = await readFile(path.join(skillDir, "references", "component-map.yaml"), "utf8");
  const css = await readFile(path.join(skinDir, "dream-skin.css"), "utf8");
  const source = await readFile(path.join(skinDir, "renderer-inject.js"), "utf8");
  const selectors = mapSelectors(mapText);
  const registeredAtoms = atoms(selectors.join("\n"));
  const usedAtoms = new Set([...atoms(cssSelectorText(css)), ...atoms(runtimeSelectorText(source))]);
  const missingAtoms = [...usedAtoms].filter((atom) => !registered(atom, registeredAtoms)).sort();
  const errors = missingAtoms.map((atom) => `宿主选择器原子未登记到 component-map.yaml：${atom}`);
  for (const match of css.matchAll(CSS_RULE)) {
    const prelude = match[1];
    const declarations = match[2];
    const targetsPluginRegion = prelude.includes('[id^="plugins-search-"]') || prelude.includes('[id^="plugins-marketplace-"]');
    if (targetsPluginRegion && /(?:^|;)\s*(?:background(?:-color|-image)?|border-radius)\s*:/m.test(declarations)) {
      errors.push("插件分区锚点不得直接设置背景或卡片圆角；请将卡片样式限定到分区内的列表项。");
    }
  }
  const declaredSurfaces = new Set([...mapText.matchAll(SURFACE_LINE)].map((match) => match[1]));
  const missingSurfaces = SURFACES.filter((surface) => surface !== "other" && !declaredSurfaces.has(surface));
  if (missingSurfaces.length) errors.push(`组件 Map 缺少页面 surface：${missingSurfaces.join(", ")}`);
  for (const surface of SURFACES) {
    const stateSelector = `html[data-dream-surface='${surface}']`;
    if (!selectors.includes(stateSelector)) errors.push(`组件 Map 缺少页面状态选择器：${stateSelector}`);
  }
  if (!runtimeSelectorText(source)) errors.push("renderer-inject.js 缺少集中式 HOST_SELECTORS。");
  if (!source.includes('get("initialRoute")')) errors.push("renderer-inject.js 未读取渲染窗口 initialRoute。");
  const routeStart = source.indexOf("const ROUTE_SURFACES");
  const routeEnd = source.indexOf("]);", routeStart);
  const routeBlock = routeStart >= 0 && routeEnd >= 0 ? source.slice(routeStart, routeEnd) : "";
  for (const surface of [...ROUTE_SURFACES].sort()) {
    if (!routeBlock.includes(`"${surface}"`)) errors.push(`renderer-inject.js 缺少空/错误状态路由兜底：${surface}`);
  }
  const previewSource = await readFile(path.join(skillDir, "scripts", "render_preview.mjs"), "utf8");
  for (const surface of [...SURFACES].sort()) {
    if (!previewSource.includes(`"${surface}"`) && !previewSource.includes(`'${surface}'`)) errors.push(`render_preview.mjs 缺少页面预览：${surface}`);
  }
  const featureReference = await readFile(path.join(skillDir, "references", "feature-surfaces.md"), "utf8");
  for (const heading of ["拉取请求", "站点", "已安排", "插件", "设置"]) {
    if (!featureReference.includes(`## ${heading}`)) errors.push(`功能页参考资料缺少独立章节：${heading}`);
  }
  return errors;
}

async function main() {
  const { positional } = parseArgs(process.argv.slice(2));
  if (positional.length > 1) throw new Error("最多接收一个待审计皮肤目录。");
  const skinDir = path.resolve(positional[0] ?? path.join(SKILL_DIR, "assets", "skin-template"));
  const errors = await auditSkin(SKILL_DIR, skinDir);
  for (const error of errors) console.error(`错误：${error}`);
  if (errors.length) throw new Error(`选择器 Map 审计失败：${errors.length} 个错误。`);
  console.log("选择器 Map 审计通过：模板宿主选择器已登记，五类功能页资料与 8 类页面状态完整。");
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(`选择器 Map 审计失败：${error.message}`);
    process.exitCode = 1;
  });
}
