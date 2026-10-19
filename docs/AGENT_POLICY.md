---
schema_version: 4
confirmed_by: ManonLoki (explicit confirmation in Codex chat)
confirmed_at: 2026-10-09
decision_mode: reuse_then_infer_then_ask
superpowers: disabled
user_owned_tasks: disabled
parallel_worktree_subagents: disabled
acceptance_smoke: enabled
e2e_hint: disabled
post_release_action: local_package
---

# Agent 运行策略

本文件是本项目 Agent 能力、用户可见 Task 自动拆分、候选冒烟偏好、E2E 建议默认值和发布后动作的唯一持久事实来源。五项能力仅 enabled/disabled，post_release_action 仅 local_package/push_release_branch，确认元数据来自本次用户表单，不允许 pending。Git 发布在本地默认主分支与 tag 完成，之后执行并检测本次冻结的发布后动作。

GUI 框架取根 Cargo metadata，本项目明确为 gpui，不增加策略字段。已确认九字段与空热键 action contract 取 GUI_APP_PROFILE，不重复询问；用户已选择保留中性行为。GPUI 使用原生 Rust、Node.js/pnpm 工程入口及独立 GPUI 打包 Skill，不进入 Tauri 插件/前端/初始化 E2E 路线。

## 字段语义

- `superpowers`：`enabled` 允许按任务触发名称以 `superpowers:` 开头的 Skills；`disabled` 禁止后续规划、实现、验证和发布调用或遵循这些 Skills。
- `user_owned_tasks`：控制是否由 Agent 自动把新结果拆到 Codex 左侧菜单中用户可独立进入的 user-owned Task/thread。`disabled` 是默认值：不得自动创建或按阶段拆 Task，但用户明确要求创建时仍可执行；`enabled` 表示用户已经长期授权按本文件的结果边界自动创建，不得再次要求用户提醒。它不控制当前 Task 内部的 plan、Subagent 或单元 Worktree，也不替代 `parallel_worktree_subagents`。
- `parallel_worktree_subagents`：`enabled` 只表示用户明确要求并行时允许使用 Worktree + 写入型 Subagent；`disabled` 使用单 Agent 当前工作树。持久启用本身不能触发并行步骤；启用时仍须给每个写入单元分配不重叠的文件所有权，并把创建出的 Worktree 和分支登记到当前 Git 生命周期。
- `acceptance_smoke`：只在完整真实候选验收中，允许 Agent 对候选执行适用的冒烟测试。
- `e2e_hint`：仅作为每次显式发布候选构建询问 E2E 时展示的建议默认值；无论是 `enabled` 还是 `disabled`，都不能替代当前候选的明确选择，也不授权凭据、支付、生产数据、发布或不可逆副作用。
- `post_release_action`：`local_package` 表示 Git 发布后进入现有适用的本地打包流程；`push_release_branch` 表示把已发布的同一提交放到本地小写 `release` 分支，非强制推送远端默认主分支、同名 release 分支与发布 tag。初始化表单必须给出两项并以 `local_package` 为建议默认值；仅所选接口含 CLI，或 GUI 已选框架具备适用本地打包路线时，该默认项才可确认：GPUI 支持 macOS/Windows 及 Linux GNU x64/ARM64，Tauri 支持 macOS/Windows。纯 TUI/MCP 或不含 CLI 的仅 Linux Tauri GUI 须明确选择 `push_release_branch`，不得暗中应用不可执行的默认项。用户须明确选择或确认可执行的默认值。选择在未来发布中持续生效，可用 `$desktop-switch-post-release-action` 更改。
- `decision_mode: reuse_then_infer_then_ask`：复用能力偏好；对 E2E 则先读取建议默认值，再复用当前候选构建请求中已经明确的选择，否则在构建前询问一次。发布后动作直接读取 `post_release_action`，缺失、非法或 `pending` 都阻断后续流程完成。

`enabled` 表示“允许且适用时执行”，`disabled` 表示默认不启用可选能力。`user_owned_tasks: enabled` 是自动建 Task 的明确长期授权；`post_release_action` 是用户对未来 Git 发布后所选动作的长期授权，不授权配置远端、凭据、签名、渠道分发或绕过候选构建的当次 E2E 选择。安全、产品和渠道硬要求优先于项目偏好，不能用 `disabled` 绕过必需门禁；当前候选构建的 E2E 明确选择优先于 `e2e_hint` 建议值。需要远端源码或远程构建的渠道必须先走 `push_release_branch` 并复核远端 `release` 分支与 tag；该要求不改变 Git 发布完成条件。

除非用户明确说“当前 Task 内部步骤、plan 或 Subagent”，本文的 Task 一律指 Codex 左侧菜单中用户可独立进入的 user-owned Task/thread。plan、Todo、Subagent、agent thread、Worktree、brief、report、review、checkpoint 和内部单元都不是新的左侧 Task；不得用这些内部结构冒充 `create_thread` 创建的用户可见 Task。

Task 标题、编号、四种进度及真实绑定的操作合同统一见 `$desktop-task-workflow`；原确认选择继续取本文件，不以 Skill 默认值补写缺失字段。

## 开发分支与主分支发布生命周期

本项目已完成中性初始化。新功能或独立修复写入前由 $desktop-manage-git-lifecycle 建立独立 feature 分支，本地创建不依赖远端。实际分支命名、资源登记和恢复由该 Skill 的标准库 helper 唯一执行。

- 首次真实开发才创建 Git common dir 的 agent-first-harness/git-lifecycle.json，精确登记分支、Worktree 与发布进度；不扫描或删除链外资源，不设置保护分支、严格线性、fast-forward-only、lease 或 atomic push。
- 明确发布时先提交源码/已触发记录并冻结 sourceHead，再生成双语 release-notes.json 与 .harness/release-context.json；这两份是唯一发布元数据提交内容。release 必须传精确 --release-context-sha256，普通合并登记分支到本地默认主分支，创建并复读 v{版本}-{YYYYMMDD} 指向最终 HEAD。
- 本地默认主分支与 tag 精确一致即完成 Git 发布；这一步不 fetch、push、打包或清理登记资源。下一开发分支复核主分支/tag 后才 finalize-release 复位版本周期。
- Git 发布开始时冻结已确认的 `post_release_action`，保存为生命周期 pending/last release 的 postReleaseAction；重试消费原快照，未来切换不改旧发布。当前 local_package 随后进入 GPUI 打包 Skill，并检测实际结果；失败不撤销 Git 发布，但不得声称后续流程完成。
- 若未来明确切换 push_release_branch，则独立 `push-release --remote <name>` 从最近发布记录读取固定 HEAD/tag，安全创建或快进本地小写 release 分支，非强制推送具名远端默认主分支、`release` 分支和 tag，三者逐项复读。一个远端可复用，多个须明确选择；缺少远端/凭据时停止，不创建配置或凭据。
- 普通 publish 仅用于用户明确要求的发布前主分支推送，不创建 tag、不替代发布后推送。发布不自动删除分支或 Worktree，清理由独立请求触发。

## 用户可见 Task 开关、粒度与创建门禁

本文件保留项目选择和本地限制；标题、编号、描述模板、创建绑定、恢复与授权继承的唯一操作合同见 [$desktop-task-workflow](../.agents/skills/desktop-task-workflow/SKILL.md) 及其 [Task 合同](../.agents/skills/desktop-task-workflow/references/task-contract.md)。旧三个入口只作兼容路由，不复制合同。

`user_owned_tasks: disabled` 保持当前请求在同一会话完成，不自动拆左侧 Task。显式创建只授权对应结果，不永久开启开关。一个 Task 固定一个结果、范围、禁止范围和完成条件；必要阅读、实现、开发回归与同范围返工不按步骤拆分。正式验收有独立责任，不能由开发回归冒充。项目原有限制继续保留：每项目同时仅一个写入型 active 用户可见 Task；Task0 只协调。

### 所选环境与 Local 串行交接

`task_worktrees` 仅选择后续 Git Task 的环境，不触发 Task 创建。启用后用 `environment.type=worktree`，关闭或非 Git 用 `environment.type=local`；派发记录冻结环境。同一 Local cwd 串行，任何单元只在自己的已绑定环境修改文件，不进入其他任务的目录或清理资源。

当前合法 schema 4 没有该字段，状态为 `selection_required`，不推断默认值。新 Git Task 分配环境前必须通过策略 Skill 取得明确选择；既有 Task 按原冻结环境恢复，不因工程升级迁移、中断或改挂。实际分支、登记及整合仍由 Git 生命周期 Skill 负责，详细工作树操作见 [工作树执行](../.agents/skills/desktop-task-workflow/references/worktree-execution.md)。`parallel_worktree_subagents: disabled` 继续关闭写入型内部 Worktree，不影响只读协作。

已授权范围内的资料阅读、实现、同范围修复、必要检查与适用本地提交自主执行，绑定自检通过后不再增加人工 BOUND 批准。真实超范围、未授权不可逆副作用或新增凭据/权限仍须按对应门禁确认。queued、绑定不符或资源归属不明保持零实现；不重复创建替代任务，不删除已有分支或工作树。

## 初始化与持久化

- 当前 schema_version 为 4，保持合法兼容读取。五项能力、post_release_action、confirmed_by、confirmed_at 均已由本次表单确认，不接受 pending，不从上游继承批准。
- 用户可明确开启/关闭左侧 Task 或自动 Task 拆分；更新 user_owned_tasks 与确认元数据并在当日 ADR 记录前后值、原因、影响和恢复条件。相同目标值幂等 no-op，不重写元数据或 ADR。
- 开关只影响之后结果边界，不迁移、中断、删除或改挂既有 Task/Worktree。关闭后仍响应显式建 Task 请求，开启后仅新结果边界自动创建。
- $desktop-switch-post-release-action 只在 local_package / push_release_branch 间切换未来动作；原子更新受保护字段，保留五项能力及初始化确认，ADR 记录本次确认与影响。其他永久策略变更同样须用户确认和 ADR。
- 工程升级不改写本文件 frontmatter、确认元数据或选择。旧 schema 3 的发布动作由 `$desktop-switch-post-release-action` 补选后只迁移至 4；schema 4 的 `task_worktrees` 仅在用户明确选择后由 `$desktop-configure-agent-policy migrate-task-worktrees` 受限迁移至 5，保留原五项、动作、确认事实与正文，新选择来源写真实 ADR。进行中的旧发布/推送先恢复或核对，不把新选择套用旧事实。

## 开发与构建

- 日常直接实施，只运行必要相关非空测试，不自动追加计划、全仓检查、构建、E2E 或人工复核。
- parallel_worktree_subagents 只控制当前 Task 内部受管并行 Worktree，不自动创建左侧 Task；当前关闭，读取型协作不形成写入 Worktree。
- 普通 GPUI 本地试包使用 $desktop-build-gpui-release 的 local 模式，不升级为发布候选，不生成发布日志、上下文、release/、提交或 tag。
- Linux GNU x64/ARM64的同系统构建、交叉目标依赖与deb/tar.gz打包按`docs/LINUX_GPUI_TOOLCHAIN.md`及本项目保留的GPUI Skill执行；宿主单元测试不替代交叉目标运行证据。不改变已确认的发布后动作或当前候选的独立选择。
- 发布候选需 clean 已发布默认主分支/tag，运行项目全部非空单元测试，每次单独解析 e2eSelection。当前请求明确时复用；否则先读 e2e_hint 建议再询问一次，产品/渠道硬要求优先。只把本次选择写候选证据，不改本策略。
- E2E 启用时只运行完整最终候选；禁用记录 Not run、原因和风险。acceptance_smoke 的允许不替代产品批准和完整候选要求。GPUI 不使用 Tauri 插件、前端测试或 Tauri 初始化 E2E Skill。
- macOS 签名默认 disabled/not-requested 且不探测身份；仅已批准配置、当次主动要求或渠道硬要求才启用，签名、公证、stapling 完整执行，不降级。Windows 运行验收只能在真实 Windows 宿主完成。
- 安全/隐私、不可逆操作、对外契约及产品/渠道硬要求不因 disabled 跳过门禁。发布当次 reviewSelection 独立解析，必要审查强制启用；普通语义审查不混入日常开发。
