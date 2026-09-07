const ROOT = "html.codex-dream-skin";
const DARK = 'html.codex-dream-skin[data-dream-shell="dark"]';
const LIGHT = 'html.codex-dream-skin[data-dream-shell="light"]';
const PALETTE = ["--skin-bg", "--skin-panel", "--skin-panel-alt", "--skin-accent", "--skin-accent-alt", "--skin-text", "--skin-muted", "--skin-line"];
const ROOT_PROPERTIES = [
  "--skin-preview-image", "--skin-background-image", "--skin-background-size",
  "--skin-background-position-x", "--skin-background-position-y", "--skin-radius",
  "--skin-blur", "--skin-sidebar-opacity", "--skin-main-opacity", "--skin-header-opacity",
  "--skin-composer-opacity", "--skin-card-opacity", "--skin-border-width",
  "--skin-shadow-opacity", "--skin-texture-opacity",
];

const invalid = (message) => new Error(`theme.css ${message}`);

function parseBlock(body) {
  const declarations = new Map();
  for (const raw of body.split(";")) {
    if (!raw.trim()) continue;
    const separator = raw.indexOf(":");
    if (separator < 0) throw invalid("包含无效声明。");
    const property = raw.slice(0, separator).trim();
    const value = raw.slice(separator + 1).trim();
    if (!property.startsWith("--skin-") || !value || value.includes("!important") || declarations.has(property)) {
      throw invalid("只能声明固定 --skin-* 变量，且每项只能出现一次。");
    }
    declarations.set(property, value);
  }
  return declarations;
}

function exactProperties(block, expected) {
  return block.size === expected.length && expected.every((property) => block.has(property));
}

function numberValue(value, suffix, min, max) {
  if (!value.endsWith(suffix)) return false;
  const number = Number(value.slice(0, -suffix.length));
  return Number.isInteger(number) && number >= min && number <= max;
}

function localImage(value) {
  const match = value.match(/^url\((["'])([^"'()\\/]+\.(?:png|jpe?g))\1\)$/i);
  if (!match || ["theme.json", "theme.css"].includes(match[2].toLowerCase())) throw invalid("图片必须引用根目录 PNG/JPEG 安全文件名。");
  return match[2];
}

export function parseThemeCss(source, { explicitAppearance = false } = {}) {
  if (typeof source !== "string" || !source || Buffer.byteLength(source, "utf8") > 64 * 1024) throw invalid("必须非空且不超过 64 KiB。");
  const css = source.replace(/\/\*[\s\S]*?\*\//g, " ");
  if (css.includes("/*") || css.includes("*/")) throw invalid("包含未闭合注释。");
  const blocks = new Map();
  let rest = css.trim();
  while (rest) {
    const open = rest.indexOf("{");
    const close = rest.indexOf("}", open + 1);
    if (open < 0 || close < 0) throw invalid("结构无效。");
    const selector = rest.slice(0, open).trim();
    if (![ROOT, DARK, LIGHT].includes(selector) || blocks.has(selector)) throw invalid("只能使用三个固定根选择器。");
    const body = rest.slice(open + 1, close);
    if (body.includes("{")) throw invalid("结构无效。");
    blocks.set(selector, parseBlock(body));
    rest = rest.slice(close + 1).trim();
  }
  if (blocks.size !== (explicitAppearance ? 1 : 3)) {
    throw invalid(explicitAppearance ? "声明 appearance 后只能包含基础变量块。" : "必须完整声明基础、深色和浅色三个变量块。");
  }
  const root = blocks.get(ROOT);
  const expectedRoot = explicitAppearance ? [...ROOT_PROPERTIES, ...PALETTE] : ROOT_PROPERTIES;
  if (!exactProperties(root, expectedRoot)) throw invalid("基础变量不完整或包含未知变量。");
  if (!["cover", "contain"].includes(root.get("--skin-background-size"))
    || !numberValue(root.get("--skin-background-position-x"), "%", 0, 100)
    || !numberValue(root.get("--skin-background-position-y"), "%", 0, 100)
    || !numberValue(root.get("--skin-radius"), "px", 0, 32)
    || !numberValue(root.get("--skin-blur"), "px", 0, 40)
    || !numberValue(root.get("--skin-border-width"), "px", 0, 3)) throw invalid("尺寸、位置或背景适配值超出范围。");
  for (const property of ["--skin-sidebar-opacity", "--skin-main-opacity", "--skin-header-opacity", "--skin-composer-opacity", "--skin-card-opacity", "--skin-shadow-opacity", "--skin-texture-opacity"]) {
    if (!numberValue(root.get(property), "%", 0, 100)) throw invalid(`${property} 必须为 0% 到 100%。`);
  }
  const palettes = {};
  if (explicitAppearance) {
    if (PALETTE.some((property) => !/^#[0-9a-f]{6}(?:[0-9a-f]{2})?$/i.test(root.get(property)))) {
      throw invalid("基础色板必须完整使用 #RRGGBB 或 #RRGGBBAA。");
    }
  } else {
    for (const [mode, selector] of [["dark", DARK], ["light", LIGHT]]) {
      const block = blocks.get(selector);
      if (!exactProperties(block, PALETTE) || PALETTE.some((property) => !/^#[0-9a-f]{6}(?:[0-9a-f]{2})?$/i.test(block.get(property)))) {
        throw invalid(`${mode} 色板必须完整使用 #RRGGBB 或 #RRGGBBAA。`);
      }
      palettes[mode] = Object.fromEntries(PALETTE.map((property) => [property, block.get(property)]));
    }
  }
  return {
    preview: localImage(root.get("--skin-preview-image")),
    background: localImage(root.get("--skin-background-image")),
    palettes,
  };
}

const MODE_PROPERTIES = new Set([
  ...PALETTE,
  ...ROOT_PROPERTIES.filter((property) => !property.endsWith("-image")),
  "--skin-background-image",
]);

function validModeValue(property, value) {
  if (PALETTE.includes(property)) return /^#[0-9a-f]{6}(?:[0-9a-f]{2})?$/i.test(value);
  if (property === "--skin-background-image") {
    try { localImage(value); return true; } catch { return false; }
  }
  if (property === "--skin-background-size") return ["cover", "contain"].includes(value);
  if (["--skin-background-position-x", "--skin-background-position-y", "--skin-sidebar-opacity", "--skin-main-opacity", "--skin-header-opacity", "--skin-composer-opacity", "--skin-card-opacity", "--skin-shadow-opacity", "--skin-texture-opacity"].includes(property)) return numberValue(value, "%", 0, 100);
  if (property === "--skin-radius") return numberValue(value, "px", 0, 32);
  if (property === "--skin-blur") return numberValue(value, "px", 0, 40);
  if (property === "--skin-border-width") return numberValue(value, "px", 0, 3);
  return false;
}

export function parseModeThemeCss(source, mode, file = `theme.${mode}.css`) {
  if (typeof source !== "string" || !source || Buffer.byteLength(source, "utf8") > 64 * 1024) throw new Error(`${file} 必须非空且不超过 64 KiB。`);
  if (!["light", "dark"].includes(mode)) throw new Error(`${file} 的颜色模式无效。`);
  const css = source.replace(/\/\*[\s\S]*?\*\//g, " ").trim();
  const selector = mode === "dark" ? DARK : LIGHT;
  const open = css.indexOf("{");
  const close = css.lastIndexOf("}");
  if (open < 0 || close <= open || css.slice(0, open).trim() !== selector || css.slice(close + 1).trim() || /[{}]/.test(css.slice(open + 1, close))) {
    throw new Error(`${file} 只能使用对应颜色模式的单一根选择器。`);
  }
  const block = parseBlock(css.slice(open + 1, close));
  if (!block.size) throw new Error(`${file} 至少需要声明一个增量变量。`);
  for (const [property, value] of block) {
    if (!MODE_PROPERTIES.has(property) || !validModeValue(property, value)) throw new Error(`${file} 的 ${property} 不允许或值超出范围。`);
  }
  return {
    declarations: Object.fromEntries(block),
    background: block.has("--skin-background-image") ? localImage(block.get("--skin-background-image")) : null,
  };
}
