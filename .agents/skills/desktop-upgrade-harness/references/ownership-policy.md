# Harness 升级所有权策略

## 目的

本策略说明 `$desktop-upgrade-harness` 如何区分可更新工程资产与下游项目事实。机器匹配规则位于同目录 `ownership-manifest.json`；两者冲突时先停止，不自行选择更宽松的一方。

## 五类主要所有权

- `managed`：由 Harness 维护且候选已经完成下游身份渲染的纯工程文件。只有既有文件仍等于旧基线时才能自动更新；新增和删除仍需人工逐项处理。`docs/design_standards/**` 是身份中立的受管设计目录；产品专属像素和例外写入受保护的 `docs/GUI_APP_PROFILE.md`/ADR，不通过编辑目录制造分叉。`$desktop-manage-git-lifecycle` 的完整 Skill、helper 与测试是所有终端下游都适用且无需身份渲染的受管工程资产；升级只传播这些文件，不执行 Git 生命周期命令，也不创建 common-dir 状态。
  - `managed-self` 是 `managed` 的机器子模式，不是第六类所有权；它表示升级器自身，必须在其他安全变更后最后应用并由新版复验。
- `merge-sections`：Harness 与下游共同拥有的文件，例如 `AGENTS.md`、README 和规范文档。必须按章节合并，禁止整文件覆盖。
- `conditional`：只在已选接口或已启用能力中存在的工程资产。先确认下游选择，再人工或由对应适配器 Skill 合并。`$mantine-list-view`、`$desktop-add-gui-system-locale`、`$desktop-add-gui-updater`、`$desktop-add-gui-window-state`、`$desktop-add-gui-dialog`、`$desktop-add-gui-system-tray`、`$desktop-add-gui-single-instance`、`$desktop-add-gui-deep-link`、`$desktop-add-gui-global-shortcut`、`$desktop-add-gui-system-notifications`、`$desktop-add-gui-autostart` 与 `$desktop-prepare-gui-support-surfaces` 只随已选 GUI 且 `gui-framework = "tauri"` 的下游传播；其中 `$mantine-list-view` 的 SKILL、metadata、references 与完整 `assets/` 资产集是一个条件知识资产，传播本身不生成页面或安装 dnd-kit。系统语言、updater、窗口状态与 dialog 属于固定基线，dialog 固定保持主窗口 `dialog:default` 且不授权 fs，托盘、单实例、深链接、全局快捷键、通知和开机自启是否实际接线继续由 profile 中各自的 `enabled|disabled` 决定。支持界面的 Skill、参考、React 模板、品牌 profile/i18n/manifest 和全部原始媒体属于同一完整工程资产，产品实例 `docs/GUI_SUPPORT_SURFACES.md` 不属于。`$desktop-add-gui-adapter`、Tauri 打包 Skills 同样仅适用于 Tauri；`$desktop-add-gpui-adapter` 与 `$desktop-build-gpui-release` 只适用于 `gui-framework = "gpui"` 的 GUI 下游，分别传播原生 Rust 模板和独立本机 macOS `.app`/DMG、Windows NSIS 打包管线，不注入 Tauri/WebView、Mantine、React 或 Tauri 插件；两种 GUI 都使用 Node.js/pnpm 工程工具。`$desktop-prepare-gui-app-identity` 可随两种框架传播。根 Cargo 中缺省框架的旧 GUI 下游沿用 `tauri`；未知值、类型错误、重复字段或非 GUI 声明框架都失败关闭。升级计划绑定受保护 Cargo 清单的完整快照，并机械阻断不适用框架的候选；绝不从候选内容推断或改写目标选择。
- `protected`：产品源码、项目记忆、策略、身份、许可证、Cargo 当前版本、`.harness/version-state.json` 发布周期/去重状态、`.harness/release-context.json` 当前发布事实、验证证据和未知本地文件。升级器只报告，不写入；目标尚无发布上下文时保持缺席，未来只能由目标项目真实执行 `$desktop-prepare-release` 创建。旧下游缺少版本状态时也不产生所有权例外：工程层升级与基线记录完成后，必须先说明旧 pending、bug ID 与发布历史无法恢复，再由用户明确批准目标项目单独执行 `$desktop-manage-version init --migration-approved`；升级器本身不得调用或代写。
- `docs/AGENT_POLICY.md` 的 `post_release_action` 也属于 protected 下游选择。旧 schema 3 在工程层升级及来源锁记录后必须询问用户，在目标项目中调用 `$desktop-switch-post-release-action` 先受保护合并旧策略正文的新发布后规则，再原子迁移 frontmatter 至 schema 4 并写入当日 ADR；旧正文未消除相反规则时不得通过 `check`；`check` 返回合法已配置值前，不能把完整升级标记为完成。升级器的候选树、`plan|apply|record` 均不得设置默认值或代写该字段。切换 Skill 的 `SKILL.md`、metadata、helper 与回归测试是所有终端下游都要传播的 managed 工程文件。
- `tombstone`：终端下游永久不应恢复的 Harness 初始化/派生能力和模板专用文件；来源候选必须排除，目标出现时阻断。`$desktop-test-gui-initialization-e2e` 只在 GUI 唯一基线提交前使用，其 `verify-gui-lifecycle-contract.mjs`、`gui-lifecycle-plugin-contract.mjs`、夹具与测试都随该前置 Skill 一同删除；升级不得把它重新注入终端下游。
- GPUI 目录内的 `scripts/create_gpui_workspace.mjs`、`scripts/create_gpui_workspace.test.mjs` 与 `assets/core/**` 同为初始化专用 `tombstone`，必须先于该目录的 `conditional` 规则。终端下游保留受限 `scripts/merge_gpui_node_tooling.mjs`、纯渲染库 `scripts/gpui_node_tooling.mjs`、`scripts/gpui_validate.mjs` 及其测试、纯渲染库 `scripts/gpui_adapter_files.mjs`、既有 core 的 GUI 增量入口 `scripts/add_gpui_adapter.mjs` 及其测试和 GUI/品牌资产；升级不恢复首次创建工作区或派生 core 的入口。

根 `package.json` 保持 protected，不进入候选或来源锁。GPUI 的 `scripts/merge_gpui_node_tooling.mjs`、`scripts/gpui_node_tooling.mjs` 与 `scripts/gpui_validate.mjs` 是条件工程资产；在已授权的工程入口迁移范围内，前者可在候选收敛后独立定点合并 scripts/engines，拒绝冲突并保留其他字段。通用升级器不调用该工具，不因此获得写入产品清单的权限。无 npm 依赖的脚本入口不要求 `pnpm install` 或 pnpm 锁文件。

项目本地 `.agents/skills/design-taste-frontend/**` 是原样安装且可由下游定制的第三方知识资产，始终 `protected`；候选树和来源锁排除该目录，升级不覆盖、删除或重新安装它。旧目标清单缺少这一保护规则时，先按显式清单合并补齐保护，再重新生成计划，不能把通用 managed 规则当成安装或删除授权。该 protected 条目必须排在所有可能匹配设计目录或内部路径的不同 mode 规则之前，包括自定义通配规则；无法证明不重叠的前置规则也会阻断，移到保护条目之后再重新规划。

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
