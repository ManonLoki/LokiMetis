---
name: desktop-task-workflow
description: 统一管理项目可选的左侧 Task、Worktree、内部并行、授权继承和恢复；自动 Task 默认关闭，已授权范围自主执行，独立工作树按所有权并行。
---

# Task 与工作树工作流

本 Skill 是 Task 和 Worktree 操作合同的唯一来源。项目选择保存在 [Agent Policy](../../../docs/AGENT_POLICY.md)；本技能及其引用被保留不代表能力已开启。

## 先解析项目选择

1. 读取用户级 `AGENTS.md`、项目入口及 Agent Policy 字段语义，复用当前请求已经给出的授权。用户明确修订旧规则时采用新规则，不为历史全局 Task Tree 要求绕过项目开关。
2. `user_owned_tasks` 是自动创建左侧子会话的唯一项目开关，默认 `disabled`。关闭时在当前聊天完成普通工作，不自动创建 Task 子树；用户明确要求新建一个 Task 时仅创建该结果，不永久打开开关。Task Tree 不是已经核实的 Codex 应用设置名，不能据此声称修改了应用配置。
3. 用户明确“开启左侧 Task”“关闭左侧 Task”或“开启/关闭自动 Task 拆分”时，由 `$desktop-configure-agent-policy` 原子记录 `user_owned_tasks` 和确认事实；已有明确选择不重复询问。初始化把选择一次写入策略。仅有未落盘的旧全局默认不视为当前项目已启用。
4. `task_worktrees` 仅选择 Git 左侧 Task 的环境，默认 `disabled`；开启用 `environment.type=worktree`，关闭或非 Git 用 `environment.type=local`。它不触发 Task 创建。派发冻结环境，切换只影响后续 Task，不迁移既有 Task/Worktree。
5. `parallel_worktree_subagents` 仅控制当前结果内部的写入型 Subagent Worktree，不创建左侧 Task、不占项目编号，也不改变前两项选择。只有当前请求授权并行且该字段启用时执行；已授权的整批并行不逐单元重复审批。

## 按操作加载

- 创建、恢复、检查或更新左侧 Task：完整读取 [Task 合同](references/task-contract.md)，派发使用 [描述模板](references/task-description.md)。
- 分配 Git 环境、并行创建 Worktree 或运行内部单元：完整读取 [工作树执行](references/worktree-execution.md)。
- 仅切换永久开关：调用 `$desktop-configure-agent-policy`，无需加载创建状态机或创建 Task。
- 普通实现仍由 `$desktop-implement-change` 完成；分支、提交和发布分别使用其既有入口，不重复实现生命周期。

## 授权与自主执行

用户授权一个结果即授权其范围内必要的资料阅读、实现、同范围修复、相关开发检查和适用本地提交。子 Task 和内部单元继承明确范围、所有权、当前宿主真实权限与禁止事项；常规检查由 Agent 自检通过后直接继续，不再把 BOUND、guard、postflight、开始实施或本地提交变成人工批准点。

仅实质超出结果范围、引入未授权且难以撤销的副作用，或需要新的凭据/系统权限时请求用户决定。已授权的发布、推送或系统操作直接复用原授权，并完成各自必需检查，不因为拆分再次询问同一事项。自动审批复核拒绝时先在原授权内修复可纠正原因；仍被拒绝则准确报告动作和真实原因。

Skill、Task 描述和普通文件不提供宿主提权或批准 API。不得伪造“自我审批成功”、关闭安全机制、绕过工具拒绝、要求子 Agent 使用虚构的审批参数，或声称获得高于当前会话的权限。宿主实际提供权限配置能力时，才按用户已批准范围使用该能力并复读生效结果；当前工具未提供时如实报告限制。

## 结果边界与并行

一个 Task 固定一个明确结果和执行环境。诊断、局部设计、实现、开发回归及同范围返工组成同一结果，默认留在同一 Task；不按普通内部步骤强拆。只有用户明确要求阶段隔离时才分别派发调研、设计、实现、正式验收、安装发布或推送。增加外部副作用或触碰原禁止范围时重新核对授权，已被原请求包含的步骤直接继承。

正式验收有独立的责任和证据；开发单元/回归不能承担正式验收结论。用户另行要求正式验收、独立发布或独立推送时按该结果派发，不把必要开发检查升级成正式验收。

同一 Local cwd 串行占用；独立 Worktree 在输入已提交、写入所有权不重叠、依赖明确时可以并行，不设每项目单一写入型 active Task 限制。先一次清点并预留编号，再并行创建可独立建立的 Worktree/Task；仅编号预留、所有权预留和 Git 共享状态写入用短锁，实际 checkout、业务实现与开发检查不持有全局长锁。每个单元完成自己的检查即可继续，整合在依赖结果到齐后进行。

## 兼容与交付

`$desktop-manage-user-tasks`、`$desktop-manage-task-worktrees` 和 `$desktop-run-parallel-worktrees` 只保留兼容路由，详细合同不再分别复制。内部并行 helper 仍位于原 `desktop-run-parallel-worktrees/scripts/` 路径，现有调用继续工作。

报告真实 Task/环境身份、最终确认进度、结果、分支/提交、实际检查和剩余限制。queued 只表示 setup，失败保留原 Task 与证据；不凭相似标题接管、不重复创建、不把未执行检查写为通过。
