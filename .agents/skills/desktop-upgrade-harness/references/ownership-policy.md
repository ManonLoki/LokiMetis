# Harness 升级所有权策略

## 目的

本策略说明 `$desktop-upgrade-harness` 如何区分可更新工程资产与下游项目事实。机器匹配规则位于同目录 `ownership-manifest.json`；两者冲突时先停止，不自行选择更宽松的一方。

## 五类主要所有权

- `managed`：由 Harness 维护且候选已经完成下游身份渲染的纯工程文件。只有既有文件仍等于旧基线时才能自动更新；新增和删除仍需人工逐项处理。`docs/design_standards/**` 是身份中立的受管设计目录；产品专属像素和例外写入受保护的 `docs/GUI_APP_PROFILE.md`/ADR，不通过编辑目录制造分叉。
  - `managed-self` 是 `managed` 的机器子模式，不是第六类所有权；它表示升级器自身，必须在其他安全变更后最后应用并由新版复验。
- `merge-sections`：Harness 与下游共同拥有的文件，例如 `AGENTS.md`、README 和规范文档。必须按章节合并，禁止整文件覆盖。
- `conditional`：只在受保护 profile 已启用某项可选桌面能力，或目标平台满足专用工具条件时存在的工程资产。托盘、单实例、深链接、全局快捷键、通知和开机自启相关 Skills 读取 profile 中各自的 `enabled|disabled`；Windows 本地安装 Skill 读取目标平台。GUI 适配器（内联系统语言、窗口状态和 dialog 固定子契约）、支持界面、正式 GUI 构建与发布性能 Skill 是固定工程层，按普通 `managed` 传播，无需额外选择。三个旧独立固定基线 Skill 路径与 Harness 源专用的 `desktop-curate-harness-memory` 为 `tombstone`；dialog 行为仍固定保持主窗口 `dialog:default` 且不授权 fs。支持界面的 Skill、参考、React 模板、i18n 与本地发布说明资产属于同一完整工程资产，产品实例 `docs/GUI_SUPPORT_SURFACES.md` 不属于。
- `protected`：产品源码、项目记忆、策略、身份、许可证、Cargo 当前版本、`.harness/version-state.json` 发布周期/去重状态、验证证据和未知本地文件。升级器只报告，不写入。
- `tombstone`：终端下游永久不应恢复的 Harness 初始化/派生能力和模板专用文件；来源候选必须排除，目标出现时阻断。`$desktop-test-gui-initialization-e2e` 只在 GUI 唯一基线提交前使用，其 `verify-gui-lifecycle-contract.mjs`、`gui-lifecycle-plugin-contract.mjs`、夹具与测试都随该前置 Skill 一同删除；升级不得把它重新注入终端下游。

所有权清单还把已经删除的旧接口适配器、旧非 GUI 构建路线、旧并行协调能力、旧应用升级能力和被替代的规范文件登记为显式 tombstone。旧下游必须人工删除这些路径并重新生成计划；升级候选不得包含或恢复它们。

## 三方比较

`.harness/upstream-lock.json` 为每个受管路径保存旧候选摘要和旧下游摘要。

- 新候选变化、下游未变：既有文件生成可逐文件自动应用的 `update`；`add`/`manual_add`/`delete` 只生成待人工处理动作，处理后必须重新生成计划。
- 新候选未变、下游变化：生成 `preserve_local`，记录新来源时继续保留旧目标基线，使未来上游变化转为冲突。
- 两边都变化且字节不同：生成 `conflict`。
- 两边最终字节一致：生成 `converged`。
- 无旧基线且目标已存在：生成 `bootstrap_conflict` 或 `collision`。

旧下游首次升级不得把当前任一端当作共同祖先。完成逐项初始基线审计、使纯受管理重叠项完全收敛并显式确认混合路径后，才可从一份重新生成且受审的计划建立首份锁文件。初始基线过程不得豁免符号链接、特殊文件、受保护或墓碑路径问题。

## 候选树要求

候选树不是原始 Harness 根目录。它必须：

1. 只包含当前下游适用的工程文件。
2. 排除受保护和墓碑源内容。
3. 使用下游展示名、snake_case/kebab-case 标识、crate 路径、固定 GUI 事实和已批准例外完成渲染。
4. 来自通过自身验证器、Git 工作区干净且 `HEAD`/`Version.md` 与计划声明一致的明确 Harness 版本和提交。
5. 位于目标仓库之外的本次升级专用目录，且不含符号链接或特殊文件。
6. 源 Harness 存在无需身份渲染的必需 `managed` 传播文件时，候选必须包含内容摘要一致的结果；当前包括行数与 core-first 两个检查器及各自专属测试，遗漏或内容过期都会阻断计划与基线记录。

## 永久保护

以下内容不得由升级自动覆盖：产品规格、产品状态、工作计划、ADR、变更记录、验证记录、技术债、业务源码和测试、Cargo 产品版本与锁定选择、`.harness/version-state.json` 的发布周期/缺陷 ID 历史、项目与 GUI 身份、`docs/GUI_SUPPORT_SURFACES.md` 中的支持界面产品实例、受保护 profile、`docs/AGENT_POLICY.md`、双语许可证、Git 历史与配置、普通 Git 工作区状态和未登记本地文件。`$desktop-manage-version` 的 Skill/helper 可以作为工程资产升级，但升级器不得据此初始化、重算或覆盖产品状态。

若新版 Harness 改变法律文本、产品边界或硬规则，升级计划只能报告并请求独立确认；不能把来源仓库的批准事实导入下游。

自动应用一次只替换一个受审路径，之后必须重新生成计划；普通 `managed` 完成后才进入 `managed-self`，升级入口脚本在自更新过程中最后替换。普通权限位参与三方比较；遇到特殊权限位、符号链接、目录联接点和其他特殊节点时，必须以阻断方式失败。
