---
name: desktop-manage-user-tasks
description: 用户可见 Task 的兼容入口；统一委托 desktop-task-workflow 管理项目开关、创建、真实绑定、进度和恢复。
---

# 左侧 Task 兼容入口

本入口仅为既有调用保留。完整读取并调用 [$desktop-task-workflow](../desktop-task-workflow/SKILL.md)，由其按项目已记录选择处理创建、绑定、进度和恢复；详细合同唯一位于该 Skill 的 [Task 合同](../desktop-task-workflow/references/task-contract.md)。

项目自动开关默认关闭；普通请求不自动建左侧子树。用户明确的新建请求或已记录启用选择直接复用，不重复审批。环境与内部并行同样由统一入口解析，本文件不复制状态机、描述模板或工作树规则。
