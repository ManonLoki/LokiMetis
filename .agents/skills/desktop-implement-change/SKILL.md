---
name: desktop-implement-change
description: 直接实施范围清楚的请求，只运行本次开发需要的单元/回归测试并更新被真实事件触发的记录；不得自动追加计划、全仓检查、构建或验收。
---

# 实施变更

完成当前授权范围。新功能或独立 Bug 以“自动创建或复用本周期 feature 分支 + 实现 + 本次必要单元测试 + 事件触发记录 + 可审查的本地提交”为日常开发闭环。用户明确说“发布”时才整理本周期代码、普通合并到本地默认主分支并创建及复读版本 tag；Git 发布到此结束。完成初始化的下游随后按 `post_release_action` 已确认的选择执行并检测发布后路径；Harness 源的后续动作由用户当次决定。

修改 GUI 时先读取根 Cargo metadata 的 `gui-framework`，既有 GUI 缺失时兼容 `tauri`。下文 React/Mantine/Jotai/Query、浏览器存储、`$mantine-list-view`、TypeScript与Tauri能力只约束Tauri路径。`gpui` 按 `$desktop-add-gpui-adapter` 与 `docs/design_standards/gpui_gui.md` 使用原生Rust视图、gpui-kit与rust-i18n；页面会话由同一原生根视图在进程内持有，页面切换保留、退出重置，不复制core权威数据。控件自身动作与周围父级不串扰、core-first、中文声明注释和本次相关非空Rust回归同样适用；GPUI 同样使用 Node.js/pnpm 运行项目根验证、发布和打包脚本；不得因为通用Skill提到前端而为GPUI安装React或Mantine。五项原生能力只按 profile 与已批准配置启用，依赖标准和选择后问询由 GPUI adapter Skill 管理；深链接仍 unavailable/disabled。

## 工作流程

用户可见 Task 的序号按同一 `hostId` 和精确 `projectId` 清点：先读取 `list_threads`，再逐页读取 `list_archived_threads` 清点归档 Task，从最大有效序号继续递增；空历史才从 1 开始，缺号不回填。内部 Subagent 不使用标题合同且不占用序号。Git Task 还必须用 `git rev-parse --path-format=absolute --git-common-dir` 和 `git worktree list --porcelain` 证明独立 Worktree 绑定正确。

1. 检查 `AGENTS.md`、`docs/AGENT_POLICY.md`、`docs/ENGINEERING_RULES.md`，有 Git 时再检查 Git 根/分支和工作区；只读取当前变更直接需要的 Product Spec、ADR、接口或技术事实。若规范根同时包含 Harness 专用 `Version.md` 和活动 `$desktop-instantiate-project`，只允许修改 Harness 自身规则、Skills、中性资产、脚本、验证器、文档或其交付工程；任何产品目的、业务功能、产品专属 UI/文案/数据、远程地址、凭据、构建或发布需求都必须在写入前拒绝，并要求用户切换到终端下游后重新提出。保护用户已有修改，重叠无法安全处理时停止。对于已初始化下游的新功能或独立 Bug，在任何项目文件写入前自动调用 `$desktop-manage-git-lifecycle start --project-root <project-root> --summary <ascii-kebab>`，从具名分支或 detached HEAD 直接创建并切换到 `feature-{ascii-kebab摘要}-{YYYYMMDD}`；当前路径是非主 Worktree 时，helper 同时登记该精确 Worktree 与新分支，不需要预建另一 Task 分支或重复登记。日期取 `Asia/Shanghai` 自然日，同名冲突由 helper 追加递增后缀。本地开始开发不要求存在 remote，也不因尚未配置远端而阻断。继续同一任务时复用当前发布周期已登记的同摘要 feature 分支，不创建链式叶子或第二条同任务分支。Harness 源维护使用当前请求已经建立的 feature 分支，不在模板根创建下游运行时状态或假装执行下游生命周期。

   若当前说明来自左侧 user-owned Task，首次编辑前必须核对描述中的 Task 编号、单一结果、范围、禁止范围、完成条件、保存项目完整路径和 `projectId`；Git Task 还须核对 repository identity、环境和起始提交。Git Task 必须位于独立 Codex Worktree，非 Git Local Task 须核对 cwd 与保存项目完整路径一致，且与同项目其他写入 Task 串行；保存项目与 Git Task 顶层的规范化 Git common-dir 必须相同，且 worktree registry 必须登记当前顶层。任一绑定不符都保持零写入，不退化到 plan、Subagent、Local Git checkout 或普通 Worktree。当前 Task 内部并行仍只在用户明确要求且 `parallel_worktree_subagents` 允许时调用 `$desktop-run-parallel-worktrees`。
   用户可见 Task 必须使用 `Task {序号} | {当前进度} | {单一结果}`；Task 编号和单一结果全程稳定，进度只能是 `已分配`、`运行中`、`检查中`、`已完成`。可读取线程元数据时以真实 `threadId` 对账标题、`projectId`、cwd 和状态，不靠标题承担身份。
   `user_owned_tasks: disabled` 时当前请求直接在当前 Task 中完成，不自动调用 `create_thread`；用户明确要求创建时仍执行。`enabled` 时，若交付物类型、生命周期阶段、外部副作用、禁止范围或验收责任使结果越出当前固定边界，任何写入前结束或暂停当前 Task 并创建新的 user-owned Task。证明同一结果的测试、review、checkpoint 和必要同范围修复不拆。每项目同一时间只允许一个写入型 active Task，Task0 只协调。创建必须按 Agent Policy 依次 `list_projects`、清点 Task 历史、以 `Task {序号} | 已分配 | {单一结果}` 调用一次 `create_thread`、取得真实 `threadId`、用 `list_threads` 复核，并核对干净工作区和起始提交；只返回 `clientThreadId` 或任一事实不符时保持零实现且不得重复创建。内部 Subagent 不使用此标题合同且不占用项目序号。
2. 对已初始化下游，若上次 Git 发布已由默认主分支和 tag 确认，先用 `$desktop-manage-git-lifecycle start` 从该已发布 HEAD 建立下一 feature 分支；在首个新改动和 `plan` 前，以该分支尚未变化的 HEAD 调用 `$desktop-manage-version finalize-release`，只读复核发布事实并解锁新周期。随后判断变化相对已发布规格与行为的性质。新目标、新能力、新使用场景或扩大既有需求只要有合理依据就优先分类为 `feature`/Minor；确认不属于新需求后，具有新稳定 ID 的问题修复或用户可感知优化分类为 `bug-fix`/Patch。确认同一事件或权威记录互相矛盾时，先收集两端证据、确认冲突并分配稳定纠错 ID，使用 `record-reconciliation`/Minor；低于最终版本的历史 `required_version` 不是冲突。用户批准的显式 Major 使用 `major`；查询、诊断、复现、未完成/重复处理、行为保持重构、内部优化、测试补强、文档、格式和清理使用 `maintenance`。产品边界不明确且不同答案会改变范围时，先确认具体需求；不得为了避开 Minor 而把疑似新需求改称修复。随后直接实现请求，不自动追加持久计划、构建候选或完整验收。
3. 日常开发先直接运行本次必要测试，不做环境预检。只有真实测试命令已经失败，且命令、退出状态和脱敏诊断明确表明缺少或不兼容的受管工具链/系统依赖时，才调用 `$desktop-check-development-environment`，成功后重试原命令一次；不得因新任务、新会话、首次代码修改、显式构建或缺少环境证据主动探测。纯文档/元数据任务不增加环境步骤。
4. 追踪真实执行路径，不留下模拟实现、桩、占位或仅有源码的片段。Core-first 是硬规则：接口/宿主无关的领域类型、业务规则、语义校验、默认值、用例编排、状态转换、稳定错误、平台无关权限、迁移和持久化策略必须在 core；adapter 只拥有装配、接口语法/协议、展示/纯交互状态、调用 core 和结果映射。适配器只拒绝无法解析、缺少协议必填字段或违反宿主能力约束的输入；值域、跨字段关系和其他业务有效性由 core 判定。GUI 交互 handler 必须绑定在实际拥有动作的按钮、链接、`Switch`、`Checkbox` 或菜单项本身，不得由 Card、`Table.Tr`、`Table.Td` 等父级代理；父级有独立动作时只执行自身语义并隔离传播冲突。普通页面的选项卡、查询/筛选、排序和分页由同一原生根视图 Entity 在进程内持有，跨页面保留、退出重置，不持久化或镜像 core 权威数据；成功且当前非第一页为空才回第一页。GPUI 列表不调用 Mantine 或浏览器存储专属 Skill。
5. 修改 adapter 前确认“适配器操作 → core API → core 测试”映射；仅修改布局、协议映射、系统托盘、窗口、终端恢复或 stdio 生命周期等接口/宿主机制时，记录其专属性理由。
6. 代码行为变化增加或更新本次开发需要的相关非空单元/回归测试：业务行为先覆盖 core 的核心成功路径和最高风险失败路径，再覆盖本次实际修改的适配器单元映射；缺陷修复增加能复现问题的回归测试。GUI 行内交互分别点击语义控件与周围父级区域，证明父级不会代理子控件动作；表格中的 `Switch` 不得因点击行或单元格而切换。GPUI 页面会话用同一原生根视图验证跨页面保留、新根视图恢复默认及成功空页回第 1 页；按本次真实数据/交互边界增加相关回归，不引入前端框架。
7. 只运行第 6 步的测试并立即修复失败。用户可见 Task 真实进入这些测试、review 或 checkpoint 前把标题进度更新为 `检查中`；检查失败并返回同范围修复时更新为 `运行中`，再次检查再进入 `检查中`。每次真实转换至多尝试一次，并按真实 `threadId` 用 `list_threads` 有界复读。内部 Subagent 不适用用户可见 Task 标题合同。纯文档、元数据、格式或不可合理单测的机械变更不创建空洞测试，只运行证明解析成功或差异完整所必需的最小检查。
8. 日常开发不得自动追加格式化、lint、静态、集成/契约、全仓测试、构建、文件行数、Rust/TypeScript 中文声明注释、core-first 机械门禁、冒烟、E2E、Verification 或人工复核。普通构建也只新增全量非空单元测试和实际构建，构建事实只进入 `release/` manifest、其声明的相邻制品证据和最终回复；其他治理检查仅在本次变化需要、用户明确请求或发布/渠道硬要求时运行。任一已运行检查失败仍必须在当前授权范围内修复并重跑。
9. 第 7 步的本次相关测试或最小替代检查通过、变化确实完成后，使用与第 2 步相同的参数调用 `$desktop-manage-version apply`。本周期首个功能提升一次 Minor；每个独立 `bug-fix` 稳定 ID 提升一次 Patch；本周期首个已确认记录冲突提升 Minor，即使功能锁已开启，后续冲突共用该版本。相同纠错 ID/证据跨周期重试不重复升级，证据变更失败关闭。新生成 Minor/Patch 采用 `0..99` base-100 自动进位；自动进位到 Major 是数值例外，显式 Major 仍需用户批准。不得在测试失败、修正未完成或诊断阶段提前提交版本。
10. 只更新被独立事件触发的记忆：产品目标/边界/约束/成功标准变化更新 Product Spec；长期重要决定/硬规则例外写 ADR；合格的可感知变化写 Changelog；重要阻断/交接、发布/完整验收/审计或用户要求再写 Product Status、Work Plan 或 Verification。Product Spec、ADR、Changelog 或 Work Plan 一旦独立触发，必须记录稳定 `change_id` 与版本门禁返回的 `required_version`；版本变化本身不触发任何记忆。构建请求、执行和结果本身不触发 Product Spec、ADR、Changelog、Product Status、Work Plan 或 Verification；未触发时不写占位。
11. 若用户提供了活动计划，只更新其中与本次实现直接对应的 Todo；计划不得改变本 Skill 的最小开发闭环或静默增加检查。
12. 对 Git 项目每完成一个逻辑闭环，都紧邻真实提交调用 `$desktop-configure-git-commits`，以表达结果的提交信息把当前范围的精确路径创建为可审查的本地提交，正常运行 hooks 且不得使用 `--no-verify`。普通完成不自动推送、发布或删除登记资源。用户明确要求发布时路由 `$desktop-prepare-release`，完成范围与版本审查、发布上下文和日志提交后，以精确 `--release-context-sha256 <sha256>` 调用单一路径 `release`：合并登记分支到本地默认主分支，创建并复读指向最终 HEAD 的 `v{版本}-{日期}`；不 fetch、push、打包或清理资源。完成初始化的下游随后必须按已确认的 `post_release_action` 调用现有适用打包 Skill，或以独立 `push-release --remote <name>` 把同一已发布 HEAD 放到本地小写 `release` 分支并同步远端默认主分支、同名 release 分支与 tag；复核真实制品或远端 refs 后才报告后续路径完成。最终交付前确认测试、权威文档、适用提交与干净工作树；未完成的所选路径明确报告；Harness 源的后续源码归档或推送仍由用户当次决定。
13. 第 12 步要求的结果、验证、文档及适用的本地提交、clean 状态，以及用户本次明确要求的推送或发布全部完成后，用户可见 Task 才把标题进度更新为终态 `已完成` 并按真实 `threadId` 用 `list_threads` 有界复读。更新失败不创建替代 Task、不无限重试、不推翻已经完成的结果，并在最终回复报告最后确认状态；阻断时保留最后真实进度。

## 边界与输出

- `$desktop-manage-git-lifecycle start` 只建立或复用本地 feature 开发周期，不要求或配置 remote。`release` 只在用户明确发布时合并本地默认主分支、创建并复读 tag；它不推送、打包或删除资源。`push-release --remote <name>` 只用于已确认的 `push_release_branch` 后续路径，目标失败不改变本地 Git 发布结论，但该后续路径仍未完成。普通开发期的 `publish` 仅在用户明确要求发布前推送主分支时使用，不能代替发布后推送。
- 单元测试只证明本次代码单元行为，不代表真实候选可用、完整验收或发布就绪。
- 报告变更分类、稳定 `change_id`、`required_version`、是否实际提升及幂等原因、本次实际运行的单元测试/最小替代检查、未执行项、剩余风险和适用的用户可见 Task 进度标题状态；左侧 Task 另报告分支、提交哈希和干净状态，只有用户要求的活动计划存在时报告 Todo 状态。
