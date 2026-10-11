// 旧六文件皮肤 dream-skin.css 的静态性能门禁。
//
// 规则来自 2026 年 9 月 2 日虾米子皮肤的真实 Codex A/B 结论与内置皮肤审计：
// 滚动固定背景、实时模糊、大范围通用选择器和 transition: all 会让宿主在列表更新
// 与输入时反复触发样式重算。阻断级规则是确定性的高成本写法；计数级规则表示
// 规模风险，需要人工按皮肤视觉意图复核，因此只给出带实际数量的警告。

const BLUR_LIMIT = 4;
const BACKDROP_LIMIT = 4;
const SHADOW_LIMIT = 12;
const HAS_LIMIT = 2;
const BROAD_SELECTOR_LIMIT = 6;

const GENERIC_TAGS = new Set([
  "a", "article", "aside", "button", "div", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6",
  "header", "img", "input", "label", "li", "main", "nav", "ol", "option", "p", "section", "select",
  "span", "svg", "table", "tbody", "td", "textarea", "th", "tr", "ul",
]);

function stripComments(css) {
  let output = "";
  let index = 0;
  while (index < css.length) {
    const char = css[index];
    if (char === "/" && css[index + 1] === "*") {
      const end = css.indexOf("*/", index + 2);
      if (end < 0) return output;
      index = end + 2;
      continue;
    }
    if (char === '"' || char === "'") {
      const end = stringEnd(css, index);
      output += css.slice(index, end + 1);
      index = end + 1;
      continue;
    }
    output += char;
    index += 1;
  }
  return output;
}

function stringEnd(css, start) {
  const quote = css[start];
  for (let index = start + 1; index < css.length; index += 1) {
    if (css[index] === "\\") index += 1;
    else if (css[index] === quote) return index;
  }
  return css.length - 1;
}

// 逐条产出最内层规则，并附带所在 @ 规则条件；@media 等外层块在其子规则之后产出空声明体。
export function parseRules(css) {
  const source = stripComments(css);
  const rules = [];
  const stack = [];
  let prelude = "";
  let index = 0;
  while (index < source.length) {
    const char = source[index];
    if (char === '"' || char === "'") {
      const end = stringEnd(source, index);
      prelude += source.slice(index, end + 1);
      index = end + 1;
      continue;
    }
    if (char === "{") {
      stack.push(prelude.trim());
      prelude = "";
    } else if (char === "}") {
      const selector = stack.pop() ?? "";
      rules.push({
        selector,
        body: prelude,
        conditions: stack.filter((entry) => entry.startsWith("@")),
      });
      prelude = "";
    } else {
      prelude += char;
    }
    index += 1;
  }
  return rules;
}

// `prefers-reduced-motion` 下的通用重置是推荐的无障碍写法，且只在系统请求降低动效时生效，
// 不能因为使用了 * 就阻断皮肤。
function isReducedMotionScope(conditions) {
  return conditions.some((condition) => /prefers-reduced-motion/i.test(condition));
}

export function parseDeclarations(body) {
  const declarations = [];
  let depth = 0;
  let buffer = "";
  let index = 0;
  const flush = () => {
    const separator = buffer.indexOf(":");
    if (separator > 0) {
      declarations.push({
        property: buffer.slice(0, separator).trim().toLowerCase(),
        value: buffer.slice(separator + 1).trim(),
      });
    }
    buffer = "";
  };
  while (index < body.length) {
    const char = body[index];
    if (char === '"' || char === "'") {
      const end = stringEnd(body, index);
      buffer += body.slice(index, end + 1);
      index = end + 1;
      continue;
    }
    if (char === "(") depth += 1;
    else if (char === ")") depth = Math.max(0, depth - 1);
    if (char === ";" && depth === 0) flush();
    else buffer += char;
    index += 1;
  }
  flush();
  return declarations;
}

function selectorParts(selector) {
  if (selector.startsWith("@")) return [];
  const parts = [];
  let depth = 0;
  let buffer = "";
  for (let index = 0; index < selector.length; index += 1) {
    const char = selector[index];
    if (char === "(" || char === "[") depth += 1;
    else if (char === ")" || char === "]") depth = Math.max(0, depth - 1);
    if (char === "," && depth === 0) {
      parts.push(buffer.trim());
      buffer = "";
      continue;
    }
    buffer += char;
  }
  if (buffer.trim()) parts.push(buffer.trim());
  return parts.filter(Boolean);
}

// 取选择器最右侧的主体compound，并去掉伪类参数，用于判断规则实际匹配的节点范围。
function subjectCompound(part) {
  const withoutArguments = part.replace(/\((?:[^()]|\([^()]*\))*\)/g, "");
  const tokens = withoutArguments.split(/[\s>+~]+/).filter(Boolean);
  const last = tokens.at(-1) ?? "";
  return last.replace(/::?[a-z-]+$/i, "") || last;
}

function isUniversalSubject(part) {
  const subject = subjectCompound(part);
  return subject === "*" || subject.endsWith("*");
}

function isBroadSubject(part) {
  const subject = subjectCompound(part);
  if (!subject || subject === "*") return false;
  if (subject.startsWith("[")) return true;
  return GENERIC_TAGS.has(subject.toLowerCase());
}

function hasAllKeyword(value) {
  return /(?:^|[\s,])all(?:[\s,]|$)/i.test(value);
}

export function analyzeCssPerformance(css) {
  const rules = parseRules(css);
  const errors = [];
  const warnings = [];
  const metrics = { backdropFilter: 0, blurFilter: 0, boxShadow: 0, has: 0, broadSelectors: 0 };
  const universal = new Set();
  const fixedBackground = new Set();
  const transitionAll = new Set();
  const broadExamples = new Set();

  for (const { selector, body, conditions } of rules) {
    const parts = selectorParts(selector);
    const reducedMotion = isReducedMotionScope(conditions);
    for (const part of parts) {
      if (isUniversalSubject(part) && !reducedMotion) universal.add(part);
      if (isBroadSubject(part)) {
        metrics.broadSelectors += 1;
        broadExamples.add(part);
      }
    }
    metrics.has += parts.filter((part) => /:has\(/i.test(part)).length;

    for (const { property, value } of parseDeclarations(body)) {
      if (property === "backdrop-filter" || property === "-webkit-backdrop-filter") {
        metrics.backdropFilter += 1;
      }
      if (property === "filter" && /\bblur\(/i.test(value)) metrics.blurFilter += 1;
      if (property === "box-shadow" || property === "-webkit-box-shadow") metrics.boxShadow += 1;
      if (property === "background-attachment" && /\bfixed\b/i.test(value)) {
        fixedBackground.add(selector);
      }
      if ((property === "background" || property === "background-image")
        && /(?:^|[\s,])fixed(?:[\s,]|$)/i.test(value)) {
        fixedBackground.add(selector);
      }
      if (property === "transition-property" && hasAllKeyword(value)) transitionAll.add(selector);
      if (property === "transition" && hasAllKeyword(value)) transitionAll.add(selector);
    }
  }

  const listed = (values, limit = 3) => {
    const items = [...values];
    const head = items.slice(0, limit).join("、");
    return items.length > limit ? `${head} 等 ${items.length} 处` : head;
  };

  if (fixedBackground.size) {
    errors.push(`滚动时固定背景会强制整页重绘，请改为静态表面：${listed(fixedBackground)}。`);
  }
  if (transitionAll.size) {
    errors.push(`transition 使用 all 会让宿主每次属性变化都进入过渡，请显式列出属性：${listed(transitionAll)}。`);
  }
  if (universal.size) {
    errors.push(`通用选择器 * 会匹配宿主全部节点，请收敛到具体组件：${listed(universal)}。`);
  }
  if (metrics.backdropFilter > BACKDROP_LIMIT) {
    warnings.push(`实时 backdrop-filter 共 ${metrics.backdropFilter} 处，超过建议的 ${BACKDROP_LIMIT} 处；请改用更不透明的静态表面保留层级。`);
  }
  if (metrics.blurFilter > BLUR_LIMIT) {
    warnings.push(`filter: blur() 共 ${metrics.blurFilter} 处，超过建议的 ${BLUR_LIMIT} 处。`);
  }
  if (metrics.boxShadow > SHADOW_LIMIT) {
    warnings.push(`box-shadow 共 ${metrics.boxShadow} 处，超过建议的 ${SHADOW_LIMIT} 处；大量阴影会放大列表更新时的绘制成本。`);
  }
  if (metrics.has > HAS_LIMIT) {
    warnings.push(`:has() 共 ${metrics.has} 处，超过建议的 ${HAS_LIMIT} 处；父级选择器会扩大样式重算范围。`);
  }
  if (metrics.broadSelectors > BROAD_SELECTOR_LIMIT) {
    warnings.push(`以通用标签或裸属性结尾的宽泛选择器共 ${metrics.broadSelectors} 处，超过建议的 ${BROAD_SELECTOR_LIMIT} 处：${listed(broadExamples)}。`);
  }

  return { metrics, errors, warnings };
}

export function formatPerformanceMetrics(metrics) {
  return `性能计数：backdrop-filter ${metrics.backdropFilter}、blur ${metrics.blurFilter}、box-shadow ${metrics.boxShadow}、:has() ${metrics.has}、宽泛选择器 ${metrics.broadSelectors}。`;
}
