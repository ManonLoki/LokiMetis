---
name: desktop-upgrade-harness
description: 在已初始化的下游项目中安全更新由 Harness 维护的工程规则、保留的项目 Skills 和维护工具，不覆盖产品代码、项目记忆、身份、策略、许可证或本地决定。用于用户要求把下游项目的 Harness 工程层与明确提供的较新 Harness 源进行升级、同步、迁移、刷新或比较时。
---

# 升级 Harness

通过可审计的预览、冲突解决、应用、验证和基线记录闭环，更新一个终端下游项目。

## 工作流程

1. 读取下游的 `AGENTS.md`、`docs/AGENT_POLICY.md`、存在时日期最新的产品规格、产品状态、工作计划和 ADR、`docs/ENGINEERING_RULES.md`、`docs/TECH_DEBT.md`，以及存在时的 `.harness/upstream-lock.json`。读取源 Harness 的 `Version.md`、日期最新的产品规格、产品状态和 ADR，以及其验证命令。
2. 无法无歧义发现源 Harness 根目录时，要求用户明确指定。将源根目录和下游根目录解析为互不相同的规范绝对路径。要求每个现有 Git 仓库的顶层目录等于其声明根目录；拒绝符号链接根目录、嵌套歧义、源与目标相同，以及声明根目录以外的路径。
3. 读取 [所有权策略](references/ownership-policy.md)，并使用 [所有权清单](references/ownership-manifest.json)。将该清单视为最低保护策略，而不是复制每个匹配源文件的许可。
4. 检查下游 GUI 身份映射、目标平台、保留的 Skills、受保护 profile、已批准例外和持久 Agent 策略。构建只包含适用于该下游的工程文件候选树。排除每个 `protected` 或 `tombstone` 源路径，并把下游展示名称、标识符、crate 路径、固定 GUI 事实和其他已批准身份差异渲染到候选中；原始 Harness 源目录树绝不是有效候选树。源 Harness 中 `.agents/skills/desktop-implement-change/scripts/check_file_line_limits.py`、`check_rust_chinese_comments.py`、`check_core_first.py` 及其各自测试必须保留内容摘要一致的候选，升级器在生成计划时机械阻断遗漏或旧版占位内容。身份中立的 `docs/design_standards/**` 作为 managed 工程标准传播；下游 `docs/GUI_APP_PROFILE.md` 和 ADR 中已批准的产品像素/例外保持 protected，并在标准匹配时优先，升级不得用新旧 Harness 缺省覆盖。GUI 固定工程层始终同步 `$desktop-add-gui-adapter`（其中内联 system-locale、window-state 与 dialog 固定子契约）、`$desktop-prepare-gui-app-identity`、`$desktop-prepare-gui-support-surfaces`、`$desktop-build-tauri-release` 与 `$desktop-test-gui-release-performance`；目标平台包含 Windows 时还同步 `$desktop-build-tauri-local-install`。dialog 继续保持主窗口 `dialog:default` 且不授权 fs；托盘、单实例、深链接、全局快捷键、通知和开机自启相关 Skills 是否进入候选，仍读取受保护 profile 中各自的 `enabled|disabled`。所有已移除或已内联的 Skill 路径都只作为所有权清单中的 tombstone；旧下游存在这些路径时必须显式清理并重新生成计划，候选不得恢复它们。`$desktop-curate-harness-memory` 只属于 Harness 源，也必须作为 tombstone 排除。`$desktop-test-gui-initialization-e2e` 及其 `verify-gui-lifecycle-contract.mjs`、`gui-lifecycle-plugin-contract.mjs` 等脚本仍是初始化前 tombstone 资产，终端下游升级不得重新注入。终端下游的 `docs/GUI_SUPPORT_SURFACES.md` 是 `protected` 产品实例，升级不得覆盖。
   `.harness/version-state.json` 始终是 `protected` 下游状态，来源或候选都不得包含、初始化、重算或覆盖它；Cargo 产品版本同样受保护。`$desktop-manage-version` 的 Skill/helper 属于可升级工程资产，但产品周期与缺陷 ID 历史不属于。
5. 要求源 Harness Git 工作树干净，并且其现有 `HEAD` 与声明的源提交匹配；要求声明的源版本与源 `Version.md` 匹配。信任候选之前先运行源 Harness 验证器。不得把该模板专用验证器复制到下游。记录源版本、源提交、候选构建输入、下游分支、提交、脏状态摘要，以及未验证平台。
6. 生成只读计划：

   ```text
   python3 .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.py plan \
     --source-root <clean-harness-source-root> \
     --source-version <harness-version> \
     --source-commit <harness-source-head> \
     --candidate-root <rendered-candidate-root> \
     --target-root <downstream-root> \
     --ownership .agents/skills/desktop-upgrade-harness/references/ownership-manifest.json \
     --lock <downstream-root>/.harness/upstream-lock.json \
     --output <new-plan-path-outside-candidate-and-target.json>
   ```

   输出路径必须不存在，并且必须位于两棵目录树之外。省略 `--output` 时只打印 JSON，不写入文件。
7. 复核每一种分类。只有现有 `managed` 或 `managed-self` 文件的内容与普通权限模式仍然匹配基线时，其 `update` 才可自动应用。`add`、`manual_add` 和 `delete` 需要显式人工文件操作，然后重新生成计划；升级器绝不会创建或删除项目文件。`preserve_local` 必须继续锚定旧目标基线，使后续上游变更成为冲突，而不是覆盖本地决定。`manual_merge` 需要按章节合并。`conflict`、`collision`、所有权模式漂移、候选中存在受保护或墓碑路径、路径越界、特殊权限位、特殊文件、目录联接点或相关符号链接都会阻断完成。
8. 锁文件不存在时，执行初始基线审计。把每个重叠的受管理文件或混合所有权文件视为冲突；不得推断共同祖先。使每个受管理重叠项在字节和权限模式上完全收敛，显式解决混合路径，生成新的已复核计划，然后建立第一份基线：

   ```text
   python3 .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.py record \
     --plan <reviewed-bootstrap-plan.json> \
     --source-version <harness-version> \
     --source-commit <harness-source-commit> \
     --bootstrap \
     --approval bootstrap-verified-baseline
   ```

   初始基线过程绝不豁免符号链接或特殊文件问题、受保护或墓碑路径、存在差异的受管理重叠项，或未经复核的混合路径。
9. 已批准的 Harness 升级直接按复核后的计划串行应用，不自动创建额外持久协调计划、并行分支或完整验收步骤。混合所有权文件始终逐项处理。
10. 用户批准试运行后，只应用安全的受管理操作：

    ```text
    python3 .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.py apply \
      --plan <reviewed-upgrade-plan.json> \
      --approval apply-managed-changes \
      --path <one-reviewed-update-path>
    ```

    该命令根据目标拥有的精确所有权清单和 `.harness/upstream-lock.json` 重新构建计划，绑定源与目标 Git 身份，比较完整的已复核 JSON，预检每项操作，并且只原子替换一个现有文件。每应用一个路径后都要生成并复核新计划。普通 `managed` 更新必须先于 `managed-self` 完成；在自更新路径内部，命令行工具强制执行稳定顺序，并最后替换自身入口。计划篡改或源、候选、目标、控制文件发生任何漂移，都会在写入前中止。使用 `apply_patch` 单独解决 `add`、`manual_add`、`delete`、`merge-sections` 和 `conditional` 项；不得仅为避免合并而替换整个混合所有权文件。
11. 代码行为变化只运行本次升级实际影响的非空单元/回归测试；纯文档或元数据升级只运行其必要替代验证。不得因 Harness 升级自动追加全仓格式、lint、静态、文件行数、中文注释、依赖图、独立构建、冒烟、E2E 或完整验收；用户显式请求构建时才进入对应构建 Skill。
12. 重新运行 `plan`。解决每个阻断项和未应用的受管理操作。只有目标包含已复核结果后，才能记录已验证基线：

    ```text
    python3 .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.py record \
      --plan <newly-reviewed-converged-plan.json> \
      --source-version <harness-version> \
      --source-commit <harness-source-commit> \
      --approval record-verified-baseline
    ```

    每个剩余且已复核的 `manual_merge` 分类都必须重复传入 `--resolved-manual <path>`。精确集合必须与计划匹配，否则记录失败。`manual_add` 必须先创建目标并生成新的已复核收敛计划。绝不得记录仍有 `add`、`manual_add`、`delete`、`update`、阻断项、已变更计划、源 Harness 必需 managed 路径遗漏或未经复核混合路径的基线。
13. 完成任何 `managed-self` 更新后，重新运行升级后升级器的相关测试和只读计划。只更新被独立事件触发的下游记忆：重要阻断或交接更新 Status，符合范围的新增、行为/契约变化、移除或安全事项更新 Changelog，用户要求完整验收或长期审计时更新 Verification。普通缺陷修复、纯重构和内部清理本身不触发这些记忆。不得导入源 Harness 的任何项目记忆或批准历史。

## 安全边界

- 默认执行 `plan`；绝不得仅因调用本 Skill 就写入。
- 绝不得覆盖产品源代码、测试、Cargo 产品版本或锁定选择、`.harness/version-state.json`、项目记忆、项目身份、GUI 身份、GUI 支持界面产品实例、持久 Agent 策略、许可证、Git 配置与历史、远端、分支、标签、普通 Git 工作区状态、敏感信息或未登记本地文件。
- 绝不得把 `Version.md`、`$desktop-instantiate-project`、`$desktop-initialize-rust-project`、模板验证器文件、`docs/HARNESS_ENGINEERING.md` 或其他活动派生入口恢复到终端下游。
- 绝不得自动应用 `merge-sections`、`conditional`、`protected`、未知或冲突路径。
- 升级器绝不自动添加或删除项目文件。已复核的 `add` 或 `delete` 必须逐项人工执行，然后由新计划显示收敛，才能记录基线。
- 实施和验证完成前不得更新 `.harness/upstream-lock.json`。该锁文件记录 Harness 溯源，不是第二个产品版本来源。
- 如果无法确定身份渲染、所有权、共同祖先、必需的外部授权或冲突，必须停止并询问用户。

## 完成要求

报告源与目标身份、基线状态、试运行分类、已批准和已应用路径、保留的本地修改、冲突、本次测试或替代验证、锁文件更新、未验证平台和剩余风险。未显式请求时，构建与完整验收均为 `Not run`；仅有成功的试运行不代表升级完成。
