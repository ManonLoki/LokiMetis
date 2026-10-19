---
name: desktop-configure-agent-policy
description: 用户明确修改已初始化下游的永久 Agent 能力偏好时，校验并原子更新指定字段与确认事实；临时任务选择不触发，发布后动作使用其专用 Skill。
---

# 配置 Agent 能力偏好

只处理 `superpowers`、`user_owned_tasks`、`task_worktrees`、`parallel_worktree_subagents`、`acceptance_smoke`、`e2e_hint` 的明确永久选择。读取 `docs/AGENT_POLICY.md` 的字段语义和“初始化与持久化”；Harness 源 pending 模板、初始化、临时任务选择不由本入口写入。

`user_owned_tasks` 是项目唯一自动创建左侧子会话的开关，默认关闭；用户明确选择后由本入口记录，历史全局 Task Tree 要求不绕过项目值。Task 与 Worktree 详细行为统一委托 [$desktop-task-workflow](../desktop-task-workflow/SKILL.md)。明确新建一个 Task 只授权该结果，不自动永久打开开关，也不改无关能力。

## 执行

1. 确认目标是独立终端下游 Git 根，用户明确提供字段与 enabled/disabled 新值；复用本次选择，不为“开始写入”“完成自检”或“提交”再索取同一批准。`post_release_action` 委托 `$desktop-switch-post-release-action`；真实范围、凭据或不可逆副作用变化仍按实际授权处理。
2. 用 helper 的 `inspect` 只读获取当前值；缺失、非法字段失败关闭；合法 schema 3/4 保持只读兼容，task_worktrees 返回 selection_required，不填默认值。schema 4 原五项配置继续可用；schema 3 先补选发布动作至 schema 4。
3. 新旧相同按 no-op 报告，不写确认元数据或 ADR。不同则复用本次用户确认来源/日期及原因，按 `$desktop-implement-change` 建立开发周期和必要下游版本分类。
4. 从项目根执行以下命令，绝对路径与参数使用本次真实事实：

   ```text
   node .agents/skills/desktop-configure-agent-policy/scripts/agent_policy.mjs inspect --project-root <absolute-root>
   node .agents/skills/desktop-configure-agent-policy/scripts/agent_policy.mjs set --project-root <absolute-root> --field <capability> --value <enabled|disabled> --expected-value <old-value> --confirmed-by <confirmation-source> --confirmed-at <ISO-date> --confirmed-user-choice
   ```

5. helper 与发布后动作 writer 共用策略锁，原子更新指定字段和 confirmed_by/confirmed_at，保留其他字段及正文。复读 `inspect` 确认结果；正常短锁竞争由调用方有界等待后复读重试，旧值变化先核对原授权，不用覆盖参数绕过真实冲突。自动检查通过后继续，不设置人工 BOUND 审批。
6. 仅 `changed: true` 时调用 `$desktop-record-adr`，复用同一上下文记录确认来源、前后值、原因、影响、恢复条件及适用 change_id/required_version；完成必要相关验证与本地提交。helper 不伪造或自动批准 ADR。

## schema 4 受限工作树迁移

“开启/关闭 Task 独立工作树”仅映射 task_worktrees；“开启/关闭左侧 Task”仅映射 user_owned_tasks。先合并受保护正文的双环境合同并清除无条件 Worktree 规则，逐字保留既有 frontmatter；用户明确选择复用不重问。计算实际策略 SHA-256，再执行：

```text
node .agents/skills/desktop-configure-agent-policy/scripts/agent_policy.mjs migrate-task-worktrees --project-root <absolute-root> --value <enabled|disabled> --expected-schema 4 --expected-sha256 <sha256> --confirmed-by <source> --confirmed-at <ISO-date> --confirmed-user-choice
```

共享锁内复读合法 schema 4 与摘要，只改 schema 至 5 并增加明确的 task_worktrees，逐字保留原五项能力、动作、原确认元数据及正文；新选择的来源、日期、原因、影响与恢复条件由本流程调用 $desktop-record-adr 写入，不由 helper 伪造。schema 3 的动作补选只迁移到 4，再独立补选工作树；不能替用户顺便填新值。schema 5 同值 no-op，不同值使用正常 set。正文未完成迁移不能报告完整完成。

## 边界与输出

开关只影响后续适用任务，不迁移、中断或删除既有 Task/Worktree，不重写已冻结发布，不立即运行所启用能力。报告 changed/no-op、指定字段前后值、确认事实、ADR/提交与实际检查；不暴露凭据或改变全局配置。
