#!/usr/bin/env node
/** 为下游 Harness 工程升级生成三方计划并安全更新既有受管文件。 */

import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { buildPlan, stableJson, writeNewPlan } from "./harness_upgrade_core.mjs";
import { applyPlan } from "./harness_upgrade_mutation.mjs";
import { recordLock } from "./harness_upgrade_record.mjs";
import { UpgradeError } from "./harness_upgrade_safety.mjs";

function parseCommand(argv) {
  const command = argv[0];
  if (!["plan", "apply", "record"].includes(command)) throw new UpgradeError("必须提供 plan、apply 或 record 子命令");
  let options;
  if (command === "plan") options = Object.fromEntries(["source-root", "source-version", "source-commit", "candidate-root", "target-root", "ownership", "lock", "output"].map((name) => [name, { type: "string" }]));
  else if (command === "apply") options = { plan: { type: "string" }, approval: { type: "string" }, path: { type: "string" } };
  else options = { plan: { type: "string" }, "source-version": { type: "string" }, "source-commit": { type: "string" }, approval: { type: "string" }, bootstrap: { type: "boolean", default: false }, "resolved-manual": { type: "string", multiple: true, default: [] } };
  let values;
  try { ({ values } = parseArgs({ args: argv.slice(1), options, strict: true })); } catch (error) { throw new UpgradeError(error.message); }
  const required = command === "plan" ? ["source-root", "source-version", "source-commit", "candidate-root", "target-root", "ownership", "lock"] : (command === "apply" ? ["plan", "approval", "path"] : ["plan", "source-version", "source-commit", "approval"]);
  if (required.some((name) => !values[name])) throw new UpgradeError("缺少必需命令参数");
  return { command, values };
}

export function execute(argv) {
  const { command, values } = parseCommand(argv);
  if (command === "plan") {
    const result = buildPlan(values["source-root"], values["source-version"], values["source-commit"], values["candidate-root"], values["target-root"], values.ownership, values.lock);
    if (values.output) writeNewPlan(values.output, result, [result.candidate_root, result.target_root]);
    return { result, exitCode: result.blocked ? 2 : 0 };
  }
  if (command === "apply") return { result: applyPlan(values.plan, values.approval, values.path), exitCode: 0 };
  return { result: recordLock({ plan: values.plan, sourceVersion: values["source-version"], sourceCommit: values["source-commit"], approval: values.approval, bootstrap: values.bootstrap, resolvedManual: values["resolved-manual"] }), exitCode: 0 };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try { const { result, exitCode } = execute(process.argv.slice(2)); process.stdout.write(`${stableJson(result)}\n`); process.exitCode = exitCode; }
  catch (error) { process.stderr.write(`${stableJson({ ok: false, error: error.message })}\n`); process.exitCode = 2; }
}
