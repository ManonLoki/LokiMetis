---
name: desktop-manage-version
description: 管理下游产品的自动语义化版本门禁、发布周期状态、变更所需版本与重复缺陷去重；不改变 Harness 时间版本。
---

# 管理下游产品版本

为已初始化下游产品计算并提交语义化版本。根 `Cargo.toml` 的 `[workspace.package].version` 是当前版本唯一事实源，`.harness/version-state.json` 只保存发布周期、去重和变更所需版本状态；Harness 自身的 `Version.md` 与 `YYYYMMDDHHMM` 方案不属于本 Skill。

## 固定语义

- 版本必须是无预发布/构建元数据的 `MAJOR.MINOR.PATCH`，三个分量都在闭区间 `0..100`。任何溢出都停止并询问用户，不进位。
- `major` 只接受用户对精确目标 Major 的明确批准，写成 `N.0.0`，并把本发布周期的功能提升视为已包含；不得由 Agent 推断。
- 一个正式发布周期内，第一个已完成的新功能把当前版本提升为 `MAJOR.(MINOR+1).0`；后续功能只记录当时所需版本，不再因功能重复提升。Minor 提升必定把 Patch 归零。
- 每个已完成且具有新稳定缺陷 ID 的缺陷修复把 Patch 提升 1。相同缺陷 ID 的重复修改、重试或补充处理永不再次提升；正式发布后确认的回归必须使用新的回归缺陷 ID，才可提升。
- 缺陷查询、诊断、复现、未完成或重复修复尝试，以及不改变需求或修复结果的重构、测试补强、文档、格式和内部清理都属于 `maintenance`，不改变版本。
- 维护不改变版本；不得把维护换名为功能或缺陷修复来绕过分类。
- 普通构建、`pending` 候选、验收和失败发布都不重置周期。只有正式发布已经成功后才能执行 `finalize-release`；它清空待发布变化并允许下一周期的首个功能再次提升 Minor，但保留历史缺陷 ID。
- Product Spec、ADR、Changelog 或 Work Plan 只有被自身事件独立触发时才记录相应 `change_id` 的 `required_version`。版本变化不得为普通缺陷或维护任务强制创建这些文档；最终发布版本允许高于早先记录的最低所需版本。

## 工作流程

1. 除初始化流程唯一的 `init` 外，先确认目标是已初始化的下游项目、项目根是独立 Git 顶层目录，并读取根 `Cargo.toml` 与 `.harness/version-state.json`。状态缺失只允许在初始化流程中运行；此时 `init` 是唯一允许在独立 Git 建立前运行的命令，必须在根 Cargo 初始版本写入后、GUI E2E 与一次性裁剪前生成受保护状态，且不得借此提前初始化 Git：

   ```text
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py init --project-root .
   ```

2. 实施前用 `plan` 只读计算分类和所需版本。功能、缺陷修复和 Major 变化必须提供稳定 `change_id`；Major 还必须提供用户批准的精确值和 `--user-approved`。例如：

   ```text
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py plan --project-root . --kind feature --change-id FEAT-123
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py plan --project-root . --kind bug-fix --change-id BUG-456
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py plan --project-root . --kind major --change-id BREAK-7 --major 2 --user-approved
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py plan --project-root . --kind maintenance --change-id INVESTIGATE-9
   ```

3. 完成实现并让本次相关非空测试通过后，使用相同参数把 `plan` 改为 `apply`。不得在查询、诊断、复现、失败尝试或实现尚未完成时提前 `apply`。命令会原子更新当前版本与周期状态；重复 `change_id` 返回幂等结果。
4. 被独立触发的 Product Spec、ADR、Changelog 或 Work Plan 使用命令 JSON 中的 `required_version`，并同时保存稳定 `change_id`。如果其他已完成变化随后提高目标版本，不回写为虚假的原始需求版本。
5. 构建、候选收集、验收与发布准备在任何测试或打包前只运行一致性检查，不得借机提升或重置：

   ```text
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py check --project-root . --phase build
   ```

6. 只有正式发布的真实渠道操作已经成功、精确版本与 40 位源码提交已有证据后，才运行：

   ```text
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py finalize-release --project-root . --released-version 0.2.1 --source-commit <40-hex> --release-succeeded
   ```

## 失败关闭

- 根 Cargo 版本与状态目标不一致、状态缺失/损坏、路径为符号链接、Git 根不独立、版本格式不受支持、缺少稳定 ID、ID 被不同类别复用、Major 未明确批准或任一分量将超过 100时都停止；不得手工绕过状态文件。
- 不得修改成员 crate 的独立版本；所有成员继续使用 `version.workspace = true`。
- 不得把 `finalize-release` 当作构建收尾，也不得仅凭 tag、候选存在或发布尝试开始就重置周期。

## 输出

报告分类、`change_id`、变更前后版本、`required_version`、是否实际提升、幂等原因、周期是否仍有待发布变化，以及实际执行的检查。版本门禁不代表构建、验收或发布已经完成。
