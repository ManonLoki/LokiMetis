#!/usr/bin/env node
/** 只读更新日志入口；在任何文件访问前拒绝写入命令与额外参数。 */
import path from "node:path";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { loadDocument, normalizeDisplayVersion, renderDocument } from "../../desktop-prepare-release/scripts/release_notes.mjs";

/** 严格解析两个只读命令，不透传原维护 CLI。 */
export function parseArguments(argv) {
  const [command, ...rest] = argv;
  const allowed = command === "check" ? ["--file", "--expected-version"] : command === "render" ? ["--file", "--locale"] : null;
  if (!allowed) throw new Error("command must be check or render");
  const values = new Map();
  for (let index = 0; index < rest.length; index += 2) {
    const key = rest[index];
    const value = rest[index + 1];
    if (!allowed.includes(key) || values.has(key)) throw new Error(`unknown or duplicate argument: ${key}`);
    if (!value || value.startsWith("--")) throw new Error(`missing value: ${key}`);
    values.set(key, value);
  }
  if (!values.has("--file")) throw new Error("--file is required");
  if (command === "render" && !["zh-CN", "en-US"].includes(values.get("--locale"))) throw new Error("--locale must be zh-CN or en-US");
  return { command, file: path.resolve(values.get("--file")), expectedVersion: values.get("--expected-version"), locale: values.get("--locale") };
}

/** 委托既有读取/渲染库，参数错误为 2，文档错误为 1。 */
export function main(argv = process.argv.slice(2), io = process) {
  let args;
  try { args = parseArguments(argv); }
  catch (error) { io.stderr.write(`${error.message}\n`); return 2; }
  try {
    const document = loadDocument(args.file);
    if (args.command === "render") io.stdout.write(`${renderDocument(document, args.locale)}\n`);
    else {
      const actual = document.releases[0]?.version ?? null;
      if (args.expectedVersion !== undefined && actual !== normalizeDisplayVersion(args.expectedVersion)) throw new Error(`latest release version ${actual} does not match expected ${normalizeDisplayVersion(args.expectedVersion)}`);
      io.stdout.write(`release-notes.valid=true retained=${document.releases.length}\n`);
    }
    return 0;
  } catch (error) { io.stderr.write(`release-notes.error=${error.message}\n`); return 1; }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) process.exitCode = main();
