# Harness 升级所有权策略

## 目的

本策略说明 `$desktop-upgrade-harness` 如何区分可更新工程资产与下游项目事实。机器匹配规则位于同目录 `ownership-manifest.json`；两者冲突时先停止，不自行选择更宽松的一方。

候选树中的 `AGENTS.md` 与 `docs/ENGINEERING_RULES.md` 必须剥离仅限 Harness 上游的 Python 禁令及检查器要求；不得将上游脚本语言限制传播到终端下游。下游继续按自身需求选择脚本语言，并遵守显式本地依赖规则。

## 五类主要所有权

- `managed`：由 Harness 维护且候选已经完成下游身份渲染的纯工程文件。只有既有文件仍等于旧基线时才能自动更新；新增和删除仍需人工逐项处理。`docs/design_standards/**` 是身份中立的受管设计目录；产品专属像素和例外写入受保护的 `docs/GUI_APP_PROFILE.md`/ADR，并在标准匹配时优先，升级不得用新旧 Harness 缺省覆盖，不通过编辑目录制造分叉。`$desktop-manage-git-lifecycle` 的完整 Skill、helper 与测试是所有终端下游都适用且无需身份渲染的受管工程资产；升级只传播这些文件，不执行 Git 生命周期命令，也不创建 common-dir 状态。
  - `managed-self` 是 `managed` 的机器子模式，不是第六类所有权；它表示升级器自身，必须在其他安全变更后最后应用并由新版复验。
- `merge-sections`：Harness 与下游共同拥有的文件，例如 `AGENTS.md`、README 和规范文档。必须按章节合并，禁止整文件覆盖。
- `conditional`：只在已选接口或能力中适用的工程资产，先确认下游事实再构造候选；传播本身不启用能力。根 Cargo 中缺省框架的旧 GUI 下游沿用 `tauri`，未知值、错误类型、重复字段或非 GUI 声明框架都失败关闭。升级计划绑定受保护 Cargo 清单的完整快照，不从候选反推或改写选择。Tauri GUI 只读 [Tauri 工程候选](tauri-ownership.md)，GPUI GUI 只读 [GPUI 工程候选](gpui-ownership.md)；非 GUI 不加载这两份引用。
非 GUI 下游不得因此新增 pnpm 或 GUI-only Skill。框架选择不授权迁移或替换产品代码、profile、偏好或已选桌面能力。

- `protected`：产品源码、项目记忆、策略、身份、许可证、Cargo 当前版本、`.harness/version-state.json` 发布周期/去重状态、`.harness/release-context.json` 当前发布事实、验证证据和未知本地文件。升级器只报告，不写入；目标尚无发布上下文时保持缺席，未来只能由目标项目真实执行 `$desktop-prepare-release` 创建。旧下游缺少版本状态时也不产生所有权例外：工程层升级与基线记录完成后，必须先说明旧 pending、bug ID 与发布历史无法恢复，再由用户明确批准目标项目单独执行 `$desktop-manage-version init --migration-approved`；升级器本身不得调用或代写。
- `docs/AGENT_POLICY.md` 的 `post_release_action` 也属于 protected 下游选择。旧 schema 3 在工程层升级及来源锁记录后必须询问用户，在目标项目中调用 `$desktop-switch-post-release-action` 先受保护合并旧策略正文的新发布后规则，再原子迁移 frontmatter 至 schema 4 并写入当日 ADR；旧正文未消除相反规则时不得通过 `check`；`check` 返回合法已配置值前，不能把完整升级标记为完成。升级器的候选树、`plan|apply|record` 均不得设置默认值或代写该字段。切换 Skill 的 `SKILL.md`、metadata、helper 与回归测试是所有终端下游都要传播的 managed 工程文件。
- `tombstone`：终端下游永久不应恢复的 Harness 初始化/派生能力和模板专用文件；来源候选必须排除，目标出现时阻断。`$desktop-test-gui-initialization-e2e` 只在 GUI 唯一基线提交前使用，其 `verify-gui-lifecycle-contract.mjs`、`gui-lifecycle-plugin-contract.mjs`、夹具与测试都随该前置 Skill 一同删除；升级不得把它重新注入终端下游。

根 `package.json` 是 protected 产品文件，不进入候选或来源锁；GPUI 已授权的工程脚本迁移只按对应框架引用处理，通用升级器不调用专用迁移工具。

项目本地 `.agents/skills/design-taste-frontend/**` 是原样安装且可由下游定制的第三方知识资产，始终 `protected`；候选树和来源锁排除该目录，升级不覆盖、删除或重新安装它。旧目标清单缺少这一保护规则时，先按显式清单合并补齐保护，再重新生成计划，不能把通用 managed 规则当成安装或删除授权。该 protected 条目必须排在所有可能匹配设计目录或内部路径的不同 mode 规则之前，包括自定义通配规则；无法证明不重叠的前置规则也会阻断，移到保护条目之后再重新规划。

## 升级前事实检查

`docs/AGENT_POLICY.md` 继续是受保护的下游事实。已有 schema 4 正文若未合并默认主分支、release 和 tag 同步规则，须在目标项目保留 frontmatter 与自定义规则迁移正文，再 inspect/check，无需重选；升级器不代写 protected 正文。第 4 步只读记录发布后动作状态：旧 `schema_version: 3` 且缺少 `post_release_action` 表示需要补选；`schema_version: 4` 或 `schema_version: 5` 只接受 `local_package` 或 `push_release_branch`。重复/缺失字段、未知 schema 或非法值是冲突，不能推断为默认项，也不能宣布升级完成。

同时只读解析目标根 `Cargo.toml` 的 `[workspace.metadata.agent-first-harness]` 下 `dependency-lock-policy` 字段：缺省或 `"ignored"` 沿用忽略锁文件的工程分支，显式 `"tracked"` 走锁文件受跟踪与冻结解析分支；未知值、错误类型或重复键失败关闭。`tracked` 须核对根及 `rust-test-manifests` 指出的每个独立工作区 `Cargo.lock`，以及已选 Tauri GUI 根的 `pnpm-lock.yaml`，均为真实文件、受 Git 跟踪且未被忽略。候选中的通用规则和 Skill 应保留两条分支，并按目标的实际接口与工作区路径渲染；不得以源默认策略覆盖目标的显式选择。根 Cargo 元数据、`.gitignore` 和项目锁文件均为目标受保护事实，升级器 `plan|apply|record` 不修改它们；目标需要首次选择或修复 `tracked` 时，由下游在独立开发范围内显式调整并验证后再继续记录基线。

`.harness/release-context.json` 始终是 `protected` 下游发布事实，来源和候选均不得包含、初始化、重算或覆盖它；目标尚无该文件时保持缺席，只有目标项目真实执行 `$desktop-prepare-release` 时才可创建。Git common-dir 生命周期清单位于 tracked 目标树之外，从不进入候选、所有权清单或来源锁。

## 三方比较

`.harness/upstream-lock.json` 为每个受管路径保存旧候选摘要和旧下游摘要。

- 新候选变化、下游未变：既有文件生成可逐文件自动应用的 `update`；`add`/`manual_add`/`delete` 只生成待人工处理动作，处理后必须重新生成计划。
- 新候选未变、下游变化：生成 `preserve_local`，记录新来源时继续保留旧目标基线，使未来上游变化转为冲突。
- 两边都变化且字节不同：生成 `conflict`。
- 两边最终字节一致：生成 `converged`。
- 无旧基线且目标已存在：字节及权限一致生成 `converged`，不同则生成 `bootstrap_conflict` 或 `collision`。

旧下游首次升级不得把当前任一端当作共同祖先。完成逐项初始基线审计、使纯受管理重叠项完全收敛并显式确认混合路径后，才可从一份重新生成且受审的计划建立首份锁文件。初始基线过程不得豁免符号链接、特殊文件、受保护或墓碑路径问题。

完全未接入 Harness 的既有项目先按 [首次接入步骤](adopt-existing-project.md)安装源提交中的所有权清单；这一步只建立 `plan` 的读取前提，不创建来源锁，也不授权覆盖目标内容。目标已有清单时先核对，不能重置为源缺省。旧版来源锁中若仍跟踪上游专用 Python 检查器，所有权 mode 保留 `managed` 以兼容基线迁移，但候选不得再包含它；目标人工移除后重新计划，`record` 才会结束跟踪。

目标旧清单若只缺新增的 `.agents/skills/` 专项 `managed` 规则，而通用规则已对该完整路径赋予相同有效 mode，`plan` 可继续，并通过 `ownership_repairs` 列出缺项。候选清单必须包含显式新规则；目标清单按已有来源锁进行 `managed-self` 三方更新或人工解决本地差异，重新生成计划后才能 `record`。记录基线时仍严格要求目标清单具有所有最低规则。缺失 `protected`、`tombstone`、`conditional`、`managed-self` 规则，存在重叠的不同 mode，或旧锁中的有效 mode 漂移时继续阻断；此兼容读取不授权覆盖产品事实或省略清单修复。

## 候选树要求

候选树不是原始 Harness 根目录。它必须：

1. 只包含当前下游适用的工程文件。
2. 排除受保护和墓碑源内容。
3. 使用下游展示名、snake_case/kebab-case 标识、crate 路径、接口选择和已批准例外完成渲染。
4. 来自通过自身验证器、Git 工作区干净且 `HEAD`/`Version.md` 与计划声明一致的明确 Harness 版本和提交。
5. 位于目标仓库之外的任务专用目录，且不含符号链接或特殊文件。
6. 源 Harness 存在无需身份渲染的必需 `managed` 传播文件时，候选必须包含内容摘要一致的结果；当前包括行数、Rust 中文注释与 core-first 检查器及各自专属测试；上游专用 `check_no_python.mjs` 及其测试必须从候选排除，旧目标中对应文件须经审查后移除，以及 `$desktop-manage-git-lifecycle` 的完整 Skill、helper 与测试，遗漏或内容过期都会阻断计划与基线记录。

## 永久保护

以下内容不得由升级自动覆盖：产品规格、产品状态、工作计划、ADR、变更记录、验证记录、技术债、业务源码和测试、Cargo 产品版本与锁定选择、`.harness/version-state.json` 的发布周期/缺陷 ID 历史、`.harness/release-context.json` 的当前发布事实、项目与 GUI 身份、`docs/GUI_APP_PROFILE.md`、`docs/GUI_SUPPORT_SURFACES.md` 中的支持界面产品实例、接口选择、`docs/AGENT_POLICY.md`、双语许可证、Git 历史与配置、Git common-dir 生命周期清单以及未登记本地文件。`$desktop-manage-version`、`$desktop-manage-git-lifecycle` 与 `$desktop-switch-post-release-action` 的 Skill/helper 可以作为工程资产升级，但升级器不得据此初始化、重算或覆盖产品状态、发布上下文与受保护的发布后选择，也不得操作 remote、分支、标签或 Worktree。缺失状态的显式迁移只能在工程升级完成后由目标项目版本 Skill 执行，并保持用户批准、当前合法 Cargo 版本空周期基线和不可恢复历史报告三项事实可见。

若新版 Harness 改变法律文本、产品边界或硬规则，升级计划只能报告并请求独立确认；不能把来源仓库的批准事实导入下游。

自动应用一次只替换一个受审路径，之后必须重新生成计划；普通 `managed` 完成后才进入 `managed-self`，升级入口脚本在自更新过程中最后替换。普通权限位参与三方比较；遇到特殊权限位、符号链接、目录联接点和其他特殊节点时，必须以阻断方式失败。

## 工程资产传播

所有终端下游都必须同步无需身份渲染的 `$desktop-manage-git-lifecycle` 完整目录：`SKILL.md`、`agents/openai.yaml`、`scripts/git_lifecycle.mjs`、`scripts/git_lifecycle_core.mjs`、`scripts/git_lifecycle_publication.mjs`、`scripts/git_publication_report.mjs`、`scripts/git_lifecycle_test_support.mjs`、`scripts/git_publication_test_cases.test.mjs`、`scripts/git_lifecycle.test.mjs` 和 `scripts/git_lifecycle_release.test.mjs` 均须与源内容摘要一致。升级只传播这些 `managed` 工程文件，不调用 `inspect|start|track-worktree|publish|release`，不创建 common-dir 生命周期清单，也不改动任何 Git remote、分支、标签、历史或 Worktree。目标缺少这些工程文件时按 `add` 逐项人工加入。

所有终端下游还必须同步 `$desktop-switch-post-release-action` 的 `SKILL.md`、`agents/openai.yaml`、`scripts/post_release_action.mjs` 和 `scripts/post_release_action.test.mjs`，并保持源内容摘要一致；升级器在候选遗漏或旧版内容时阻断。只传播该 managed Skill，不调用其写入命令。

四项跨接口知识入口 `$desktop-add-file-operations`、`$desktop-add-external-process`、`$desktop-manage-user-tasks`、`$desktop-configure-agent-policy` 及其 references、helper/测试必须完整传播且与源摘要一致；策略实例仍 protected，升级不得启用能力或改写用户选择。

跨接口完整传播 `$desktop-manage-dependencies`、`$desktop-review-core-boundaries`、`$desktop-record-adr`、`$desktop-handoff-project`、`$desktop-inspect-release-notes`，只传播工程知识/只读入口，不代写记忆或清单。Tauri GUI 条件完整传播 `$desktop-enable-gui-updates` 与 `$desktop-add-gui-telemetry`；GPUI GUI 条件完整传播 `$desktop-add-gpui-autostart`，均不自动启用。`$desktop-test-gpui-initialization-e2e` 为初始化专用 tombstone，完成基线前删除，升级永久不恢复；产品 profile、支持实例和根清单继续 protected。

中文提交的完整 `$desktop-configure-git-commits` Skill、metadata、规范、模板、helper/测试，以及 `$desktop-prepare-release` 的 `release_git.mjs`/测试必须作为同一 managed 依赖闭包传播。按章节合并启动文档的项目 Task 默认关闭、非 pinned、显式阶段隔离和自主恢复入口；目标 `docs/AGENT_POLICY.md` 的既有 frontmatter（原 schema 的全部偏好与确认元数据）逐字节保留，并保留自定义正文。受保护正文迁移在目标授权范围单独处理；不得改变已有项目选择或把旧全局 Task Tree 规则当作项目启用事实。用户级 `AGENTS.md` 位于项目候选和来源锁范围之外，不复制或覆盖；项目 AGENTS/README 继续按 merge-sections 处理。

完整 managed 传播 `$desktop-manage-task-worktrees`（SKILL 与 metadata），即使 task_worktrees 关闭也保留。schema 3/4 发布动作保持合法读取，schema 4 新环境选择为 selection_required；工程升级不得补默认值、改确认元数据、用户级 AGENTS 或迁移既有 Task/Worktree。受保护正文按章节合并后，由用户明确补选并通过 `$desktop-configure-agent-policy migrate-task-worktrees` 从 schema 4 迁移至 5；schema 3 先由动作专用流程迁移至 4。

统一 Task/工作树合同完整 managed 传播 `$desktop-task-workflow` 的 SKILL、metadata 与 references；旧三个入口仅兼容路由，脚本路径保持兼容。独立工作树尽量并行创建，只有同 Local cwd 与共享状态写入短串行。
