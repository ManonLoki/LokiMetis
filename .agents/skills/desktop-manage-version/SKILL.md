---
name: desktop-manage-version
description: 管理下游产品的自动语义化版本门禁、base-100 进位、发布周期功能锁、变更所需版本与 bug-fix 稳定 ID 去重；不改变 Harness 时间版本。
---

# 管理下游产品版本

为已初始化下游产品计算并提交语义化版本。根 `Cargo.toml` 的 `[workspace.package].version` 是当前版本唯一事实源，`.harness/version-state.json` 只保存发布周期、去重和变更所需版本状态；Harness 自身的 `Version.md` 与 `YYYYMMDDHHMM` 方案不属于本 Skill。

## 固定语义

- 版本必须是无预发布/构建元数据的 `MAJOR.MINOR.PATCH`。新生成的 Minor/Patch 数位在 `0..99`，采用 base-100 自动进位：`0.0.99 -> 0.1.0`、`0.99.99 -> 1.0.0`；Major 不受 99/100 的业务上限约束，但必须处于 Cargo `u64` 范围 `0..18446744073709551615`。没有更高数位可承接的自动进位必须在写入前失败关闭。
- `major` 只接受用户对精确目标 Major 的明确批准，写成 `N.0.0`，目标必须高于当前版本规范化后的 Major，并把本发布周期的功能提升视为已包含；不得由 Agent 推断。base-100 自动进位到 Major 是数值计算例外，不需要也不代表显式 Major 批准。
- 一个正式发布周期内，第一个已完成的新功能把当前版本提升一个 Minor 数位并把 Patch 归零，必要时进位 Major；后续功能只记录当时所需版本，不再因功能重复提升。只有真实正式发布成功才解锁下一周期的首次功能提升。
- 每个已完成且具有新稳定 ID 的问题修复或用户可感知优化统一使用 `bug-fix`，把 Patch 提升一个数位并自动进位；这一提升不受功能锁影响。相同 ID 的重复修改、重试或补充处理永不再次提升；正式发布后确认的回归必须使用新的稳定 ID，才可提升。
- 查询、诊断、复现、未完成或重复处理，以及不改变可观察行为的重构、内部优化、测试补强、文档、格式和内部清理都属于 `maintenance`，不改变版本。
- 维护不改变版本；不得把维护换名为功能或缺陷修复来绕过分类。
- 历史 Cargo 与状态 `target_version` 中的 Minor/Patch `100` 继续可读；Harness 升级、`check`、`plan` 和 `maintenance` 不改写它们。只有下一次确实提升版本的 `feature`、`bug-fix` 或显式 `major` 的 `apply` 才先按 base-100 规范化当前 Cargo 与 `target_version`，再应用本次变化；`cycle_base_version`、`last_release` 和既有 `pending_changes.required_version` 保留原始证据值。
- 普通构建、`pending` 候选、验收和失败发布都不重置周期。只有正式发布已经成功后才能执行 `finalize-release`；它清空待发布变化并允许下一周期的首个功能再次提升 Minor，但保留历史 `bug-fix` 稳定 ID。
- Product Spec、ADR、Changelog 或 Work Plan 只有被自身事件独立触发时才记录相应 `change_id` 的 `required_version`。版本变化不得为普通缺陷或维护任务强制创建这些文档；最终发布版本允许高于早先记录的最低所需版本。

## 工作流程

1. 除初始化流程唯一的 `init` 外，先确认目标是已初始化的下游项目、项目根是独立 Git 顶层目录，并读取根 `Cargo.toml` 与 `.harness/version-state.json`。状态缺失只允许在初始化流程中运行；此时 `init` 是唯一允许在独立 Git 建立前运行的命令，必须在根 Cargo 初始版本写入后、GUI E2E 与一次性裁剪前生成受保护状态，且不得借此提前初始化 Git：

   ```text
   python3 .agents/skills/desktop-manage-version/scripts/version_gate.py init --project-root .
   ```

2. 实施前用 `plan` 只读计算分类和所需版本。功能、`bug-fix`（问题修复或用户可感知优化）和显式 Major 变化必须提供稳定 `change_id`；Major 还必须提供用户批准的精确值和 `--user-approved`。`plan` 绝不写入文件。例如：

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

- 根 Cargo 版本与状态目标不一致、状态缺失/损坏、路径为符号链接、Git 根不独立、版本格式不受支持、Minor/Patch 超出兼容读取范围 `0..100`、Major 超出 Cargo `u64` 范围、缺少稳定 ID、当前 `pending_changes` 中的 ID 被不同类别复用、历史 `bug-fix` ID 被改作其他提升类别，或显式 Major 未明确批准时停止；不得手工绕过状态文件。Major 不设 99/100 的业务上限，新生成 Minor/Patch 的 99 边界必须自动进位；只有最高 Major 超出 `u64::MAX` 时才因没有更高数位而失败。
- 不得修改成员 crate 的独立版本；所有成员继续使用 `version.workspace = true`。
- 不得把 `finalize-release` 当作构建收尾，也不得仅凭 tag、候选存在或发布尝试开始就重置周期。

## 输出

报告分类、`change_id`、变更前后版本、`required_version`、是否实际提升、幂等原因、周期是否仍有待发布变化，以及实际执行的检查。版本门禁不代表构建、验收或发布已经完成。
