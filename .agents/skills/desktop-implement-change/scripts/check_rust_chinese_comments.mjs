#!/usr/bin/env node

/** 检查 Cargo package/workspace 中受管 Rust 声明是否有紧邻中文文档注释。 */

import {
  globSync,
  lstatSync,
  readFileSync,
  readdirSync,
  realpathSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { canonicalRoot, lstatOrNull, parseArguments } from "./check_file_line_limits.mjs";

const DECLARATION_KINDS = new Set(["enum", "fn", "struct", "trait", "type", "union"]);
const FUNCTION_MODIFIERS = new Set(["async", "const", "default", "unsafe"]);
const SOURCE_DIRECTORIES = ["src", "tests"];
const DELIMITER_PAIRS = new Map([["(", ")"], ["[", "]"], ["{", "}"]]);

function containsHan(text) { return /\p{Script=Han}/u.test(text); }

function quotedEnd(source, quoteStart) {
  let cursor = quoteStart + 1;
  let escaped = false;
  while (cursor < source.length) {
    const character = source[cursor];
    if (escaped) escaped = false;
    else if (character === "\\") escaped = true;
    else if (character === '"') return cursor + 1;
    cursor += 1;
  }
  return null;
}

function charEnd(source, quoteStart) {
  let cursor = quoteStart + 1;
  if (cursor >= source.length || "\r\n'".includes(source[cursor])) return null;
  if (source[cursor] === "\\") {
    cursor += 1;
    if (cursor >= source.length || "\r\n".includes(source[cursor])) return null;
    if (source[cursor] === "u" && source[cursor + 1] === "{") {
      const closing = source.indexOf("}", cursor + 2);
      if (closing < 0) return null;
      cursor = closing + 1;
    } else cursor += 1;
  } else cursor += 1;
  return cursor < source.length && source[cursor] === "'" ? cursor + 1 : null;
}

function rawStringEnd(source, start) {
  let cursor = start;
  if (source.startsWith("br", cursor) || source.startsWith("cr", cursor)) cursor += 2;
  else if (source.startsWith("r", cursor)) cursor += 1;
  else return null;
  const hashesStart = cursor;
  while (cursor < source.length && source[cursor] === "#") cursor += 1;
  if (cursor >= source.length || source[cursor] !== '"') return null;
  const terminator = `"${source.slice(hashesStart, cursor)}`;
  const end = source.indexOf(terminator, cursor + 1);
  return end < 0 ? null : end + terminator.length;
}

function advancePosition(fragment, line, column) {
  const parts = fragment.split("\n");
  return parts.length > 1 ? [line + parts.length - 1, parts.at(-1).length + 1] : [line, column + fragment.length];
}

function lex(source) {
  const tokens = [];
  const errors = [];
  let cursor = 0;
  let line = 1;
  let column = 1;
  while (cursor < source.length) {
    const start = cursor;
    const startLine = line;
    const startColumn = column;
    if (/\s/u.test(source[cursor])) cursor += 1;
    else if (source.startsWith("//", cursor)) {
      const end = source.indexOf("\n", cursor);
      cursor = end < 0 ? source.length : end;
      const payload = source.slice(start, cursor);
      if (payload.startsWith("///") && !payload.startsWith("////")) tokens.push({ kind: "doc_outer", text: payload, line: startLine, column: startColumn });
      else if (payload.startsWith("//!")) tokens.push({ kind: "doc_inner", text: payload, line: startLine, column: startColumn });
    } else if (source.startsWith("/*", cursor)) {
      let depth = 1;
      cursor += 2;
      while (cursor < source.length && depth > 0) {
        if (source.startsWith("/*", cursor)) { depth += 1; cursor += 2; }
        else if (source.startsWith("*/", cursor)) { depth -= 1; cursor += 2; }
        else cursor += 1;
      }
      if (depth > 0) { errors.push(`第 ${startLine} 行存在未闭合块注释`); cursor = source.length; }
      const payload = source.slice(start, cursor);
      if (payload.startsWith("/**") && !payload.startsWith("/***")) tokens.push({ kind: "doc_outer", text: payload, line: startLine, column: startColumn });
      else if (payload.startsWith("/*!")) tokens.push({ kind: "doc_inner", text: payload, line: startLine, column: startColumn });
    } else {
      const rawEnd = rawStringEnd(source, cursor);
      if (rawEnd !== null) {
        cursor = rawEnd;
        tokens.push({ kind: "string", text: source.slice(start, cursor), line: startLine, column: startColumn });
      } else if (source[cursor] === '"' || (["b", "c"].includes(source[cursor]) && source[cursor + 1] === '"')) {
        const quoteStart = source[cursor] === '"' ? cursor : cursor + 1;
        const end = quotedEnd(source, quoteStart);
        if (end === null) { errors.push(`第 ${startLine} 行存在未闭合字符串字面量`); cursor = source.length; }
        else cursor = end;
        tokens.push({ kind: "string", text: source.slice(start, cursor), line: startLine, column: startColumn });
      } else if (source[cursor] === "'" || (source[cursor] === "b" && source[cursor + 1] === "'")) {
        const quoteStart = source[cursor] === "'" ? cursor : cursor + 1;
        const end = charEnd(source, quoteStart);
        if (end === null) { cursor += 1; tokens.push({ kind: "punct", text: source.slice(start, cursor), line: startLine, column: startColumn }); }
        else { cursor = end; tokens.push({ kind: "char", text: source.slice(start, cursor), line: startLine, column: startColumn }); }
      } else if (source[cursor] === "_" || /\p{L}/u.test(source[cursor])) {
        cursor += 1;
        while (cursor < source.length && (source[cursor] === "_" || /[\p{L}\p{N}]/u.test(source[cursor]))) cursor += 1;
        tokens.push({ kind: "word", text: source.slice(start, cursor), line: startLine, column: startColumn });
      } else {
        cursor += 1;
        tokens.push({ kind: "punct", text: source.slice(start, cursor), line: startLine, column: startColumn });
      }
    }
    [line, column] = advancePosition(source.slice(start, cursor), line, column);
  }
  return [tokens, errors];
}

function delimiterErrors(tokens) {
  const stack = [];
  const closingToOpening = new Map([...DELIMITER_PAIRS].map(([opening, closing]) => [closing, opening]));
  const errors = [];
  for (const token of tokens) {
    if (DELIMITER_PAIRS.has(token.text)) stack.push(token);
    else if (closingToOpening.has(token.text)) {
      if (stack.length === 0 || stack.at(-1).text !== closingToOpening.get(token.text)) errors.push(`第 ${token.line} 行存在不匹配的 \`${token.text}\``);
      else stack.pop();
    }
  }
  errors.push(...stack.map((token) => `第 ${token.line} 行存在未闭合的 \`${token.text}\``));
  return errors;
}

function matchingOpen(tokens, closingIndex, opening, closing) {
  let depth = 0;
  for (let index = closingIndex; index >= 0; index -= 1) {
    if (tokens[index].text === closing) depth += 1;
    else if (tokens[index].text === opening) { depth -= 1; if (depth === 0) return index; }
  }
  return null;
}

function matchingClose(tokens, openingIndex) {
  const opening = tokens[openingIndex].text;
  const closing = DELIMITER_PAIRS.get(opening);
  if (closing === undefined) return null;
  let depth = 0;
  for (let index = openingIndex; index < tokens.length; index += 1) {
    if (tokens[index].text === opening) depth += 1;
    else if (tokens[index].text === closing) { depth -= 1; if (depth === 0) return index; }
  }
  return null;
}

function macroBodyIndexes(tokens) {
  const skipped = new Set();
  tokens.forEach((token, index) => {
    if (token.text !== "!" || index === 0 || tokens[index - 1].kind !== "word") return;
    let openingIndex = index + 1;
    if (tokens[index - 1].text === "macro_rules" && tokens[openingIndex]?.kind === "word") openingIndex += 1;
    if (!DELIMITER_PAIRS.has(tokens[openingIndex]?.text)) return;
    const closingIndex = matchingClose(tokens, openingIndex);
    if (closingIndex !== null) for (let body = openingIndex + 1; body < closingIndex; body += 1) skipped.add(body);
  });
  return skipped;
}

function declarationPrefixStart(tokens, keywordIndex) {
  let cursor = keywordIndex - 1;
  while (cursor >= 0) {
    const token = tokens[cursor];
    if (token.kind === "word" && (FUNCTION_MODIFIERS.has(token.text) || token.text === "pub" || token.text === "extern")) { cursor -= 1; continue; }
    if (token.kind === "string" && cursor > 0 && tokens[cursor - 1].kind === "word" && tokens[cursor - 1].text === "extern") { cursor -= 2; continue; }
    if (token.text === ")") {
      const opening = matchingOpen(tokens, cursor, "(", ")");
      if (opening !== null && opening > 0 && tokens[opening - 1].kind === "word" && tokens[opening - 1].text === "pub") { cursor = opening - 2; continue; }
    }
    break;
  }
  return cursor + 1;
}

function attributeStart(tokens, closingIndex) {
  if (tokens[closingIndex].text !== "]") return null;
  const opening = matchingOpen(tokens, closingIndex, "[", "]");
  if (opening === null || opening === 0 || tokens[opening - 1].text !== "#") return null;
  if (opening >= 2 && tokens[opening - 2].text === "!") return null;
  return opening - 1;
}

function splitTopLevelArguments(tokens) {
  const argumentsList = [];
  let current = [];
  const stack = [];
  for (const token of tokens) {
    if (DELIMITER_PAIRS.has(token.text)) stack.push(token.text);
    else if (stack.length > 0 && token.text === DELIMITER_PAIRS.get(stack.at(-1))) stack.pop();
    if (token.text === "," && stack.length === 0) { argumentsList.push(current); current = []; }
    else current.push(token);
  }
  argumentsList.push(current);
  return argumentsList;
}

function attributeMetaHasChineseDoc(tokens) {
  if (tokens.length === 3 && tokens[0].kind === "word" && tokens[0].text === "doc" && tokens[1].text === "=" && tokens[2].kind === "string" && containsHan(tokens[2].text)) return true;
  if (tokens.length < 5 || tokens[0].kind !== "word" || tokens[0].text !== "cfg_attr" || tokens[1].text !== "(" || tokens.at(-1).text !== ")") return false;
  if (matchingClose(tokens, 1) !== tokens.length - 1) return false;
  const argumentsList = splitTopLevelArguments(tokens.slice(2, -1));
  return argumentsList.length >= 2 && argumentsList.slice(1).some(attributeMetaHasChineseDoc);
}

function hasChineseItemDoc(tokens, prefixStart) {
  let cursor = prefixStart - 1;
  while (cursor >= 0) {
    const token = tokens[cursor];
    if (token.kind === "doc_outer") { if (containsHan(token.text)) return true; cursor -= 1; continue; }
    const start = attributeStart(tokens, cursor);
    if (start !== null) {
      if (attributeMetaHasChineseDoc(tokens.slice(start + 2, cursor))) return true;
      cursor = start - 1;
      continue;
    }
    break;
  }
  return false;
}

function declarationName(tokens, keywordIndex) {
  const next = tokens[keywordIndex + 1];
  if (!next || next.kind !== "word") return null;
  if (next.text === "r" && tokens[keywordIndex + 2]?.text === "#" && tokens[keywordIndex + 3]?.kind === "word") return `r#${tokens[keywordIndex + 3].text}`;
  return next.text;
}

function inspectSource(relative, source) {
  const [tokens, lexicalErrors] = lex(source);
  lexicalErrors.push(...delimiterErrors(tokens));
  const skipped = macroBodyIndexes(tokens);
  const violations = [];
  let declarations = 0;
  tokens.forEach((token, index) => {
    if (skipped.has(index) || token.kind !== "word" || !DECLARATION_KINDS.has(token.text)) return;
    const name = declarationName(tokens, index);
    if (name === null) return;
    declarations += 1;
    if (!hasChineseItemDoc(tokens, declarationPrefixStart(tokens, index))) {
      violations.push({ path: relative, line: token.line, column: token.column, kind: token.text, name, reason: "missing_chinese_outer_doc" });
    }
  });
  return [declarations, violations, lexicalErrors];
}

function stripTomlComment(line) {
  let quote = null;
  let escaped = false;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    if (quote === '"' && escaped) { escaped = false; continue; }
    if (quote === '"' && character === "\\") { escaped = true; continue; }
    if (quote !== null && character === quote) { quote = null; continue; }
    if (quote === null && (character === '"' || character === "'")) { quote = character; continue; }
    if (quote === null && character === "#") return line.slice(0, index);
  }
  return line;
}

function parseStringArray(raw) {
  const value = raw.trim();
  if (!value.startsWith("[") || !value.endsWith("]")) throw new Error("expected string array");
  const items = [];
  let cursor = 1;
  while (cursor < value.length - 1) {
    while (/[\s,]/u.test(value[cursor])) cursor += 1;
    if (cursor >= value.length - 1) break;
    const quote = value[cursor];
    if (quote !== '"' && quote !== "'") throw new Error("expected quoted string");
    cursor += 1;
    let item = "";
    while (cursor < value.length) {
      const character = value[cursor];
      if (character === quote) { cursor += 1; break; }
      if (quote === '"' && character === "\\") {
        const escaped = value[cursor + 1];
        const replacements = { n: "\n", r: "\r", t: "\t", '"': '"', "\\": "\\" };
        if (!(escaped in replacements)) throw new Error("unsupported escape in string array");
        item += replacements[escaped];
        cursor += 2;
      } else { item += character; cursor += 1; }
    }
    items.push(item);
    while (/\s/u.test(value[cursor] ?? "")) cursor += 1;
    if (cursor < value.length - 1 && value[cursor] !== ",") throw new Error("expected comma in string array");
  }
  return items;
}

function readManifest(target, workspaceRoot) {
  const relative = path.relative(workspaceRoot, target).split(path.sep).join("/");
  const stat = lstatOrNull(target);
  if (stat?.isSymbolicLink()) return [null, [`Cargo manifest 不得是符号链接: ${relative}`]];
  let payload;
  try { payload = readFileSync(target); } catch (error) { return [null, [`无法读取 Cargo manifest ${relative}: ${error.message}`]]; }
  if (payload.includes(0)) return [null, [`Cargo manifest 含 NUL 字节: ${relative}`]];
  let text;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(payload); }
  catch (error) { return [null, [`Cargo manifest 无法解析: ${relative}: ${error.message}`]]; }
  try {
    const manifest = { package: null, workspace: null };
    const lines = text.split(/\r?\n/);
    let section = null;
    for (let index = 0; index < lines.length; index += 1) {
      const line = stripTomlComment(lines[index]).trim();
      if (!line) continue;
      const sectionMatch = /^\[([^\]]+)\]$/.exec(line);
      if (sectionMatch) {
        section = sectionMatch[1].trim();
        if (section === "package") manifest.package = {};
        if (section === "workspace") manifest.workspace = manifest.workspace ?? {};
        continue;
      }
      if (section === "workspace") {
        const assignment = /^(members|exclude)\s*=\s*(.*)$/.exec(line);
        if (assignment) {
          let raw = assignment[2];
          let depth = [...raw].filter((character) => character === "[").length - [...raw].filter((character) => character === "]").length;
          while (depth > 0 && index + 1 < lines.length) {
            index += 1;
            const continuation = stripTomlComment(lines[index]);
            raw += `\n${continuation}`;
            depth += [...continuation].filter((character) => character === "[").length - [...continuation].filter((character) => character === "]").length;
          }
          manifest.workspace[assignment[1]] = parseStringArray(raw);
        }
      }
    }
    return [manifest, []];
  } catch (error) { return [null, [`Cargo manifest 无法解析: ${relative}: ${error.message}`]]; }
}

function packageRoots(root) {
  const manifestPath = path.join(root, "Cargo.toml");
  if (!lstatOrNull(manifestPath)) return [[], ["项目根缺少 Cargo.toml"]];
  const [manifest, errors] = readManifest(manifestPath, root);
  if (manifest === null) return [[], errors];
  const roots = manifest.package !== null ? [root] : [];
  if (manifest.workspace !== null) {
    const members = manifest.workspace.members ?? [];
    const excludes = manifest.workspace.exclude ?? [];
    if (!Array.isArray(members) || members.some((item) => typeof item !== "string")) return [[], [...errors, "Cargo workspace.members 必须是字符串列表"]];
    if (!Array.isArray(excludes) || excludes.some((item) => typeof item !== "string")) return [[], [...errors, "Cargo workspace.exclude 必须是字符串列表"]];
    for (const pattern of members) {
      const patternParts = pattern.split(/[\\/]/);
      if (path.isAbsolute(pattern) || patternParts.includes("..")) { errors.push(`Cargo workspace member 不得越界: ${pattern}`); continue; }
      const matches = globSync(pattern, { cwd: root, withFileTypes: false }).sort();
      if (matches.length === 0) errors.push(`Cargo workspace member 未匹配任何路径: ${pattern}`);
      for (const match of matches) {
        const candidate = path.join(root, match);
        const relative = path.relative(root, candidate).split(path.sep).join("/");
        if (excludes.some((excluded) => path.matchesGlob(relative, excluded))) continue;
        if (lstatOrNull(candidate)?.isSymbolicLink()) { errors.push(`Cargo workspace member 不得是符号链接: ${relative}`); continue; }
        let resolved;
        try {
          resolved = realpathSync(candidate);
          const boundary = path.relative(root, resolved);
          if (boundary === ".." || boundary.startsWith(`..${path.sep}`) || path.isAbsolute(boundary)) throw new Error("outside workspace root");
        } catch (error) { errors.push(`Cargo workspace member 无法安全解析: ${relative}: ${error.message}`); continue; }
        const cargo = path.join(resolved, "Cargo.toml");
        const cargoStat = lstatOrNull(cargo);
        if (!lstatOrNull(resolved)?.isDirectory() || !cargoStat?.isFile() || cargoStat.isSymbolicLink()) {
          errors.push(`Cargo workspace member 缺少普通 Cargo.toml: ${relative}`);
          continue;
        }
        const [memberManifest, memberErrors] = readManifest(cargo, root);
        errors.push(...memberErrors);
        if (memberManifest !== null && memberManifest.package === null) errors.push(`Cargo workspace member 缺少 [package]: ${relative}`);
        else if (memberManifest !== null) roots.push(resolved);
      }
    }
  }
  if (roots.length === 0 && errors.length === 0) errors.push("项目根未解析出任何 Cargo package");
  return [[...new Set(roots)].sort(), errors];
}

function rustSources(packageRoot, workspaceRoot) {
  const sources = [];
  const errors = [];
  const buildScript = path.join(packageRoot, "build.rs");
  const buildStat = lstatOrNull(buildScript);
  if (buildStat) {
    const relative = path.relative(workspaceRoot, buildScript).split(path.sep).join("/");
    if (buildStat.isSymbolicLink() || !buildStat.isFile()) errors.push(`Rust 构建脚本必须是普通文件: ${relative}`);
    else sources.push(buildScript);
  }
  function walk(directory, relativeDirectory) {
    let entries;
    try { entries = readdirSync(directory).sort(); }
    catch (error) { errors.push(`无法枚举 Rust 源码目录 ${relativeDirectory}: ${error.message}`); return; }
    for (const name of entries) {
      const candidate = path.join(directory, name);
      const relative = path.relative(workspaceRoot, candidate).split(path.sep).join("/");
      let stat;
      try { stat = lstatSync(candidate); } catch (error) { errors.push(`无法检查 Rust 源码 ${relative}: ${error.message}`); continue; }
      if (stat.isSymbolicLink()) errors.push(`Rust 源码${stat.isDirectory() ? "目录" : ""}不得是符号链接: ${relative}`);
      else if (stat.isDirectory()) walk(candidate, relative);
      else if (name.endsWith(".rs")) {
        if (stat.isFile()) sources.push(candidate);
        else errors.push(`Rust 源码不是普通文件: ${relative}`);
      }
    }
  }
  for (const directoryName of SOURCE_DIRECTORIES) {
    const directory = path.join(packageRoot, directoryName);
    const stat = lstatOrNull(directory);
    if (!stat) continue;
    const relative = path.relative(workspaceRoot, directory).split(path.sep).join("/");
    if (stat.isSymbolicLink()) errors.push(`Rust 源码目录不得是符号链接: ${relative}`);
    else if (!stat.isDirectory()) errors.push(`Rust 源码路径不是目录: ${relative}`);
    else walk(directory, relative);
  }
  if (sources.length === 0 && errors.length === 0) {
    errors.push(`Cargo package 未找到 Rust 源码: ${path.relative(workspaceRoot, packageRoot).split(path.sep).join("/") || "."}`);
  }
  return [[...new Set(sources)].sort(), errors];
}

export function inspectProject(root) {
  const [canonical, errors] = canonicalRoot(root);
  const report = { ok: false, root: String(root), checkedPackages: 0, checkedRustFiles: 0, checkedDeclarations: 0, violations: [], errors };
  if (canonical === null) return report;
  report.root = canonical;
  const [packages, packageErrors] = packageRoots(canonical);
  report.errors.push(...packageErrors);
  report.checkedPackages = packages.length;
  for (const packageRoot of packages) {
    const [sources, sourceErrors] = rustSources(packageRoot, canonical);
    report.errors.push(...sourceErrors);
    for (const sourcePath of sources) {
      const relative = path.relative(canonical, sourcePath).split(path.sep).join("/");
      let payload;
      try { payload = readFileSync(sourcePath); } catch (error) { report.errors.push(`无法读取 Rust 源码 ${relative}: ${error.message}`); continue; }
      if (payload.includes(0)) { report.errors.push(`Rust 源码含 NUL 字节: ${relative}`); continue; }
      let source;
      try { source = new TextDecoder("utf-8", { fatal: true }).decode(payload); }
      catch (error) { report.errors.push(`Rust 源码不是 UTF-8: ${relative}: ${error.message}`); continue; }
      report.checkedRustFiles += 1;
      const [declarations, violations, lexicalErrors] = inspectSource(relative, source);
      report.checkedDeclarations += declarations;
      report.violations.push(...violations);
      report.errors.push(...lexicalErrors.map((error) => `${relative}: ${error}`));
    }
  }
  if (report.checkedRustFiles > 0 && report.checkedDeclarations === 0) report.errors.push("未发现受中文注释门禁管理的 Rust 声明");
  report.violations.sort((left, right) => left.path.localeCompare(right.path) || left.line - right.line || left.column - right.column);
  report.errors.sort();
  report.ok = report.errors.length === 0 && report.violations.length === 0;
  return report;
}

export function main(argumentsList = process.argv.slice(2), inspector = inspectProject) {
  const options = parseArguments(argumentsList);
  const report = inspector(options.root);
  if (options.json) process.stdout.write(`${JSON.stringify(report)}\n`);
  else {
    report.errors.forEach((error) => process.stderr.write(`ERROR: ${error}\n`));
    report.violations.forEach((violation) => process.stderr.write(
      `ERROR: Rust 声明缺少紧邻的中文文档注释: ${violation.path}:${violation.line} ${violation.kind} ${violation.name}\n`,
    ));
    if (report.ok) process.stdout.write(`Rust Chinese comment check passed: ${report.checkedPackages} package(s), ${report.checkedRustFiles} file(s), ${report.checkedDeclarations} declaration(s).\n`);
  }
  if (report.errors.length > 0) return 2;
  return report.ok ? 0 : 1;
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  try { process.exitCode = main(); }
  catch (error) { process.stderr.write(`ERROR: ${error.message}\n`); process.exitCode = 2; }
}
