---
name: desktop-manage-version
description: 管理下游产品的语义化版本门禁、base-100 进位、发布周期功能锁、记录冲突纠正与 bug-fix 稳定 ID 去重；不改变 Harness 时间版本。
---

# 管理下游产品版本

为已初始化下游产品计算并提交语义化版本。根 `Cargo.toml` 的 `[workspace.package].version` 是当前版本唯一事实源，`.harness/version-state.json` 只保存发布周期、去重和变更所需版本状态；Harness 自身的 `Version.md` 与 `YYYYMMDDHHMM` 方案不属于本 Skill。

既有项目若有独立决定安装包版本的 JSON 文件，可在根 `Cargo.toml` 的 `[workspace.metadata.agent-first-harness]` 声明 `version-mirrors = ["tauri.conf.json", "package.json"]` 等实际相对路径。每个镜像须为普通非符号链接 JSON 文件，顶层只有一个可定位的字符串 `version`；`init`、`check`、`plan` 和 `apply` 均要求它与根 Cargo 相等，`apply` 升版时同步这些镜像，写入失败尝试恢复已写文件并报告不能恢复的路径。未声明的文件不擅自修改；项目应声明所有实际决定安装包版本的镜像。首次接入时先调查并对齐已有冲突，不用版本提升掩盖旧漂移。

## 固定语义

- 版本必须是无预发布/构建元数据的 `MAJOR.MINOR.PATCH`。新生成的 Minor/Patch 数位在 `0..99`，采用 base-100 自动进位：`0.0.99 -> 0.1.0`、`0.99.99 -> 1.0.0`；Major 不受 99/100 的业务上限约束，但必须处于 Cargo `u64` 范围 `0..18446744073709551615`。没有更高数位可承接的自动进位必须在写入前失败关闭。
- `major` 只接受用户对精确目标 Major 的明确批准，写成 `N.0.0`，目标必须高于当前版本规范化后的 Major，并把本发布周期的功能提升视为已包含；不得由 Agent 推断。base-100 自动进位到 Major 是数值计算例外，不需要也不代表显式 Major 批准。
- 已完成正式发布并确认版本 tag 的下游开始新改动时，先判断它是否提出新目标、新能力、新使用场景或扩大既有需求；只要有合理依据认为是新需求，就优先归类为 `feature`，再考虑 `bug-fix`。请求名称中的“修复”“优化”不能取代这个判断。确认不属于新需求后，问题修复或用户可感知优化仍可归为 `bug-fix`，无需证明违背既有承诺。产品范围本身不明确且不同答案会改变实施边界时，先确认需求内容；不得仅因分类措辞含糊而把疑似新需求降为 Patch。
- 一个正式发布周期内，第一个已完成的新功能把当前版本提升一个 Minor 数位并把 Patch 归零，必要时进位 Major；后续功能只记录当时所需版本，不再因功能重复提升。只有真实正式发布成功才解锁下一周期的首次功能提升。
- 对已核实的同一事件、受保护版本状态或 Git 发布事实冲突，使用独立稳定 ID 的 `record-reconciliation`。当周期内首个此类纠正强制提升一个新的 Minor 并把 Patch 归零，即使本周期已完成过功能提升；同周期后续纠正共用这一 Minor，不重复提升。纠正会占用本周期功能锁；随后功能不再额外提升。纠正 ID 与冲突证据长期保留在受保护状态的 `applied_reconciliations`：跨发布周期用同一 ID 和证据重试仍幂等，换证据则失败；同一冲突事件 ID 也不能换纠正 ID 再次升版，新的独立冲突事实须用新的事件 ID。旧状态缺此字段时按空历史兼容读取，不伪造既往纠正。疑似新需求仍先按 `feature` 判断，不能用记录纠正代替真实新功能。仅有历史 `required_version` 低于最终发布版本、不同事件的记录值不同或未经核实的猜测不属于此类冲突。
- 每个已完成且具有新稳定 ID 的问题修复或用户可感知优化统一使用 `bug-fix`，把 Patch 提升一个数位并自动进位；这一提升不受功能锁影响。相同 ID 的重复修改、重试或补充处理永不再次提升；正式发布后确认的回归必须使用新的稳定 ID，才可提升。
- 查询、诊断、复现、未完成或重复处理，以及不改变可观察行为的重构、内部优化、测试补强、文档、格式和内部清理都属于 `maintenance`，不改变版本。
- 维护不改变版本；不得把维护换名为功能或缺陷修复来绕过分类。
- Minor/Patch 固定为 `0..99`，不兼容任何历史下位分量 `100`；Cargo 或状态 `target_version` 中一旦出现 `100`，`init`、`check`、`plan`、`maintenance` 和 `apply` 一律失败关闭，没有可读取或延迟规范化的旧值例外，必须先手动把版本改回 `0..99` 才能继续任何操作。
- `init` 在新项目尚未建立独立 Git 时创建初始状态；在已存在的独立 Git 项目缺少状态时，它只接受用户对本次不可逆历史缺口明确批准后的 `--migration-approved`。迁移以当前合法 Cargo 版本建立空周期基线，并报告旧 `pending_changes`、`applied_bug_ids` 与 `last_release` 无法恢复；不得推断、伪造或从 Git/发布日志回填这些历史。
- 普通构建、`pending` 候选、验收和失败发布都不重置周期。只有本地默认主分支已完成整合，且 `v{版本}-{YYYYMMDD}` 本地 tag 与其 HEAD 精确一致，才视为正式发布成功。`finalize-release` 在该发布之后创建的下一条 `feature-*` 分支上、首次改动前运行；它只读核对 Git common-dir 生命周期状态中的 `lastRelease`、默认主分支、所有同版本本地 tag 与调用者提供的源码提交，然后清空待发布变化并允许下一周期的首个功能再次提升 Minor，保留历史 `bug-fix` 稳定 ID。下游按本次发布冻结的 `postReleaseAction` 本地打包或推送 `release` 分支；这些后续动作即使尚未执行、失败或待复核，也不影响 `finalize-release`，不另切分 SemVer 功能周期。
- Product Spec、ADR、Changelog 或 Work Plan 只有被自身事件独立触发时才记录相应 `change_id` 的 `required_version`。版本变化不得为普通缺陷或维护任务强制创建这些文档；`required_version` 是变化完成当时的最低所需版本，最终发布版本高于它是合法历史，不属于记录不一致，不得据此回写旧记录或另行提升 Minor。真实冲突须先调查并修正其事实来源，明确两份针对同一事件的记录及不同观察值；若根 Cargo 与受保护状态漂移，所有 `plan`/`apply` 均失败关闭，必须先经单独受控修复使二者一致，本 Skill 不自动覆盖状态或以升版掩盖漂移。纠正后运行适用的全部验证并完成上述 Git 发布，才算口径统一修正结束。

## 工作流程

1. 除 `init` 外，先确认目标是已初始化的下游项目、项目根是独立 Git 顶层目录，并读取根 `Cargo.toml` 与 `.harness/version-state.json`。新项目初始化时，`init` 是唯一允许在独立 Git 建立前运行的命令，必须在根 Cargo 初始版本写入后、GUI E2E 与一次性裁剪前生成受保护状态，且不得借此提前初始化 Git：

   ```text
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs init --project-root .
   ```

   对版本管理上线前已经初始化、已有独立 Git 但缺少状态的旧下游，先说明无法恢复的历史并取得用户对本次迁移的明确批准，再执行：

   ```text
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs init --project-root . --migration-approved
   ```

   该命令只使用当前合法 Cargo 版本创建空 `pending_changes`/`applied_bug_ids` 基线；没有 `--migration-approved` 时必须零写入失败。Harness 升级器只能在工程层升级完成后要求此路径，不能自行执行或写入 protected 状态。

2. 实施前按上述新需求优先顺序分类，再用 `plan` 只读计算所需版本。功能、`bug-fix`（问题修复或用户可感知优化）、记录纠正和显式 Major 变化必须提供稳定 `change_id`；Major 还必须提供用户批准的精确值和 `--user-approved`。`record-reconciliation` 的 `--conflict-event-id` 标识同一个真实事件，两个 `--conflict-record-*` 分别是相对路径或 `git:refs/...` 加 `#字段`，观察值必须不同；仅在实际逐项核实并确认两者冲突后才传 `--conflict-confirmed`。这些结构化证据会保留在受保护状态的对应待发布条目和长期纠正历史中，同一 ID 重试必须提供完全相同的证据。`plan` 绝不写入文件。例如：

   ```text
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs plan --project-root . --kind feature --change-id FEAT-123
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs plan --project-root . --kind bug-fix --change-id BUG-456
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs plan --project-root . --kind major --change-id BREAK-7 --major 2 --user-approved
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs plan --project-root . --kind maintenance --change-id INVESTIGATE-9
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs plan --project-root . --kind record-reconciliation --change-id RECON-9 --conflict-event-id EVENT-9 --conflict-record-a docs/adr/decision.md#result --conflict-value-a 0.2.0 --conflict-record-b docs/changelog/entry.md#result --conflict-value-b 0.3.0 --conflict-confirmed
   ```

3. 完成实现或事实纠正并让本次相关非空测试通过后，使用相同参数把 `plan` 改为 `apply`。不得在查询、诊断、复现、失败尝试或实现尚未完成时提前 `apply`。命令会更新当前版本与周期状态；重复 `change_id` 且证据一致时返回幂等结果。记录纠正升版后，须按冲突涉及的事实来源执行完整适用验证、修正同一事件的全部受影响记录，并完成本地主分支及 tag 发布；不得把 `apply` 成功误报为发布完成。
4. 被独立触发的 Product Spec、ADR、Changelog 或 Work Plan 使用命令 JSON 中的 `required_version`，并同时保存稳定 `change_id`。如果其他已完成变化随后提高目标版本，不回写为虚假的原始需求版本。
5. 构建、候选收集、验收与发布准备在任何测试或打包前只运行一致性检查，不得借机提升或重置：

   ```text
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs check --project-root . --phase build
   ```

6. Git 发布流程合并到本地默认主分支并创建、复读匹配的版本 tag 后，先由生命周期 helper 留下 `lastRelease`，再从该 tag/主分支 HEAD 创建下一条 `feature-*` 分支。进入该新分支但尚未做其他改动时，才运行：

   ```text
   node .agents/skills/desktop-manage-version/scripts/version_gate.mjs finalize-release --project-root . --released-version 0.2.1 --source-commit <40-hex-tagged-main-head> --release-succeeded
   ```

## 失败关闭

- 根 Cargo 版本与状态目标不一致、状态缺失/损坏、旧下游迁移未传 `--migration-approved`、路径为符号链接、Git 根不独立、版本格式不受支持、Minor/Patch 超出支持范围 `0..99`、Major 超出 Cargo `u64` 范围、缺少稳定 ID、记录冲突证据不完整或未经确认、当前 `pending_changes` 中的 ID 被不同类别复用、相同纠正 ID 的证据改变、历史 `bug-fix` ID 被改作其他提升类别，或显式 Major 未明确批准时停止；不得手工绕过状态文件。Major 不设 99/100 的业务上限，新生成 Minor/Patch 的 99 边界必须自动进位；只有最高 Major 超出 `u64::MAX` 时才因没有更高数位而失败。
- 不得修改成员 crate 的独立版本；所有成员继续使用 `version.workspace = true`。
- 不得把 `finalize-release` 当作构建收尾，也不得仅凭单独 tag、候选存在或发布尝试开始就重置周期；生命周期登记、默认主分支、所有同版本 tag、源码提交和下一开发分支起点须同时一致。

## 输出

报告分类、`change_id`、变更前后版本、`required_version`、是否实际提升、幂等原因、周期是否仍有待发布变化，以及实际执行的检查。版本门禁不代表构建、验收或发布已经完成。
