#!/usr/bin/env node
/** 仅更新用户明确选择的单项永久能力，保留发布动作与策略正文。 */
import path from "node:path";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";
import {
  ActionError, projectRoot, readPolicy, atomicWrite, withPolicyLock, validConfirmedAt,
} from "../../desktop-switch-post-release-action/scripts/post_release_action.mjs";

export const CAPABILITY_FIELDS = Object.freeze([
  "superpowers", "user_owned_tasks", "task_worktrees", "parallel_worktree_subagents", "acceptance_smoke", "e2e_hint",
]);

/** 保留合法旧策略读取；工作树缺失时不替用户补选。 */
function currentPolicy(root) {
  const policy = readPolicy(root, { validateActionSupport: false });
  return policy;
}

/** 返回可复核的能力和确认事实，不写入文件或创建锁。 */
function state(policy) {
  return {
    schema_version: Number(policy.schema),
    capabilities: Object.fromEntries(CAPABILITY_FIELDS.filter((field) => policy.fields.has(field)).map((field) => [field, policy.fields.get(field)])),
    task_worktrees: { status: policy.schema === "5" ? "configured" : "selection_required", value: policy.fields.get("task_worktrees") ?? null },
    post_release_action: policy.fields.get("post_release_action") ?? null,
    confirmed_by: policy.fields.get("confirmed_by"),
    confirmed_at: policy.fields.get("confirmed_at"),
  };
}

/** 检查当前已确认策略，不因缺少无关打包资源阻断能力读取。 */
export function inspect(root) { return state(currentPolicy(projectRoot(root))); }

/** 限定确认来源为非空普通标量，避免换行或 YAML 控制字符注入。 */
function validateConfirmation(by, at) {
  if (typeof by !== "string" || by !== by.trim() || !by || /^[?@`-]/u.test(by) || /[\x00-\x1f\x7f:#{}\[\]"'!&*|>]/u.test(by)
    || /^(?:pending|unknown|unset|n\/a)$/iu.test(by)) throw new ActionError("confirmed_by 必须是真实非空普通标量来源");
  if (typeof at !== "string" || !validConfirmedAt(at)) throw new ActionError("confirmed_at 必须是合法 ISO 日期或时间戳");
}

/** 在共享锁内核对旧值，只替换所选字段与本次确认元数据。 */
export function setCapability(rawRoot, { field, value, expectedValue, confirmedBy, confirmedAt, confirmedChoice = false }) {
  if (!confirmedChoice) throw new ActionError("写入要求 --confirmed-user-choice");
  if (!CAPABILITY_FIELDS.includes(field)) throw new ActionError("field 只能是六项能力；发布后动作使用专用入口");
  if (![value, expectedValue].every((item) => ["enabled", "disabled"].includes(item))) throw new ActionError("value 和 expected-value 必须是 enabled 或 disabled");
  validateConfirmation(confirmedBy, confirmedAt);
  const root = projectRoot(rawRoot);
  return withPolicyLock(root, () => {
    const policy = currentPolicy(root);
    if (policy.schema === "3") throw new ActionError("能力配置要求 schema_version: 4 或 5；先完成发布动作专用迁移");
    if (field === "task_worktrees" && policy.schema !== "5") throw new ActionError("task_worktrees 为 selection_required；使用 migrate-task-worktrees 明确补选");
    const previous = policy.fields.get(field);
    if (previous !== expectedValue) throw new ActionError("能力旧值已变化；重新读取并确认，不覆盖漂移");
    if (previous === value) return { ...state(policy), changed: false, field, previous_value: previous };
    const replacements = new Map([[field, value], ["confirmed_by", confirmedBy], ["confirmed_at", confirmedAt]]);
    const lines = policy.lines.map((line) => {
      const key = line.slice(0, line.indexOf(":"));
      return replacements.has(key) ? `${key}: ${replacements.get(key)}` : line;
    });
    const body = policy.normalized.slice(policy.normalized.indexOf("\n---\n", 4) + 5);
    atomicWrite(policy, `---\n${lines.join("\n")}\n---\n${body}`.replaceAll("\n", policy.newline));
    const verified = inspect(root);
    if (verified.capabilities[field] !== value || verified.post_release_action !== policy.fields.get("post_release_action")) {
      throw new ActionError("能力更新后复读不一致");
    }
    return { ...verified, changed: true, field, previous_value: previous };
  });
}

/** 拒绝仍要求无条件工作树的正文，迁移前由工程流程合并受保护章节。 */
function assertEnvironmentBody(policy) {
  const body = policy.normalized.slice(policy.normalized.indexOf("\n---\n", 4) + 5);
  const required = ["task_worktrees", "environment.type=local", "所选环境", "冻结环境", "串行"];
  const stale = ["Git 项目固定选择", "Git user-owned Task 固定使用独立 Worktree", "Git Task 只在自己的独立 Worktree 修改文件", "Git 使用独立 Task Worktree"];
  if (required.some((fragment) => !body.includes(fragment)) || stale.some((fragment) => body.includes(fragment))) {
    throw new ActionError("先合并 task_worktrees 所选环境合同，移除无条件 Worktree 规则；不得将迁移报告为完成");
  }
}

/** 在共享锁内仅从完整 v4 增加明确选择，逐字保留原确认事实与正文。 */
export function migrateTaskWorktrees(rawRoot, { value, expectedSchema, expectedSha256, confirmedBy, confirmedAt, confirmedChoice = false }) {
  if (!confirmedChoice) throw new ActionError("写入要求 --confirmed-user-choice");
  if (!["enabled", "disabled"].includes(value) || expectedSchema !== "4" || !/^[a-f0-9]{64}$/u.test(expectedSha256 ?? "")) throw new ActionError("迁移要求明确选择、expected-schema 4 与完整 SHA-256");
  validateConfirmation(confirmedBy, confirmedAt);
  const root = projectRoot(rawRoot);
  return withPolicyLock(root, () => {
    const policy = currentPolicy(root);
    assertEnvironmentBody(policy);
    if (policy.schema === "5") {
      if (policy.fields.get("task_worktrees") !== value) throw new ActionError("schema 5 不同值须使用正常 set 入口");
      return { ...state(policy), changed: false, previous_schema: 5 };
    }
    if (policy.schema !== expectedSchema || crypto.createHash("sha256").update(policy.bytes).digest("hex") !== expectedSha256) throw new ActionError("策略 schema 或摘要已变化；重新读取，不覆盖漂移");
    const lines = policy.lines.map((line) => line === "schema_version: 4" ? "schema_version: 5" : line);
    lines.splice(lines.findIndex((line) => line.startsWith("user_owned_tasks:")) + 1, 0, `task_worktrees: ${value}`);
    const body = policy.normalized.slice(policy.normalized.indexOf("\n---\n", 4) + 5);
    atomicWrite(policy, `---\n${lines.join("\n")}\n---\n${body}`.replaceAll("\n", policy.newline));
    const verified = inspect(root);
    if (verified.schema_version !== 5 || verified.capabilities.task_worktrees !== value) throw new ActionError("迁移后复读失败");
    return { ...verified, changed: true, previous_schema: 4 };
  });
}

/** 严格读取单次命令，不接受重复参数、未知字段或缺少参数值。 */
export function main(argv = process.argv.slice(2)) {
  const [command, ...rest] = argv;
  const keys = command === "set"
    ? ["--project-root", "--field", "--value", "--expected-value", "--confirmed-by", "--confirmed-at", "--confirmed-user-choice"]
    : command === "migrate-task-worktrees" ? ["--project-root", "--value", "--expected-schema", "--expected-sha256", "--confirmed-by", "--confirmed-at", "--confirmed-user-choice"] : ["--project-root"];
  if (!["inspect", "check", "set", "migrate-task-worktrees"].includes(command)) throw new ActionError("命令只能是 inspect、check、set 或 migrate-task-worktrees");
  const values = new Map();
  for (let index = 0; index < rest.length; index += 1) {
    const key = rest[index];
    if (!keys.includes(key) || values.has(key)) throw new ActionError("未知或重复参数");
    if (key === "--confirmed-user-choice") { values.set(key, true); continue; }
    const value = rest[++index];
    if (typeof value !== "string" || value.startsWith("--")) throw new ActionError("参数缺少值");
    values.set(key, value);
  }
  if (keys.some((key) => !values.has(key))) throw new ActionError("缺少必需参数");
  if (command === "migrate-task-worktrees") return migrateTaskWorktrees(values.get("--project-root"), {
    value: values.get("--value"), expectedSchema: values.get("--expected-schema"), expectedSha256: values.get("--expected-sha256"),
    confirmedBy: values.get("--confirmed-by"), confirmedAt: values.get("--confirmed-at"), confirmedChoice: values.get("--confirmed-user-choice"),
  });
  return command === "set" ? setCapability(values.get("--project-root"), {
    field: values.get("--field"), value: values.get("--value"), expectedValue: values.get("--expected-value"),
    confirmedBy: values.get("--confirmed-by"), confirmedAt: values.get("--confirmed-at"), confirmedChoice: values.get("--confirmed-user-choice"),
  }) : inspect(values.get("--project-root"));
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  try { process.stdout.write(`${JSON.stringify(main())}\n`); }
  catch (error) { process.stderr.write(`${JSON.stringify({ error: error.message })}\n`); process.exitCode = 2; }
}
