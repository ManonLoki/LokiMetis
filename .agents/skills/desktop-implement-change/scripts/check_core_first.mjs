#!/usr/bin/env node

/** 检查 Rust workspace 的 core-first 确定性依赖边界。 */

import { readFileSync, realpathSync, statSync } from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import { readDependencyLockPolicy } from "./project_lock_policy.mjs";

const ADAPTER_SUFFIXES = ["_cli", "_tui", "_mcp", "_gui"];
const INTERFACE_CRATE_NAMES = new Set([
  "clap", "crossterm", "dialoguer", "gpui", "gpui-pre", "gpui-kit", "gpui-component", "muda", "ratatui", "rmcp", "tauri", "tauri-build",
  "termion", "tray-icon", "tui-realm", "tui-realm-stdlib", "tuirealm", "winit",
]);
const INTERFACE_CRATE_PREFIXES = ["tauri-plugin-", "gpui-"];

function sortedUnique(values) { return [...new Set(values)].sort(); }

function workspacePackages(metadata) {
  const packages = metadata.packages;
  const workspaceMembers = metadata.workspace_members;
  if (!Array.isArray(packages)) return [[], ["cargo metadata 缺少 packages 数组"]];
  if (!Array.isArray(workspaceMembers) || workspaceMembers.length === 0) {
    return [[], ["cargo metadata 缺少非空 workspace_members 数组"]];
  }
  const packagesById = new Map();
  const errors = [];
  for (const packageValue of packages) {
    if (packageValue === null || Array.isArray(packageValue) || typeof packageValue !== "object") {
      errors.push("cargo metadata packages 包含非对象成员");
      continue;
    }
    if (typeof packageValue.id !== "string" || packageValue.id.length === 0) {
      errors.push("cargo metadata package 缺少稳定 id");
      continue;
    }
    packagesById.set(packageValue.id, packageValue);
  }
  const members = [];
  for (const memberId of workspaceMembers) {
    if (typeof memberId !== "string" || !packagesById.has(memberId)) {
      errors.push(`workspace member 未出现在 packages 中: ${JSON.stringify(memberId)}`);
      continue;
    }
    members.push(packagesById.get(memberId));
  }
  members.sort((left, right) => String(left.name ?? "").localeCompare(String(right.name ?? "")));
  return [members, errors];
}

function dependencies(packageValue) {
  if (!Array.isArray(packageValue.dependencies)) return [];
  return packageValue.dependencies
    .filter((dependency) => dependency !== null && !Array.isArray(dependency) && typeof dependency === "object")
    .sort((left, right) => {
      const leftKey = [left.name ?? "", left.kind ?? "", left.target ?? "", left.rename ?? ""].map(String).join("\0");
      const rightKey = [right.name ?? "", right.kind ?? "", right.target ?? "", right.rename ?? ""].map(String).join("\0");
      return leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0;
    });
}

function isInterfaceCrate(packageName) {
  return INTERFACE_CRATE_NAMES.has(packageName) || INTERFACE_CRATE_PREFIXES.some((prefix) => packageName.startsWith(prefix));
}

function dependencyScope(dependency) {
  const kind = dependency.kind || "normal";
  const target = dependency.target || "all-targets";
  const aliasText = dependency.rename ? `, alias=${dependency.rename}` : "";
  return `kind=${kind}, target=${target}${aliasText}`;
}

function hasLibraryTarget(packageValue) {
  return Array.isArray(packageValue.targets) && packageValue.targets.some(
    (target) => target && typeof target === "object" && Array.isArray(target.kind) && target.kind.includes("lib"),
  );
}

function packageDirectory(packageValue) {
  return typeof packageValue.manifest_path === "string" && packageValue.manifest_path
    ? path.dirname(path.resolve(packageValue.manifest_path))
    : null;
}

function pointsToWorkspacePackage(dependency, packageValue) {
  if (dependency.source !== null && dependency.source !== undefined) return false;
  const directory = packageDirectory(packageValue);
  return typeof dependency.path === "string" && dependency.path.length > 0 && directory !== null
    && path.resolve(dependency.path) === directory;
}

function localDependencyGraph(packagesByName) {
  const graph = new Map();
  for (const [packageName, packageValue] of packagesByName) {
    const targets = new Set();
    for (const dependency of dependencies(packageValue)) {
      const dependencyName = dependency.name;
      if (typeof dependencyName === "string" && packagesByName.has(dependencyName)
        && pointsToWorkspacePackage(dependency, packagesByName.get(dependencyName))) {
        targets.add(dependencyName);
      }
    }
    graph.set(packageName, [...targets].sort());
  }
  return graph;
}

function reachablePaths(start, graph) {
  const paths = new Map([[start, [start]]]);
  const pending = [start];
  for (let index = 0; index < pending.length; index += 1) {
    const packageName = pending[index];
    for (const target of graph.get(packageName) ?? []) {
      if (!paths.has(target)) {
        paths.set(target, [...paths.get(packageName), target]);
        pending.push(target);
      }
    }
  }
  return paths;
}

export function validateMetadata(metadata) {
  if (metadata === null || Array.isArray(metadata) || typeof metadata !== "object") {
    return ["cargo metadata 根必须是 JSON 对象"];
  }
  const [packages, errors] = workspacePackages(metadata);
  const names = packages.map((packageValue) => packageValue.name);
  if (names.some((name) => typeof name !== "string" || !name)) {
    errors.push("workspace package 缺少非空 name");
    return sortedUnique(errors);
  }
  const packageNames = names.map(String);
  if (packageNames.length !== new Set(packageNames).size) errors.push("workspace package name 必须唯一");
  const corePackages = packages.filter((packageValue) => String(packageValue.name ?? "").endsWith("_core"));
  if (corePackages.length !== 1) {
    errors.push(`workspace 必须且只能包含一个以 _core 结尾的共享核心 package，实际为 ${corePackages.length}`);
    return sortedUnique(errors);
  }
  const corePackage = corePackages[0];
  const coreName = String(corePackage.name);
  const projectId = coreName.slice(0, -"_core".length);
  if (!projectId) errors.push("共享核心 package 缺少项目标识前缀");
  if (!hasLibraryTarget(corePackage)) errors.push(`共享核心 ${coreName} 必须公开 lib target`);
  if (packageDirectory(corePackage) === null) errors.push(`共享核心 ${coreName} 缺少可解析的 manifest_path`);

  const adapterPackages = packages.filter((packageValue) => ADAPTER_SUFFIXES.some((suffix) => String(packageValue.name ?? "").endsWith(suffix)));
  if (adapterPackages.length === 0) {
    errors.push("workspace 至少需要一个 _cli/_tui/_mcp/_gui adapter package");
    return sortedUnique(errors);
  }
  const adapterNames = new Set(adapterPackages.map((packageValue) => String(packageValue.name)));
  const expectedAdapterNames = new Set(ADAPTER_SUFFIXES.map((suffix) => `${projectId}${suffix}`));
  for (const adapterName of [...adapterNames].sort()) {
    if (!expectedAdapterNames.has(adapterName)) errors.push(`adapter package ${adapterName} 与共享核心项目标识 ${projectId} 不一致`);
  }

  const packagesByName = new Map(packages.map((packageValue) => [String(packageValue.name), packageValue]));
  const graph = localDependencyGraph(packagesByName);
  for (const [packageName, reachablePath] of [...reachablePaths(coreName, graph)].sort(([left], [right]) => left.localeCompare(right))) {
    const pathText = reachablePath.join(" -> ");
    if (packageName !== coreName && adapterNames.has(packageName)) {
      errors.push(`共享核心 ${coreName} 不得依赖 adapter ${packageName} (workspace 路径: ${pathText})`);
      continue;
    }
    for (const dependency of dependencies(packagesByName.get(packageName))) {
      if (typeof dependency.name !== "string") errors.push(`workspace package ${packageName} 含缺少 name 的依赖`);
      else if (isInterfaceCrate(dependency.name)) {
        errors.push(`共享核心 ${coreName} 不得经 workspace 路径 ${pathText} 引入接口框架 ${dependency.name} (${dependencyScope(dependency)})`);
      }
    }
  }

  for (const adapterPackage of adapterPackages) {
    const adapterName = String(adapterPackage.name);
    const packageDependencies = dependencies(adapterPackage);
    const directCore = packageDependencies.filter((dependency) => dependency.name === coreName
      && (dependency.kind === null || dependency.kind === undefined)
      && (dependency.target === null || dependency.target === undefined)
      && !Boolean(dependency.optional ?? false)
      && pointsToWorkspacePackage(dependency, corePackage));
    if (directCore.length === 0) {
      errors.push(`adapter ${adapterName} 必须以非可选、非 target-specific 的普通依赖直接依赖 workspace 内的 ${coreName}`);
    }
    for (const [packageName, reachablePath] of [...reachablePaths(adapterName, graph)].sort(([left], [right]) => left.localeCompare(right))) {
      if (adapterNames.has(packageName) && packageName !== adapterName) {
        errors.push(`adapter ${adapterName} 不得依赖 adapter ${packageName} (workspace 路径: ${reachablePath.join(" -> ")})`);
      }
    }
  }
  return sortedUnique(errors);
}

export function loadCargoMetadata(workspaceRoot, { cargo = "cargo", timeoutSeconds = 60 } = {}) {
  let root;
  try { root = realpathSync(workspaceRoot); } catch (error) { throw new Error(`无法解析 workspace 根: ${error.message}`); }
  const manifest = path.join(root, "Cargo.toml");
  try { if (!statSync(manifest).isFile()) throw new Error(); }
  catch { throw new Error(`workspace 根缺少 Cargo.toml: ${root}`); }
  const lockPolicy = readDependencyLockPolicy(root);
  const args = ["metadata", ...(lockPolicy === "tracked" ? ["--locked"] : []), "--no-deps", "--format-version", "1", "--manifest-path", manifest];
  const result = spawnSync(cargo, args, { cwd: root, encoding: "utf8", timeout: timeoutSeconds * 1000, maxBuffer: 64 * 1024 * 1024 });
  if (result.error?.code === "ENOENT") throw new Error(`无法执行 Cargo: ${cargo}`);
  if (result.error?.code === "ETIMEDOUT") throw new Error(`cargo metadata 在 ${timeoutSeconds} 秒后超时`);
  if (result.error) throw new Error(`无法执行 Cargo: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`cargo metadata 失败: ${(result.stderr || result.stdout || "无诊断").trim() || "无诊断"}`);
  let metadata;
  try { metadata = JSON.parse(result.stdout); }
  catch (error) { throw new Error(`cargo metadata 输出不是合法 JSON: ${error.message}`); }
  if (metadata === null || Array.isArray(metadata) || typeof metadata !== "object") throw new Error("cargo metadata 输出根必须是 JSON 对象");
  return metadata;
}

function parseArgs(argv) {
  const options = { workspaceRoot: process.cwd(), metadataFile: null, cargo: "cargo", timeoutSeconds: 60, json: false };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--json") options.json = true;
    else {
      const value = argv[index + 1];
      if (value === undefined) throw new Error(`${token} requires a value`);
      if (token === "--workspace-root") options.workspaceRoot = value;
      else if (token === "--metadata-file") options.metadataFile = value;
      else if (token === "--cargo") options.cargo = value;
      else if (token === "--timeout-seconds") {
        if (!/^[0-9]+$/.test(value)) throw new Error("--timeout-seconds 必须为正整数");
        options.timeoutSeconds = Number(value);
      }
      else throw new Error(`unsupported argument: ${token}`);
      index += 1;
    }
  }
  return options;
}

function sortObject(value) {
  if (Array.isArray(value)) return value.map(sortObject);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortObject(value[key])]));
  return value;
}

function emitToolError(message, jsonOutput) {
  if (jsonOutput) process.stdout.write(`${JSON.stringify(sortObject({ ok: false, toolError: message, errors: [] }))}\n`);
  else process.stderr.write(`ERROR: ${message}\n`);
}

export function main(argv = process.argv.slice(2)) {
  let args;
  try { args = parseArgs(argv); }
  catch (error) { emitToolError(error.message, argv.includes("--json")); return 2; }
  if (!Number.isInteger(args.timeoutSeconds) || args.timeoutSeconds <= 0) {
    emitToolError("--timeout-seconds 必须为正整数", args.json);
    return 2;
  }
  let metadata;
  try {
    if (args.metadataFile !== null) {
      const metadataPath = realpathSync(args.metadataFile);
      const metadataText = new TextDecoder("utf-8", { fatal: true }).decode(readFileSync(metadataPath));
      metadata = JSON.parse(metadataText);
      if (metadata === null || Array.isArray(metadata) || typeof metadata !== "object") throw new Error("metadata 文件根必须是 JSON 对象");
    } else metadata = loadCargoMetadata(args.workspaceRoot, { cargo: args.cargo, timeoutSeconds: args.timeoutSeconds });
  } catch (error) { emitToolError(error.message, args.json); return 2; }
  const errors = validateMetadata(metadata);
  if (args.json) process.stdout.write(`${JSON.stringify(sortObject({ ok: errors.length === 0, toolError: null, errors }))}\n`);
  else if (errors.length > 0) errors.forEach((error) => process.stderr.write(`ERROR: ${error}\n`));
  else process.stdout.write("core-first dependency check passed\n");
  return errors.length > 0 ? 1 : 0;
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) process.exitCode = main();
