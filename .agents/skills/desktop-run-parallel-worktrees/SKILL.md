---
name: desktop-run-parallel-worktrees
description: 在用户明确要求且策略允许时，将当前已绑定的左侧 Task 拆为具有非重叠写入所有权的内部 Subagent Git Worktree；通过项目/source 绑定、guard、postflight、生命周期登记和资源保留阻止越界与资源遗失。
---

# 运行并行 Worktree

将一个左侧 Task 内部的写入批次拆为可见进度、隔离工作树和可审查结果，不得丢失用户修改，也不得把必需工作留在后台。

本 Skill 只管理当前 Task 内部的 Subagent 单元。它不调用 `create_thread`，不创建新的左侧 Task，也不受 `user_owned_tasks` 开关控制。内部 agent thread 与单元 Worktree 不使用 `Task {序号} | {当前进度} | {单一结果}` 标题合同、不占用项目 Task 序号；宿主技术 `task_name`、`codex/unit-*` 分支和父 Task 标题互相独立。父 Task 自行反映整体进度；不得把两个左侧 Task 安排进同一 Worktree，也不得用单元分支替代父 Task 的 source 分支。

## 策略与适用性

1. 读取 `docs/AGENT_POLICY.md`。只有用户在当前请求中明确要求并行 Subagent/Worktree，且 `parallel_worktree_subagents: enabled` 时才继续；日常开发不得仅因持久策略启用而自动增加并行步骤。值为 `disabled` 时使用单 Agent。
2. `enabled` 表示许可，不表示执行请求。用户已明确要求并行时，仍只有至少两个写入单元具备不重叠所有权、明确整合顺序和干净且已提交的 Git 基线才使用本 Skill；否则解释原因并使用单 Agent。
3. 产品定义、规划、只读调查、普通证据复核、构建、收集和发布元数据工作，不会仅因偏好已启用而自动并行。当前存在 feature 开发周期不构成拒绝 sibling `codex/unit-*` 的理由；不得读取或解释旧分支链状态来阻断并行。
4. 当前 source 可以是通过 BOUND 门禁的左侧 Git Task Worktree，也可以是自动 Task 关闭时直接工作的当前 Local checkout；两者都必须与保存项目拥有相同规范化 Git common-dir、出现在 Worktree registry 中并附着于登记的具名生命周期分支。不得把名称前缀、路径祖先关系或分支历史形态当作并行条件。
5. 策略许可不授权原任务以外的提交、合并、删除、推送、发布、凭据或外部副作用。

## 准备

1. 读取用户要求并行的任务说明、Agent Policy、工程规则和相关文件；只有用户已要求持久计划且确实存在时才读取 Work Plan，只有产品边界或长期决定相关时才读取 Product Spec/ADR。
2. 为每个单元定义稳定的 ASCII task/unit 技术标识、结果、一个或多个非空 repo-relative 写入所有权、依赖、验证和整合顺序。task 标识必须等于父 Task 描述中的独立 `feature-summary`；不得把父 Task 显示标题传给 `spawn_agent.task_name`、`--task`、`--unit` 或 Git ref。`.`、绝对/越界路径、符号链接逃逸、同一单元内冗余祖先/后代，以及不同活动单元间任何相同或祖先/后代所有权都失败关闭；需要重叠的工作改为串行执行。
3. 记录两个角色路径：`--project-root` 是 Codex 保存项目的 primary checkout；`--source-worktree` 是当前左侧 Task 的实际 Git 顶层，在 Worktree 模式下是独立 Worktree，在 Local 模式下可以与 `--project-root` 精确相同。所有非 `inspect` 命令都同时提供二者。task 标识 `<task>` 只用于本工作流的单元身份，每个单元分支为 `codex/unit-<task>-<unit>`；source 分支取创建时 registry 中的精确具名分支，不从 task 标识推导。
4. 从当前 source Worktree 的精确 cwd 为每个单元运行一次 `create`，每个所有权重复传入 `--write-target`：

   ```text
   node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs create --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit> --write-target <owned-path> [--write-target <owned-path> ...]
   ```

   helper 要求保存项目 primary、source registry/branch、common-dir、外部普通目录容器和干净且已提交的 source HEAD 全部匹配；source 可以是已登记的独立 Worktree，也可以是精确的保存项目 primary Local checkout，但不能是未登记的其他目录。从 source HEAD 创建单元并把不可变身份、基线和所有权登记在 Git common-dir。单元 Worktree 建立成功后，helper 必须立即调用项目内 `$desktop-manage-git-lifecycle` 的 `node scripts/git_lifecycle.mjs track-worktree --project-root <absolute-project-root> --worktree <absolute-unit-worktree>`，由该 helper 自行解析并登记精确具名分支。登记返回非零状态时，以稳定错误 `lifecycle_worktree_tracking_failed` 失败，回滚本次新建的单元状态、Worktree、分支与空容器，不能留下未登记资源。绝不得自动暂存、贮藏或提交用户修改。
5. 把 `create` 返回的精确 `worktreePath` 和 ownership 交给对应内部 Subagent。隐藏 Subagent 不占用用户可见项目序列，也不伪装用户可见 Task；协调方只使用受限技术 `task_name` 和清晰职责描述。Agent 不是独占仓库；它必须把所有文件命令和编辑限定在该 Worktree，保留其他 Agent 的改动，不扩大所有权。在任何编辑、暂存或提交前，从单元精确 cwd 运行：

   ```text
   node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs guard --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit> --write-target <intended-path> [--write-target <intended-path> ...]
   ```

   guard 只接受创建时已登记 ownership 的相同路径或子路径，并先检查此前所有实际改动；它不能替代宿主沙箱。Subagent 必须返回变更文件、开发检查、阻断项和整合说明。

## 可见执行与整合

1. 在开始、阻断、单元完成、整合和验证节点公开简洁状态。同步等待每个必需结果；内部 agent thread 只报告技术标识和父 Task，不得使用用户可见 Task 标题、占用项目序号、把自己称为新的左侧 Task或调用 `create_thread`。
2. 范围、所有权或安全假设变化时，停止或重新分派相关单元。不得静默创建替代 Agent。
3. Subagent 完成后、整合前，从其单元精确 cwd 运行 postflight：

   ```text
   node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs verify --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit>
   ```

   postflight 必须证明从登记 `baseHead` 到当前 HEAD 的 committed 路径，以及 staged、unstaged、untracked 和 rename 的源/目标路径都在 ownership 内。任何越界先由对应单元修复；不得在协调 source 中掩盖或手工忽略。
4. 检查每项差异和证据，拒绝无关编辑、缺失测试、过时注释、敏感信息、绝对本机路径和无证据支持的声明。协调方可以按依赖顺序使用普通、可逆 Git 操作提前整合已验证提交，也可以保留各登记分支，交给用户明确“推送”或“发布”时的生命周期 helper 统一普通合并；不得要求快进、祖先关系、冻结 OID 或其他历史形态。语义冲突由协调方处理，绝不得让多个 Subagent 竞态修改。
5. 整合后只运行本次变化必需的非空单元/回归测试；非代码变更只运行必要替代验证。不得因并行本身追加格式、lint、静态、构建、冒烟、E2E 或完整验收。
6. 只有整合后的必要测试通过，才能把存在的 Todo 标记为 `done`。随后返回 `$desktop-implement-change` 收口；普通构建保持开发流程。用户明确要求 Git 发布时才经 `$desktop-prepare-release` 合并本地主分支并打 tag；已初始化下游随后按本次冻结的 `post_release_action` 执行并复核后续路径，Harness 源后续动作由用户当次决定。

## 单元收口与资源保留

清理前从保存项目 primary 的精确 cwd 检查：

```text
node .agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs inspect --project-root <absolute-project-root>
```

只收口由本工作流拥有、postflight 通过且状态干净的精确单元。从 source Worktree 的精确 cwd 运行；收口不检查分支祖先关系，也不接收可被用来自证历史形态的 ref：

```text
node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs remove --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit>
```

`remove` 只删除该并行单元自己在 Git common-dir 下的状态登记，返回 `resourcesRetained: true`、`worktreeRetained: true` 与 `branchRetained: true`，表示资源仍由生命周期 helper 精确登记和保留；它不得删除 Worktree 或本地/远端分支。绝不得强制收口状态不干净、身份/所有权缺失或不匹配、越界单元，也不得手工删除 common-dir 中的登记来绕过检查。单元分支是否已提前整合不影响收口。

这些已由生命周期 helper 精确登记的 Worktree 与分支在 Git 发布后仍保留。Git 发布仅普通合并登记分支到本地默认主分支并创建、复读当前版本 tag；已初始化下游随后按本次冻结的 `post_release_action` 执行并复核后续路径，Harness 源后续动作由用户当次决定。清理是另一项需明确授权、精确核对登记资源的操作；当前发布命令不负责清理，不能因为发布或推送完成就手工删除。
