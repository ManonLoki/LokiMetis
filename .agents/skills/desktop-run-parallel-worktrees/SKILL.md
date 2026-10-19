---
name: desktop-run-parallel-worktrees
description: 内部 Subagent Worktree 的兼容入口；统一委托 desktop-task-workflow 继承授权、按所有权并行创建和执行，保留原 helper 路径。
---

# 内部并行工作树兼容入口

本入口仅为既有调用保留。完整读取并调用 [$desktop-task-workflow](../desktop-task-workflow/SKILL.md)，按其 [工作树执行](../desktop-task-workflow/references/worktree-execution.md) 处理内部并行单元、自动 guard/postflight、整合和资源保留。

原 `scripts/parallel_worktrees.mjs` 及测试路径保持兼容。内部单元不创建左侧 Task、不占项目编号；项目选择许可加当前已授权并行才执行。无依赖 Worktree 并行创建，共享状态使用短锁；正常同范围实现、修复、开发检查和本地提交不重复人工审批。本文件不复制具体命令或独立维护第二份合同。
