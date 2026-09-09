---
name: desktop-plan-change
description: 只在用户明确要求持久计划、任务需要跨会话交接，或发布/高风险工作确需协调时创建可直接实施的精简 Todo；日常开发不得自动调用。
---

# 规划变更

只在持久计划能解决真实协调问题时建立 Todo，不把计划变成日常开发前置步骤。

## 准入

1. 用户明确要求持久计划、任务需要跨会话交接，或发布/高风险工作存在多个必须协调的依赖时才调用本 Skill。
2. 多步骤、多模块、中等风险、可并行或 Agent 偏好本身不构成准入；日常开发直接交给 `$desktop-implement-change`。计划确实被触发时，每个会产生功能、缺陷修复或用户批准 Major 的 Todo 必须具有稳定 `change_id`，并记录 `$desktop-manage-version plan` 返回的 `required_version`；纯维护 Todo 明确记录版本不变即可。
3. 规划期间不编码、不建立额外写入工作区、不运行构建、冒烟或 E2E。若范围会改变产品目标、边界、约束或成功标准，先转 `$desktop-define-product`。

## 工作流程

1. 按需读取最新 Product Spec、Agent Policy、`docs/ENGINEERING_RULES.md`、相关 ADR、受影响代码/测试和存在时的活动 Work Plan；只读取当前协调所需事实。
2. 梳理当前行为、用户修改、依赖、风险及范围内/外边界，建立最少数量的 Todo。每项至少记录稳定 ID、`pending`/`in_progress`/`blocked`/`done` 状态、可观察预期行为、精确所有权、依赖和本次必要单元/回归测试或最小替代检查。
3. 对业务行为保持 core-first：先安排 core 用例及其核心成功/最高风险失败测试，再安排适配器映射；接口/宿主专属改动须记录其专属性理由。
4. 发布或高风险协调确需计划时，只记录待执行的选择解析、构建、E2E、验收依赖和失败回流标准；不得把真实候选、源码提交、运行编号、摘要、签名、公证或 `accepted`/`ready` 结论回写 Work Plan。候选事实只进入忽略的 `release/` 原子集合与最终回复，日常计划也不得自行增加这些步骤。
5. 创建或更新当天 Work Plan；新自然日综合仍有效事项。计划存在和 Todo 完成都不自动触发 Product Spec、ADR、Product Status、Verification 或 Changelog。

## 质量规则

- 使用仓库事实，不编造文件、命令、API、依赖或产物。
- 计划必须保持 core-first，优先最少 Todo 和最短依赖链，不创建形式化空批次。
- Todo 完成只依赖本次声明的实现与测试/检查；不得在执行时静默追加全仓检查、构建或验收。
