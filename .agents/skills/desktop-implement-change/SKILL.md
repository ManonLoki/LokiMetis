---
name: desktop-implement-change
description: 直接实施范围清楚的请求，只运行本次开发需要的单元/回归测试并更新被真实事件触发的记录；不得自动追加计划、全仓检查、构建或验收。
---

# 实施变更

完成当前授权范围，以本地 feature 分支、实现、本次必要测试、事件触发记录和可审查的本地提交构成日常闭环。用户明确要求发布时转 `$desktop-prepare-release`；Git 发布及后续动作由该入口编排。

修改 GUI 时先读取根 Cargo metadata 的 `gui-framework`，既有 GUI 缺失时兼容 `tauri`。下文 React/Mantine/Jotai/Query、浏览器存储、`$mantine-list-view`、TypeScript与Tauri能力只约束Tauri路径。`gpui` 按 `$desktop-add-gpui-adapter` 与 `docs/design_standards/gpui_gui.md` 使用原生Rust视图、gpui-kit与rust-i18n；页面会话由同一原生根视图在进程内持有，页面切换保留、退出重置，不复制core权威数据。控件自身动作与周围父级不串扰、core-first、中文声明注释和本次相关非空Rust回归同样适用；GPUI 同样使用 Node.js/pnpm 运行项目根验证、发布和打包脚本；不得因为通用Skill提到前端而为GPUI安装React或Mantine。五项原生能力只按 profile 与已批准配置启用，依赖/补丁由 `$desktop-manage-gpui-dependencies` 管理，明确性能任务由 `$desktop-optimize-gpui-performance` 管理，选择后问询仍由 GPUI adapter Skill 管理；深链接仍 unavailable/disabled。

技术选型、当前依赖声明、平台条件和本地补丁先查[项目技术清单](../../../docs/TECHNOLOGY.md)与根Cargo；不要把保留初始化renderer的旧版本当作当前项目下界，也不要把本机忽略锁文件当固定解析合同。

## 工作流程

左侧 Task、工作树、编号、绑定、权限继承与进度统一由 `$desktop-task-workflow` 及命中引用管理；项目当前 schema 4 的确认事实和限制保留，缺少 `task_worktrees` 不由工程升级补默认值。普通实现不创建左侧 Task。

1. 检查 `AGENTS.md`、`docs/AGENT_POLICY.md`、`docs/ENGINEERING_RULES.md`，有 Git 时再检查 Git 根/分支和工作区；只读取当前变更直接需要的 Product Spec、ADR、接口或技术事实。若规范根同时包含 Harness 专用 `Version.md` 和活动 `$desktop-instantiate-project`，只允许修改 Harness 自身规则、Skills、中性资产、脚本、验证器、文档或其交付工程；任何产品目的、业务功能、产品专属 UI/文案/数据、远程地址、凭据、构建或发布需求都必须在写入前拒绝，并要求用户切换到终端下游后重新提出。保护用户已有修改，重叠无法安全处理时停止。对于已初始化下游的新功能或独立 Bug，在任何项目文件写入前自动调用 `$desktop-manage-git-lifecycle start --project-root <project-root> --summary <ascii-kebab>`，从具名分支或 detached HEAD 直接创建并切换到 `feature-{ascii-kebab摘要}-{YYYYMMDD}`；当前路径是非主 Worktree 时，helper 同时登记该精确 Worktree 与新分支，不需要预建另一 Task 分支或重复登记。日期取 `Asia/Shanghai` 自然日，同名冲突由 helper 追加递增后缀。本地开始开发不要求存在 remote，也不因尚未配置远端而阻断。继续同一任务时复用当前发布周期已登记的同摘要 feature 分支，不创建链式叶子或第二条同任务分支。Harness 源维护使用当前请求已经建立的 feature 分支，不在模板根创建下游运行时状态或假装执行下游生命周期。

   若当前说明来自左侧 user-owned Task，先按 `$desktop-task-workflow` 核对真实绑定与结果边界；queued 或绑定失败保持零实现。当前请求内的只读协作不改变项目写入型并行选择。

2. 对已初始化下游，若上次 Git 发布已由默认主分支和 tag 确认，先用 `$desktop-manage-git-lifecycle start` 从该已发布 HEAD 建立下一 feature 分支；在首个新改动和 `plan` 前，以该分支尚未变化的 HEAD 调用 `$desktop-manage-version finalize-release`，复核发布事实并解锁新周期。随后完整读取 [版本分类与工作流程](../desktop-manage-version/SKILL.md)，按其新需求优先规则选择 `feature`、`bug-fix`、`record-reconciliation`、`major` 或 `maintenance`，通过 `plan` 取得稳定 `change_id` 对应的 `required_version`；保留参数供第 9 步 `apply`。分类、冲突证据、周期与进位规则只由该 Skill 维护。产品边界不明确且不同答案会改变范围时先确认，随后直接实现请求，不自动追加持久计划、构建候选或完整验收。
3. 日常开发先直接运行本次必要测试，不做环境预检。只有真实测试命令已经失败，且命令、退出状态和脱敏诊断明确表明缺少或不兼容的受管工具链/系统依赖时，才调用 `$desktop-check-development-environment`，成功后重试原命令一次；不得因新任务、新会话、首次代码修改、显式构建或缺少环境证据主动探测。纯文档/元数据任务不增加环境步骤。
4. 实际新增、替换或提高依赖下界时调用 `$desktop-manage-dependencies`。已批准功能确需文件读写或外部程序时，分别调用 `$desktop-add-file-operations` 或 `$desktop-add-external-process`，完成后返回同一实施流程，不重复分支、版本或提交。追踪真实执行路径，不留下模拟实现、桩或占位。Core-first 是硬规则，按 `docs/ENGINEERING_RULES.md` 的归属清单实施：接口/宿主无关内容进入 core，adapter 只做协议、展示、纯交互与结果映射；适配器只拒绝语法/协议错误或违反宿主能力约束的输入；语义校验和业务编排不得留在 adapter。GUI 交互、页面状态与存储边界按上方命中的框架标准实施，不在本入口另维护一套规则。
5. 修改 adapter 前确认“适配器操作 → core API → core 测试”映射；仅修改布局、协议映射、系统托盘、窗口、终端恢复或 stdio 生命周期等接口/宿主机制时，记录其专属性理由。
6. 代码行为变化增加或更新本次开发需要的相关非空单元/回归测试：先覆盖 core 的核心成功与最高风险失败路径，再覆盖实际修改的 adapter 映射；缺陷修复增加能复现问题的回归。GUI 按命中的框架标准验证控件事件所有权、状态保留/恢复与失败路径；GPUI 仍按原生框架标准验证，不调用 Tauri 列表入口。
7. 只运行第 6 步的测试并立即修复失败。左侧 Task 的检查与返工进度交 `$desktop-task-workflow` 按真实转换同步。纯文档、元数据、格式或不可合理单测的机械变更不创建空洞测试，只运行证明解析成功或差异完整所必需的最小检查。
8. 日常开发不得自动追加格式化、lint、静态、集成/契约、全仓测试、构建、文件行数、Rust/TypeScript 中文声明注释、core-first 机械门禁、冒烟、E2E、Verification 或人工复核。普通构建也只新增全量非空单元测试和实际构建，构建事实只进入 `release/` manifest、其声明的相邻制品证据和最终回复；其他治理检查仅在本次变化需要、用户明确请求或发布/渠道硬要求时运行。任一已运行检查失败仍必须在当前授权范围内修复并重跑。
9. 第 7 步的本次相关测试或最小替代检查通过、变化确实完成后，使用与第 2 步相同的参数调用 `$desktop-manage-version apply`。本周期首个功能提升一次 Minor；每个独立 `bug-fix` 稳定 ID 提升一次 Patch；本周期首个已确认记录冲突提升 Minor，即使功能锁已开启，后续冲突共用该版本。相同纠错 ID/证据跨周期重试不重复升级，证据变更失败关闭。新生成 Minor/Patch 采用 `0..99` base-100 自动进位；自动进位到 Major 是数值例外，显式 Major 仍需用户批准。不得在测试失败、修正未完成或诊断阶段提前提交版本。
10. 只更新被独立事件触发的记忆：产品目标/边界/约束/成功标准变化更新 Product Spec；长期重要决定/硬规则例外交 `$desktop-record-adr` 写 ADR；合格的可感知变化写 Changelog；重要阻断/交接由 `$desktop-handoff-project` 按真实事件更新 Product Status，Work Plan 与 Verification 各按自身准入；候选构建与完整验收只写忽略的 release/ 证据。Product Spec、ADR、Changelog 或 Work Plan 一旦独立触发，必须记录稳定 `change_id` 与版本门禁返回的 `required_version`；版本变化本身不触发任何记忆。构建请求、执行和结果本身不触发 Product Spec、ADR、Changelog、Product Status、Work Plan 或 Verification；未触发时不写占位。
11. 若用户提供了活动计划，只更新其中与本次实现直接对应的 Todo；计划不得改变本 Skill 的最小开发闭环或静默增加检查。
12. 对 Git 项目每完成一个逻辑闭环，都紧邻真实提交调用 `$desktop-configure-git-commits`，以中文摘要和正文表达结果，执行消息机械检查、复核差异并只提交本次精确路径；正常运行 hooks，不使用 `--no-verify`，提交后复读实际消息。最终交付前确认测试、权威文档、适用提交与干净工作树。用户明确要求发布时路由 `$desktop-prepare-release`，由其消费已确认的 `post_release_action` 并完成 Git 发布和适用后续路径；普通完成不自动推送、发布或删除登记资源。
13. 第 12 步要求的交付闭环完成后，仅存在左侧 Task 时调用 `$desktop-task-workflow` 同步真实完成状态；失败如实报告最后确认状态，不创建替代 Task。

## 边界与输出

- 分支、发布前推送、Git 发布和发布后推送统一委托 `$desktop-manage-git-lifecycle`；只有命中对应请求才读取其操作合同，不在本 Skill 复制远端选择、恢复 journal 或发布后规则。
- 单元测试只证明本次代码单元行为，不代表真实候选可用、完整验收或发布就绪。
- 报告变更分类、稳定 `change_id`、`required_version`、是否实际提升及幂等原因、本次实际运行的单元测试/最小替代检查、未执行项、剩余风险和适用的用户可见 Task 进度标题状态；左侧 Task 另报告分支、提交哈希和干净状态，只有用户要求的活动计划存在时报告 Todo 状态。
