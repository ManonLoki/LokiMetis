#!/usr/bin/env node
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { SKILL_DIR, SURFACES, parseArgs } from "./lib.mjs";

const ROUTE_SURFACES = new Set([
  "pull-requests",
  "sites",
  "automations",
  "plugins",
  "settings",
]);
const SELECTOR_LINE = /^\s+- selector:\s+("(?:[^"\\]|\\.)*")\s*$/gm;
const SURFACE_LINE = /^\s+surface:\s+([a-z-]+)\s*$/gm;
const CSS_PRELUDE = /([^{}]+)\{/g;
const CSS_RULE = /([^{}]+)\{([^{}]*)\}/g;
const SELECTOR_ATOM = /#[A-Za-z_][\w-]*|\.[A-Za-z_][\w\\/-]*|\[[^\]]+\]/g;
const HOST_MAP_VALUE =
  /^[ \t]+[A-Za-z][A-Za-z0-9]*:[ \t]*(?:\r?\n[ \t]*)?(?:"([^"]+)"|'([^']+)'),?[ \t]*$/gm;
const REQUIRED_WORKBUDDY_COMPOSER_TOKENS = [
  "--cb-content-background",
  "--cb-content-border-color",
  "--cb-main-area-background",
  "--cb-main-area-border-color",
  "--cb-main-area-box-shadow",
];

function normalizeAtom(value) {
  return value
    .trim()
    .replaceAll('"', "'")
    .replace(/\s*([~|^$*]?=)\s*/g, "$1");
}

function atoms(value) {
  return new Set(
    [...value.matchAll(SELECTOR_ATOM)].map((match) => normalizeAtom(match[0])),
  );
}

function registered(atom, registeredAtoms) {
  if (registeredAtoms.has(atom)) return true;
  const attribute = atom.match(/^\[([A-Za-z_:][\w:.-]*)/);
  return Boolean(attribute && registeredAtoms.has(`[${attribute[1]}]`));
}

function mapSelectors(mapText) {
  return [...mapText.matchAll(SELECTOR_LINE)].map((match) =>
    JSON.parse(match[1]),
  );
}

function mapComponents(mapText) {
  const components = [];
  let current = null;
  for (const line of mapText.split(/\r?\n/)) {
    const componentId = line.match(/^  - component_id:\s+([^\s#]+)\s*$/)?.[1];
    if (componentId) {
      if (current) components.push(current);
      current = { componentId, kind: "", selectors: [], source: line };
      continue;
    }
    if (!current) continue;
    current.source += `\n${line}`;
    const kind = line.match(/^    kind:\s+([a-z-]+)\s*$/)?.[1];
    if (kind) current.kind = kind;
    const selector = line.match(
      /^\s+- selector:\s+("(?:[^"\\]|\\.)*")\s*$/,
    )?.[1];
    if (selector) current.selectors.push(JSON.parse(selector));
  }
  if (current) components.push(current);
  return components;
}

function cssSelectorText(css) {
  const output = [];
  for (const match of css.matchAll(CSS_PRELUDE)) {
    const prelude = match[1].trim();
    if (
      prelude.startsWith("@") ||
      /(?:from|to)$/.test(prelude) ||
      /^\d+%$/.test(prelude)
    )
      continue;
    output.push(prelude);
  }
  return output.join("\n");
}

function runtimeSelectorText(source) {
  const start = source.indexOf("const HOST_SELECTORS");
  const end = source.indexOf("});", start);
  if (start < 0 || end < 0) return "";
  return [...source.slice(start, end).matchAll(HOST_MAP_VALUE)]
    .map((match) => match[1] ?? match[2])
    .join("\n");
}

function templateConstant(source, name) {
  const marker = `const ${name} = \``;
  const start = source.indexOf(marker);
  if (start < 0) return "";
  const valueStart = start + marker.length;
  const end = source.indexOf("`;", valueStart);
  return end < 0 ? "" : source.slice(valueStart, end);
}

function stringConstant(source, name) {
  const match = source.match(new RegExp(`const ${name} = ["']([^"']+)["'];`));
  return match?.[1] ?? "";
}

function selectorsForKind(components, kind) {
  return components
    .filter((component) => component.kind === kind)
    .flatMap((component) => component.selectors);
}

export function auditWorkBuddyAdapter(mapText, adapterSource) {
  const errors = [];
  const components = mapComponents(mapText);
  const nativeAtoms = atoms(
    selectorsForKind(components, "workbuddy-native").join("\n"),
  );
  const stateComponents = components.filter(
    (component) => component.kind === "skin-state",
  );
  const stateAtoms = atoms(
    stateComponents.flatMap((component) => component.selectors).join("\n"),
  );
  const selectorText = runtimeSelectorText(adapterSource);
  if (!selectorText) {
    errors.push("WorkBuddy 宿主适配器缺少集中式 HOST_SELECTORS。");
  } else {
    for (const atom of [...atoms(selectorText)].sort()) {
      if (!registered(atom, nativeAtoms)) {
        errors.push(
          `WorkBuddy 宿主选择器原子未登记到 kind=workbuddy-native：${atom}`,
        );
      }
    }
  }

  const styleText = templateConstant(adapterSource, "STYLE_TEXT");
  const compatibilityAtoms = new Set(
    [...atoms(cssSelectorText(styleText))].filter((atom) =>
      atom.startsWith(".loki-metis-workbuddy-"),
    ),
  );
  const versionAttribute = stringConstant(adapterSource, "VERSION_ATTRIBUTE");
  const styleId = stringConstant(adapterSource, "STYLE_ID");
  if (versionAttribute) compatibilityAtoms.add(`[${versionAttribute}]`);
  if (styleId) compatibilityAtoms.add(`#${styleId}`);
  for (const atom of [...compatibilityAtoms].sort()) {
    if (!registered(atom, stateAtoms)) {
      errors.push(
        `WorkBuddy 兼容状态选择器原子未登记到 kind=skin-state：${atom}`,
      );
    }
    if (registered(atom, nativeAtoms)) {
      errors.push(
        `WorkBuddy 兼容状态选择器不得登记为 workbuddy-native：${atom}`,
      );
    }
  }

  const stateSource = stateComponents
    .map((component) => component.source)
    .join("\n");
  for (const token of REQUIRED_WORKBUDDY_COMPOSER_TOKENS) {
    const escapedToken = token.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const declaration = styleText.match(
      new RegExp(`${escapedToken}\\s*:\\s*([^;]+);`),
    );
    if (!declaration) {
      errors.push(`WorkBuddy composer 适配器缺少宿主变量映射：${token}`);
    } else {
      const value = declaration[1].replace(/\s*!important\s*$/, "").trim();
      const isPureThemeMapping =
        value === "transparent" ||
        value === "none" ||
        value.includes("var(--skin-");
      if (!isPureThemeMapping || /var\(--(?:cb|wb)-/.test(value)) {
        errors.push(
          `WorkBuddy composer 宿主变量未映射到纯主题 token：${token}`,
        );
      }
    }
    if (!stateSource.includes(`target: "${token}"`)) {
      errors.push(
        `WorkBuddy composer 宿主变量未登记到 skin-state token_mappings：${token}`,
      );
    }
  }
  return errors;
}

async function readOptional(filePath) {
  try {
    return await readFile(filePath, "utf8");
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw error;
  }
}

export async function auditSkin(skillDir, skinDir) {
  const workBuddyAdapterPath = path.resolve(
    skillDir,
    "..",
    "..",
    "..",
    "loki_metis_gui",
    "resources",
    "workbuddy-skin-host-compat.js",
  );
  const [
    mapText,
    css,
    source,
    previewSource,
    featureReference,
    workBuddyAdapter,
  ] = await Promise.all([
    readFile(path.join(skillDir, "references", "component-map.yaml"), "utf8"),
    readFile(path.join(skinDir, "dream-skin.css"), "utf8"),
    readFile(path.join(skinDir, "renderer-inject.js"), "utf8"),
    readFile(path.join(skillDir, "scripts", "render_preview.mjs"), "utf8"),
    readFile(path.join(skillDir, "references", "feature-surfaces.md"), "utf8"),
    readOptional(workBuddyAdapterPath),
  ]);
  const selectors = mapSelectors(mapText);
  const registeredAtoms = atoms(selectors.join("\n"));
  const usedAtoms = new Set([
    ...atoms(cssSelectorText(css)),
    ...atoms(runtimeSelectorText(source)),
  ]);
  const missingAtoms = [...usedAtoms]
    .filter((atom) => !registered(atom, registeredAtoms))
    .sort();
  const errors = missingAtoms.map(
    (atom) => `宿主选择器原子未登记到 component-map.yaml：${atom}`,
  );
  for (const match of css.matchAll(CSS_RULE)) {
    const prelude = match[1];
    const declarations = match[2];
    const targetsPluginRegion =
      prelude.includes('[id^="plugins-search-"]') ||
      prelude.includes('[id^="plugins-marketplace-"]');
    if (
      targetsPluginRegion &&
      /(?:^|;)\s*(?:background(?:-color|-image)?|border-radius)\s*:/m.test(
        declarations,
      )
    ) {
      errors.push(
        "插件分区锚点不得直接设置背景或卡片圆角；请将卡片样式限定到分区内的列表项。",
      );
    }
  }
  const declaredSurfaces = new Set(
    [...mapText.matchAll(SURFACE_LINE)].map((match) => match[1]),
  );
  const missingSurfaces = SURFACES.filter(
    (surface) => surface !== "other" && !declaredSurfaces.has(surface),
  );
  if (missingSurfaces.length)
    errors.push(`组件 Map 缺少页面 surface：${missingSurfaces.join(", ")}`);
  for (const surface of SURFACES) {
    const stateSelector = `html[data-dream-surface='${surface}']`;
    if (!selectors.includes(stateSelector))
      errors.push(`组件 Map 缺少页面状态选择器：${stateSelector}`);
  }
  if (!runtimeSelectorText(source))
    errors.push("renderer-inject.js 缺少集中式 HOST_SELECTORS。");
  if (!source.includes('get("initialRoute")'))
    errors.push("renderer-inject.js 未读取渲染窗口 initialRoute。");
  const routeStart = source.indexOf("const ROUTE_SURFACES");
  const routeEnd = source.indexOf("]);", routeStart);
  const routeBlock =
    routeStart >= 0 && routeEnd >= 0 ? source.slice(routeStart, routeEnd) : "";
  for (const surface of [...ROUTE_SURFACES].sort()) {
    if (!routeBlock.includes(`"${surface}"`))
      errors.push(`renderer-inject.js 缺少空/错误状态路由兜底：${surface}`);
  }
  for (const surface of [...SURFACES].sort()) {
    if (
      !previewSource.includes(`"${surface}"`) &&
      !previewSource.includes(`'${surface}'`)
    )
      errors.push(`render_preview.mjs 缺少页面预览：${surface}`);
  }
  for (const heading of ["拉取请求", "站点", "已安排", "插件", "设置"]) {
    if (!featureReference.includes(`## ${heading}`))
      errors.push(`功能页参考资料缺少独立章节：${heading}`);
  }
  if (workBuddyAdapter)
    errors.push(...auditWorkBuddyAdapter(mapText, workBuddyAdapter));
  return errors;
}

async function main() {
  const { positional } = parseArgs(process.argv.slice(2));
  if (positional.length > 1) throw new Error("最多接收一个待审计皮肤目录。");
  const skinDir = path.resolve(
    positional[0] ?? path.join(SKILL_DIR, "assets", "skin-template"),
  );
  const errors = await auditSkin(SKILL_DIR, skinDir);
  for (const error of errors) console.error(`错误：${error}`);
  if (errors.length)
    throw new Error(`选择器 Map 审计失败：${errors.length} 个错误。`);
  console.log(
    "选择器 Map 审计通过：模板与可用的 WorkBuddy 宿主选择器已登记，五类功能页资料与 8 类页面状态完整。",
  );
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  main().catch((error) => {
    console.error(`选择器 Map 审计失败：${error.message}`);
    process.exitCode = 1;
  });
}
