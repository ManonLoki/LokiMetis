import { access, copyFile, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
export const SKILL_DIR = path.resolve(SCRIPT_DIR, "..");
export const TEMPLATE_DIR = path.join(SKILL_DIR, "assets", "skin-template");
export const SURFACES = Object.freeze([
  "home",
  "chat",
  "pull-requests",
  "sites",
  "automations",
  "plugins",
  "settings",
  "other",
]);

export function parseArgs(argv, { valueOptions = [], flags = [] } = {}) {
  const values = {};
  const positional = [];
  const valueSet = new Set(valueOptions);
  const flagSet = new Set(flags);
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) {
      positional.push(token);
      continue;
    }
    const key = token.slice(2);
    if (flagSet.has(key)) {
      values[key] = true;
      continue;
    }
    if (!valueSet.has(key)) throw new Error(`未知参数：${token}`);
    const value = argv[index + 1];
    if (value === undefined || value.startsWith("--")) throw new Error(`${token} 缺少值。`);
    values[key] = value;
    index += 1;
  }
  return { values, positional };
}

export function requireValue(values, key, label = key) {
  const value = values[key];
  if (typeof value !== "string" || !value.trim()) throw new Error(`缺少必填参数 --${key}（${label}）。`);
  return value;
}

export async function exists(target) {
  try {
    await access(target);
    return true;
  } catch {
    return false;
  }
}

export async function readUtf8(target, maximumBytes = 2 * 1024 * 1024) {
  const info = await stat(target);
  if (!info.isFile() || info.size === 0 || info.size > maximumBytes) {
    throw new Error(`文本文件必须非空且不超过 ${maximumBytes} 字节：${path.basename(target)}`);
  }
  return readFile(target, "utf8");
}

export async function readJson(target) {
  return JSON.parse(await readUtf8(target));
}

export async function writeJson(target, value) {
  await writeFile(target, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

export async function ensureDirectory(target) {
  await mkdir(target, { recursive: true });
}

export async function copy(source, destination) {
  await copyFile(source, destination);
}

export function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export function dataUrl(buffer, mime) {
  return `data:${mime};base64,${buffer.toString("base64")}`;
}

export function fail(message) {
  throw new Error(message);
}
