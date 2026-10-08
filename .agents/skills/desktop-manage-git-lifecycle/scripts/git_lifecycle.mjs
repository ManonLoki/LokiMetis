#!/usr/bin/env node
/** 以精确登记清单管理 Git 开发、推送和发布生命周期。 */

import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  LifecycleError,
  commandCheckPostRelease,
  commandInspect,
  commandStart,
  commandTrackWorktree,
  resolveRepository,
  withLifecycleStateLock,
} from "./git_lifecycle_core.mjs";
import { commandPublish, commandPushRelease, commandRelease } from "./git_lifecycle_publication.mjs";

export * from "./git_lifecycle_core.mjs";
export * from "./git_lifecycle_publication.mjs";

const COMMANDS = ["inspect", "start", "track-worktree", "publish", "release", "push-release", "check-post-release"];

/** 把 kebab-case CLI 选项转换成 camelCase。 */
function optionName(value) {
  return value.slice(2).replace(/-([a-z])/g, (_match, letter) => letter.toUpperCase());
}

/** 统一拒绝参数错误，不回显可能含本地路径的输入。 */
function invalidArguments() {
  throw new LifecycleError("invalid-argument", "Command arguments are invalid.");
}

/** 解析稳定子命令及显式选项。 */
export function parseArguments(argv) {
  if (argv.includes("--help") || argv.includes("-h")) return { help: true };
  const command = argv[0];
  if (!COMMANDS.includes(command)) invalidArguments();
  const allowed = {
    inspect: new Set(["projectRoot", "remote"]),
    start: new Set(["projectRoot", "summary", "remote"]),
    "track-worktree": new Set(["projectRoot", "worktree", "remote"]),
    publish: new Set(["projectRoot", "remote", "alsoRemote"]),
    release: new Set(["projectRoot", "version", "date", "releaseContextSha256"]),
    "push-release": new Set(["projectRoot", "remote"]),
    "check-post-release": new Set(["projectRoot", "action"]),
  }[command];
  const args = { command, alsoRemote: [] };
  for (let index = 1; index < argv.length; index += 1) {
    const token = argv[index];
    if (!token.startsWith("--")) invalidArguments();
    const name = optionName(token);
    if (!allowed.has(name)) invalidArguments();
    if (index + 1 >= argv.length || argv[index + 1].startsWith("--")) invalidArguments();
    const value = argv[index + 1];
    index += 1;
    if (name === "alsoRemote") args.alsoRemote.push(value);
    else {
      if (args[name] !== undefined) invalidArguments();
      args[name] = value;
    }
  }
  if (args.projectRoot === undefined) invalidArguments();
  if (command === "start" && args.summary === undefined) invalidArguments();
  if (command === "track-worktree" && args.worktree === undefined) invalidArguments();
  if (command === "release") {
    if (args.version === undefined || args.releaseContextSha256 === undefined) invalidArguments();
  }
  if (command === "push-release" && args.remote === undefined) invalidArguments();
  if (command === "check-post-release" && args.action === undefined) invalidArguments();
  return args;
}

/** 深度按键排序，使成功与失败始终输出稳定单行 JSON。 */
function sortedValue(value) {
  if (Array.isArray(value)) return value.map(sortedValue);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortedValue(value[key])]));
  }
  return value;
}

/** 向 stdout 写唯一一行紧凑 JSON。 */
function emit(payload) {
  process.stdout.write(`${JSON.stringify(sortedValue(payload))}\n`);
}

/** 执行一个命令，成功与失败都只写一行 JSON。 */
export async function main(argv = process.argv.slice(2)) {
  try {
    const args = parseArguments(argv);
    if (args.help) {
      emit({ status: "help", commands: COMMANDS });
      return 0;
    }
    const repository = resolveRepository(args.projectRoot);
    let operation;
    if (args.command === "inspect") operation = () => commandInspect(repository, args);
    else if (args.command === "start") operation = () => commandStart(repository, args);
    else if (args.command === "track-worktree") operation = () => commandTrackWorktree(repository, args);
    else if (args.command === "publish") operation = () => commandPublish(repository, args);
    else if (args.command === "push-release") operation = () => commandPushRelease(repository, args);
    else if (args.command === "check-post-release") operation = () => commandCheckPostRelease(repository, args);
    else operation = () => commandRelease(repository, args);
    const result = args.command === "inspect"
      ? await operation()
      : await withLifecycleStateLock(repository, operation);
    emit(result);
    return 0;
  } catch (error) {
    if (error instanceof LifecycleError) {
      emit({ status: "error", code: error.code, message: error.publicMessage });
      return 1;
    }
    emit({ status: "error", code: "internal-error", message: "Unexpected lifecycle failure." });
    return 1;
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  const interrupted = () => {
    emit({ status: "error", code: "interrupted", message: "Operation was interrupted." });
    process.exit(130);
  };
  process.once("SIGINT", interrupted);
  process.exitCode = await main();
  process.removeListener("SIGINT", interrupted);
}
