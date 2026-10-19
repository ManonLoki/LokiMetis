---
name: desktop-manage-task-worktrees
description: Task 独立工作树的兼容入口；统一委托 desktop-task-workflow 按冻结项目选择分配环境、并行创建与恢复。
---

# Task 工作树兼容入口

本入口仅为既有调用保留。完整读取并调用 [$desktop-task-workflow](../desktop-task-workflow/SKILL.md)，由其 [工作树执行](../desktop-task-workflow/references/worktree-execution.md) 处理已启用或冻结的 `task_worktrees` 环境。

保留本技能不启用 Task 或 Worktree；新 Git Task 按项目选择使用独立 Worktree 或 Local。已有授权直接继承，环境核对自动完成；本入口不分配编号、不再创建第二个 Task，也不另存审批或绑定合同。
