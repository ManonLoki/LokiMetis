---
name: desktop-upgrade-harness
description: 在已初始化或首次接入的既有下游项目中安全更新由 Harness 维护的工程规则、保留的项目 Skills 和维护工具，不覆盖产品代码、项目记忆、身份、策略、许可证或本地决定。用于用户要求把下游项目的 Harness 工程层与明确提供的较新 Harness 源进行升级、同步、迁移、刷新或比较时。
---

# 升级 Harness

通过可审计的预览、冲突解决、应用、验证和基线记录闭环，更新一个终端下游项目。

目标尚无 Harness 所有权清单或来源锁时，先完整读取 [既有项目首次接入步骤](references/adopt-existing-project.md)，安装精确所有权清单并构造适用候选，再进入下面的 `plan`。不得要求目标先已有 Harness 初始化结果，也不得把源项目记忆或 Python 禁令复制过去。

升级入口先执行用户级 `AGENTS.md` 的长期规则；项目 `user_owned_tasks: enabled` 或用户明确单次要求左侧 Task 时，先完成 `$desktop-task-workflow` 的业务前绑定。启动文档合并、工程资产传播与受保护事实按 [所有权策略](references/ownership-policy.md) 执行。

## 工作流程

1. 读取下游存在的 `AGENTS.md`、`docs/AGENT_POLICY.md`、日期最新的产品规格、产品状态、工作计划和 ADR、`docs/ENGINEERING_RULES.md`、`docs/TECH_DEBT.md`，以及存在时的 `.harness/upstream-lock.json`。首次接入尚缺这些文件时按接入步骤建立真实项目事实，不从 Harness 源复制批准结论。读取源 Harness 的 `Version.md`、日期最新的产品规格、产品状态和 ADR，以及其验证命令。
2. 无法无歧义发现源 Harness 根目录时，要求用户明确指定。将源根目录和下游根目录解析为互不相同的规范绝对路径。要求每个现有 Git 仓库的顶层目录等于其声明根目录；拒绝符号链接根目录、嵌套歧义、源与目标相同，以及声明根目录以外的路径。
3. 读取 [所有权策略](references/ownership-policy.md)，并使用 [所有权清单](references/ownership-manifest.json)。将该清单视为最低保护策略，而不是复制每个匹配源文件的许可。旧目标清单仅缺新增的 Skill 专项 `managed` 规则，且旧规则已对整个对应路径赋予相同有效所有权时，升级器允许生成计划，并在 `ownership_repairs` 中列出待补规则；候选清单仍须包含完整显式规则。保护类规则缺失、显式冲突或有效 mode 改变仍阻断。
4. 检查下游已选接口、身份映射、保留的 Skills、已批准例外和持久 Agent 策略。按 [所有权策略](references/ownership-policy.md) 的候选树、工程资产传播、框架条件、受保护事实和依赖锁策略构造适用候选；排除每个 `protected` 或 `tombstone` 源路径，原始 Harness 源目录树不是有效候选。首次检查同时只读记录版本状态是否缺失、Agent 策略 schema 与发布后动作状态，分别留待第 14、15 步处理；不得在候选构造中写入受保护事实。程序类型文案另按 [名称升级映射](references/program-type-labels.md) 核对，只映射名称，保留 Cargo `interfaces` 与实际选择，不恢复初始化入口或表单。
5. 要求源 Harness Git 工作树干净，并且其现有 `HEAD` 与声明的源提交匹配；要求声明的源版本与源 `Version.md` 匹配。信任候选之前先运行源 Harness 验证器。不得把该模板专用验证器复制到下游。记录源版本、源提交、候选构建输入、下游分支、提交、脏状态摘要，以及未验证平台。

   本 Skill 的 helper 要求当前宿主具备 Node.js >= 24.21；缺失或版本不足时失败关闭，不得改用其他运行时。所有操作命令均使用 `node` 且保持单行，不依赖 POSIX `\` 续行符、PowerShell 反引号或 shell 变量。
6. 生成只读计划：

   macOS、Linux 与 Windows PowerShell 均在替换路径占位符后直接执行：

   ```text
   node .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.mjs plan --source-root "<clean-harness-source-root>" --source-version "<harness-version>" --source-commit "<harness-source-head>" --candidate-root "<rendered-candidate-root>" --target-root "<downstream-root>" --ownership .agents/skills/desktop-upgrade-harness/references/ownership-manifest.json --lock "<downstream-root>/.harness/upstream-lock.json" --output "<new-plan-path-outside-candidate-and-target.json>"
   ```

   输出路径必须不存在，并且必须位于两棵目录树之外。省略 `--output` 时只打印 JSON，不写入文件。
7. 复核每一种分类。只有现有 `managed` 或 `managed-self` 文件的内容与普通权限模式仍然匹配基线时，其 `update` 才可自动应用。`ownership_repairs` 非空时按计划中的 `managed-self` 清单动作修复，普通 `managed` 更新仍须先完成；清单有本地改动时按三方分类审查，不覆盖。`add`、`manual_add` 和 `delete` 需要显式人工文件操作，然后重新生成计划；更新器绝不会创建或删除项目文件。`preserve_local` 必须继续锚定旧目标基线，使后续上游变更成为冲突，而不是覆盖本地决定。`manual_merge` 需要按章节合并。`conflict`、`collision`、所有权模式漂移、候选中存在受保护或墓碑路径、路径越界、特殊权限位、特殊文件、目录联接点或相关符号链接都会阻断完成。
8. 锁文件不存在时，执行初始基线审计。字节与权限模式一致的受管理重叠项显示 `converged`；存在差异的受管理重叠项与混合所有权文件须人工复核，不得推断共同祖先。使存在差异的受管理项完全收敛，显式解决混合路径，生成新的已复核计划，然后建立第一份基线：

   ```text
   node .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.mjs record --plan "<reviewed-bootstrap-plan.json>" --source-version "<harness-version>" --source-commit "<harness-source-commit>" --bootstrap --approval bootstrap-verified-baseline
   ```

   初始基线过程绝不豁免符号链接或特殊文件问题、受保护或墓碑路径、存在差异的受管理重叠项，或未经复核的混合路径。
9. 已批准的 Harness 升级直接按复核后的计划应用，不自动创建 Work Plan、Todo、并行 Worktree/Subagent 或完整验收步骤。混合所有权文件必须串行处理；只有用户明确要求并行且适用时才调用 `$desktop-run-parallel-worktrees`。
10. 用户批准试运行后，只应用安全的受管理操作：

    ```text
    node .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.mjs apply --plan "<task-plan.json>" --approval apply-managed-changes --path "<one-reviewed-update-path>"
    ```

    该命令根据目标拥有的精确所有权清单和 `.harness/upstream-lock.json` 重新构建计划，绑定源与目标 Git 身份，比较完整的已复核 JSON，预检每项操作，并且只原子替换一个现有文件。每应用一个路径后都要生成并复核新计划。普通 `managed` 更新必须先于 `managed-self` 完成；在自更新路径内部，命令行工具强制执行稳定顺序，并最后替换自身入口。计划篡改或源、候选、目标、控制文件发生任何漂移，都会在写入前中止。使用 `apply_patch` 单独解决 `add`、`manual_add`、`delete`、`merge-sections` 和 `conditional` 项；不得仅为避免合并而替换整个混合所有权文件。
11. 代码行为变化只运行本次升级实际影响的非空单元/回归测试；纯文档或元数据升级只运行其必要替代验证。不得因 Harness 升级自动追加全仓格式、lint、静态、文件行数、中文注释、依赖图、独立构建、冒烟、E2E 或完整验收；普通构建保持开发流程。用户明确要求 Git 发布时才进入 `$desktop-prepare-release`，在本地主分支合并并打 tag 后结束；发布后按已确认的 `post_release_action` 进入所选动作；升级操作本身不触发发布或该动作。升级不得在目标根运行 `check_no_python.mjs`，也不得将 Python 源码、依赖清单或运行步骤当作 Harness 升级阻断项；若旧下游受上游禁令约束，先在受审计划中移除该检查器及其测试，并逐节修正 `AGENTS.md`、`docs/ENGINEERING_RULES.md` 中旧禁令，保留目标已有脚本和产品事实。
12. 重新运行 `plan`。解决每个阻断项和未应用的受管理操作，确认 `ownership_repairs` 为空；`record` 会严格复核目标清单的每条最低规则，不接受尚待修复的兼容缺项。只有目标包含已复核结果后，才能记录已验证基线：

    ```text
    node .agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.mjs record --plan "<newly-reviewed-converged-plan.json>" --source-version "<harness-version>" --source-commit "<harness-source-commit>" --approval record-verified-baseline
    ```

    每个剩余且已复核的 `manual_merge` 分类都必须重复传入 `--resolved-manual <path>`。精确集合必须与计划匹配，否则记录失败。`manual_add` 必须先创建目标并生成新的已复核收敛计划。绝不得记录仍有 `add`、`manual_add`、`delete`、`update`、阻断项、已变更计划、源 Harness 必需 managed 路径遗漏或未经复核混合路径的基线。
13. 完成任何 `managed-self` 更新后，重新运行升级后更新器的相关测试和只读计划。只更新被独立事件触发的下游记忆：重要阻断或交接交 `$desktop-handoff-project` 更新 Status，符合范围的新增、行为/契约变化、移除或安全事项更新 Changelog，真实渠道分发后的记录或独立回顾性人工复核/长期审计才更新 Verification；候选完整验收只写忽略的 release/ 证据。普通缺陷修复、纯重构和内部清理本身不触发这些记忆。不得导入源 Harness 的任何项目记忆或批准历史。
14. 若第 4 步确认目标是已存在的独立 Git 项目但缺少 `.harness/version-state.json`，必须先完成并记录上述工程层升级，随后停止自动流程，向用户明确说明：旧 `pending_changes`、`applied_bug_ids` 与 `last_release` 历史无法恢复；迁移只会以当前合法 Cargo 版本建立空周期基线；升级器本身没有写入受保护状态。只有用户对这次迁移明确批准后，才由目标项目中的 `$desktop-manage-version` 执行 `node .agents/skills/desktop-manage-version/scripts/version_gate.mjs init --project-root "<downstream-root>" --migration-approved`，并复核输出的 `migration: true`、`history_status: unrecoverable-pre-migration-history`、空 `pending_changes`/`applied_bug_ids` 与 Cargo 版本一致。未获批准时不得代替用户推断或执行，必须把工程层升级与仍待批准的版本状态迁移分别报告，且不得声称整个升级闭环完成。
15. 工程层收敛并记录基线、且第 14 步适用的版本状态迁移完成后，在目标根调用 `node .agents/skills/desktop-switch-post-release-action/scripts/post_release_action.mjs inspect --project-root "<downstream-root>"`。旧 schema 3 的 `selection_required` 必须询问用户选择本地打包或提交远程，由目标项目中的 `$desktop-switch-post-release-action` 先在保护既有项目规则的前提下合并旧策略正文与新发布后规则，再按本次明确选择原子迁移 frontmatter 至 schema 4，并记录当日 ADR；旧正文中“发布后另行请求”或“无 release 分支”的相反规则未清除时 `set/check` 必须阻断；升级器 `plan|apply|record` 绝不得代写或传入选择。已有合法 schema 4/5 保留目标选择，不再次询问。随后运行 `node .agents/skills/desktop-switch-post-release-action/scripts/post_release_action.mjs check --project-root "<downstream-root>"`，复核 `status: configured` 与合法持久值。缺选、非法状态、写入或 ADR 复核失败时，只能报告工程基线已记录，不能声称整个升级闭环完成。

## 安全边界

- 默认执行 `plan`；绝不得仅因调用本 Skill 就写入。
- 绝不得覆盖产品源代码、测试、Cargo 产品版本或依赖锁策略、`.gitignore`、项目 `Cargo.lock`/`pnpm-lock.yaml`、`.harness/version-state.json`、`.harness/release-context.json`、项目记忆、项目身份、已选接口、GUI 身份、GUI 支持界面产品实例、持久 Agent 策略、许可证、Git 配置与历史、远端、分支、标签、Worktree、common-dir 生命周期清单、敏感信息或未登记本地文件；传播 Git 生命周期或发布后动作 Skill 不授权执行它。第 15 步的用户确认只授权目标项目专用 Skill 写入该受保护偏好与真实 ADR，不授权升级器改写任何 protected 文件。第 14 步只是在工程层升级完成后转交版本 Skill 的显式人工迁移，不能由升级器调用、合并到 `apply|record` 或解释为对 protected 状态的例外。
- 绝不得把 `Version.md`、`$desktop-instantiate-project`、`$desktop-initialize-rust-project`、模板验证器文件、`docs/HARNESS_ENGINEERING.md` 或其他活动派生入口恢复到终端下游。
- 绝不得自动应用 `merge-sections`、`conditional`、`protected`、未知或冲突路径。
- 更新器绝不自动添加或删除项目文件。已复核的 `add` 或 `delete` 必须在声明的 Todo 内人工执行，然后由新计划显示收敛，才能记录基线。
- 实施和验证完成前不得更新 `.harness/upstream-lock.json`。该锁文件记录 Harness 溯源，不是第二个产品版本来源。
- 如果无法确定身份渲染、所有权、共同祖先、必需的外部授权或冲突，必须停止并询问用户。

## 完成要求

报告源与目标身份、基线状态、试运行分类、已批准和已应用路径、保留的本地修改、冲突、本次测试或替代验证、锁文件更新、未验证平台和剩余风险。未显式请求时，构建与完整验收均为 `Not run`；仅有成功的试运行不代表升级完成。必须报告发布后动作的原值、最终值与 schema 4/5 强制检查结果；旧项目缺选时不得把工程层完成误报为完整升级。
