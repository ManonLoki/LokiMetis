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
- `post_release_action`：`local_package` 表示 Git 发布后进入现有适用的本地打包流程；`push_release_branch` 表示把已发布的同一提交放到本地小写 `release` 分支，非强制推送远端默认主分支、同名 release 分支与发布 tag。初始化表单必须给出两项并以 `local_package` 为建议默认值；仅所选接口含 CLI，或含 GUI 且目标平台含 macOS/Windows 时，该默认项才可确认。纯 TUI/MCP 或仅 Linux GUI 须明确选择 `push_release_branch`，不得暗中应用不可执行的默认项。用户须明确选择或确认可执行的默认值。选择在未来发布中持续生效，可用 `$desktop-switch-post-release-action` 更改。
- `decision_mode: reuse_then_infer_then_ask`：复用能力偏好；对 E2E 则先读取建议默认值，再复用当前候选构建请求中已经明确的选择，否则在构建前询问一次。发布后动作直接读取 `post_release_action`，缺失、非法或 `pending` 都阻断后续流程完成。

`enabled` 表示“允许且适用时执行”，`disabled` 表示默认不启用可选能力。`user_owned_tasks: enabled` 是自动建 Task 的明确长期授权；`post_release_action` 是用户对未来 Git 发布后所选动作的长期授权，不授权配置远端、凭据、签名、渠道分发或绕过候选构建的当次 E2E 选择。安全、产品和渠道硬要求优先于项目偏好，不能用 `disabled` 绕过必需门禁；当前候选构建的 E2E 明确选择优先于 `e2e_hint` 建议值。需要远端源码或远程构建的渠道必须先走 `push_release_branch` 并复核远端 `release` 分支与 tag；该要求不改变 Git 发布完成条件。

除非用户明确说“当前 Task 内部步骤、plan 或 Subagent”，本文的 Task 一律指 Codex 左侧菜单中用户可独立进入的 user-owned Task/thread。plan、Todo、Subagent、agent thread、Worktree、brief、report、review、checkpoint 和内部单元都不是新的左侧 Task；不得用这些内部结构冒充 `create_thread` 创建的用户可见 Task。

用户可见 Task 的标题唯一使用 `{Task编号} | {当前进度} | {单一结果}`。`Task编号` 固定为 `Task ` 加无前导零的正十进制序号，例如 `Task 8 | 运行中 | 左侧 Task 默认关闭并支持开关`；两个 ASCII `|` 两侧各恰好一个 ASCII 空格。`单一结果` 必须非空、单行、首尾无空白且不含 `|`，开始后不得改变；`当前进度` 只取 `已分配`、`运行中`、`检查中`、`已完成`。调用后返回的 `threadId`/`clientThreadId`、Subagent 技术 `task_name` 和 Git ref 只用于真实身份或技术绑定，不得生成 Task 编号或反填标题。

同一 `hostId` 与精确 `projectId` 的用户可见 Task 共享递增序列。新结果先调用 `list_threads(limit=50)` 合并当前与 pinned Task，再沿同宿主 `list_archived_threads` 的 `nextCursor` 读完归档页；只接受 `kind=codex`、宿主/项目精确匹配且满足当前三字段标题的记录作为序号证据，然后分配最大有效序号加 1。空历史才从 1 开始，缺号不回填，同项目批量创建连续预留；不识别任何历史标题格式，不为已废弃的旧标题保留兼容或序号连续性。隐藏 Subagent、`projectId=null`、其他项目/宿主/kind 和不合规标题都不占用项目序列。宿主或历史无法可靠枚举时不得猜测 `1`，而是保留同一结果已有编号或在创建前阻断。

进度标题贯穿用户可见 Task。`create_thread` 使用 `Task {序号} | 已分配 | {单一结果}`；目标 Task 取得执行权后进入 `运行中`，真实进入本次必要测试、review 或 checkpoint 时进入 `检查中`，同范围返工时回到 `运行中`，完成全部结果、必需检查及要求的提交/推送/远端复读后才进入终态 `已完成`。每次真实转换至多尝试一次 `set_thread_title` 并省略 `threadId`，再用真实 `threadId` 调用 `list_threads` 有界复读。创建后首次核对中的标题不符是零写入阻断；后续进度更新失败只需如实报告，不创建替代 Task、不无限重试，也不推翻已经完成的工作。内部 Subagent 不使用本标题合同、不占用 Task 序号，内部单元 Worktree 本身没有左侧 Task 标题。

## 开发分支与主分支发布生命周期

本项目已完成中性初始化。新功能或独立修复写入前由 $desktop-manage-git-lifecycle 建立独立 feature 分支，本地创建不依赖远端。实际分支命名、资源登记和恢复由该 Skill 的标准库 helper 唯一执行。

- 首次真实开发才创建 Git common dir 的 agent-first-harness/git-lifecycle.json，精确登记分支、Worktree 与发布进度；不扫描或删除链外资源，不设置保护分支、严格线性、fast-forward-only、lease 或 atomic push。
- 明确发布时先提交源码/已触发记录并冻结 sourceHead，再生成双语 release-notes.json 与 .harness/release-context.json；这两份是唯一发布元数据提交内容。release 必须传精确 --release-context-sha256，普通合并登记分支到本地默认主分支，创建并复读 v{版本}-{YYYYMMDD} 指向最终 HEAD。
- 本地默认主分支与 tag 精确一致即完成 Git 发布；这一步不 fetch、push、打包或清理登记资源。下一开发分支复核主分支/tag 后才 finalize-release 复位版本周期。
- Git 发布开始时冻结已确认的 `post_release_action`，保存为生命周期 pending/last release 的 postReleaseAction；重试消费原快照，未来切换不改旧发布。当前 local_package 随后进入 GPUI 打包 Skill，并检测实际结果；失败不撤销 Git 发布，但不得声称后续流程完成。
- 若未来明确切换 push_release_branch，则独立 `push-release --remote <name>` 从最近发布记录读取固定 HEAD/tag，安全创建或快进本地小写 release 分支，非强制推送具名远端默认主分支、`release` 分支和 tag，三者逐项复读。一个远端可复用，多个须明确选择；缺少远端/凭据时停止，不创建配置或凭据。
- 普通 publish 仅用于用户明确要求的发布前主分支推送，不创建 tag、不替代发布后推送。发布不自动删除分支或 Worktree，清理由独立请求触发。

## 用户可见 Task 开关、粒度与创建门禁

`user_owned_tasks: disabled` 是默认状态：普通请求直接在当前 Task 完成，Agent 不因结果或生命周期边界自动调用 `create_thread`。用户仍可明确要求创建左侧 Task；该次明确要求只授权对应结果，不自动永久开启本字段。`enabled` 是长期授权：当新结果超出当前 Task 的固定边界时，Agent 必须先结束或暂停当前 Task，再自动创建新的左侧 user-owned Task，不重复询问。用户明确说“不要创建新 Task”或“作为当前 Task 内部步骤”时，本次请求优先于长期开关。

一个用户可见 Task 只对应一个明确、可验收的结果、一个固定完成边界和一个独立工作区。标题、目标、实施范围、禁止范围和完成条件开始后固定。判断是否需要新 Task 只看结果是否变化，不看工作量；交付物类型变化、生命周期阶段变化、增加安装/发布/上传/远端/系统修改等外部副作用、触碰原 Task 禁止范围或验收责任变化，都属于新结果。设计到实现、诊断到修复、迁移到打包安装、实现到发布上传、构建通过到真实安装及人工测试交付是典型边界。用于证明当前结果的测试、review、checkpoint 和必要同范围缺陷修复仍留在当前 Task。

每个项目同一时间只允许一个写入型 active 用户可见 Task；Task0 可以协调和派发，但不得代替实施 Task 写入。`parallel_worktree_subagents` 只控制当前 Task 内部按用户明确要求创建的 Subagent Worktree，既不创建用户可见 Task，也不改变本节开关或粒度。

### 创建状态机

1. **RESOLVED**：依次调用 `list_projects` 和 `list_threads`。按规范化完整路径精确锁定项目名称、`projectId`、Git 状态与保存路径，确认本项目没有另一个写入型 active Task，并清点当前与归档标题分配序号。Git 项目固定选择 `target.type = project`、精确 `projectId` 和 `environment.type = worktree`；非 Git 项目固定使用同一 project target 与 `environment.type = local`。不得使用 projectless、其他项目或普通 Worktree 代替 user-owned Task。
2. **DISPATCHED**：对一个结果只调用一次 user-owned `create_thread`，显式传入 `title="Task {序号} | 已分配 | {单一结果}"`。返回真实 `threadId` 才能继续；只返回 `clientThreadId` 表示仍在 setup，必须报告 queued 并保持零实现，不得声称 Ready、把它传给要求 `threadId` 的工具、重复创建或在当前 Task/plan/Subagent 中代做。
3. **VERIFIED**：取得真实 `threadId` 后立即用 `list_threads` 精确核对该 id 的标题、`projectId`、cwd 和状态。目标 Task 可以已经合法进入 `运行中`，但 Task 编号与单一结果必须保持不变。左侧不可见、字段为空/错误、标题不合规或候选有歧义都立即阻断，禁止创建重复 Task 碰运气。
4. **BOUND**：首次写入前核对工作区干净及起始提交正确。Git Task 分别在保存项目和 Task Worktree 执行 `git rev-parse --path-format=absolute --git-common-dir`，确认两者 Git common dir 相同，并用 `git worktree list --porcelain` 确认当前顶层已登记；非 Git Task 的 cwd 必须等于保存项目完整路径。任一绑定、工作区或基线不符时保持零实现，不退化为 plan、Subagent、Local Git checkout 或普通 Worktree，也不要求用户重新解释规则。

### 命名、基线与分支

- 用户可见 Task 遵守 `Task {序号} | {当前进度} | {单一结果}`。创建后以真实 id、标题和 `projectId` 对账，标题不承担身份判断；`threadId`/`clientThreadId` 不生成序号。
- Task 描述记录稳定 Task 编号、单一结果、实施范围、禁止范围、完成条件、独立 ASCII `feature-summary`、目标 `projectId`、保存项目完整路径、Git repository identity 和起始分支/提交。显示标题与 Git 摘要互不推导。
- 只读或非产品写入 Task 在用户明确指定时按该 branch/ref 创建，否则使用保存项目默认主分支的已提交 HEAD，不硬编码 `main` 或 `master`，也不主动 fetch/pull。会形成产品变更的 Task 在首次写入前调用 `$desktop-manage-git-lifecycle start --summary <feature-summary>`；helper 从当前 HEAD 建立唯一 `feature-*` 分支，并在当前路径是非主 Worktree 时同时把分支和 Worktree 精确登记到本发布周期。当前 Task 内部单元仅在策略允许且用户明确要求时可按不重叠所有权并行；它不豁免每项目单一写入型 active 用户可见 Task 规则，本项目内部并行 Worktree 当前 disabled。基线必须已有提交；除非用户明确要求从 working tree 状态开始，否则不得复制未提交修改。基线不明确、分叉或修改无法安全归属时停止创建。
- Codex 管理 Worktree 默认可能处于 detached HEAD。Ready Git Task 在首次编辑前以描述中的 ASCII `feature-summary` 调用生命周期 `start`；helper 从当前 HEAD 创建并切换到唯一 `feature-*` 分支，同时登记当前 Worktree。`feature-summary` 只服务安全分支命名，不是显示标题。非 Git Local Task 不执行 Git 分支步骤。

### 执行、提交与边界

- Git Task 只在自己的独立 Worktree 修改文件，不直接编辑 Local 主工作目录，也不进入、清理或复用其他 Task 的 Worktree。非 Git Task 只在绑定的 Local 项目目录修改。两者都保护已有修改且不得扩大固定范围。
- 每完成一个能够独立说明结果的逻辑闭环就提交一次。提交信息按 `$desktop-configure-git-commits` 表达已经得到的结果，例如 `feat: implement WeChat accessibility selectors`、`fix: reject stale accessibility identities` 或 `docs: record capability gate evidence`；简单变化可只写主题，非简单变化保留 Why/Changes/Impact/Test，未运行测试明确写 `Not run` 原因。不得把构建缓存、`target/`、`node_modules/`、`dist/`、`.cache/` 或其他忽略生成物加入提交。
- `feature-*` 和 `codex/unit-*` 是受管实施分支。Task 不自行覆盖最终应用、删除其他 Worktree、合并或推送默认主分支，也不执行未经任务授权的发布、签名或系统副作用。生命周期 helper 精确登记这些资源；Git 发布完成后保留登记清单，后续清理由用户独立要求触发并复核资源身份。
- 最终交付前必须确认任务要求的测试已经执行、相关权威文档已经同步、全部任务改动已经提交，并且 `git status --porcelain=v1 --untracked-files=all` 为空。适用目标必须编译并验证真实目标行为；任务没有可编译产物或完整候选不在范围内时，必须把该项明确报告为 `Not applicable` 或 `Not run`，不得伪造通过，也不得因此自动扩大为构建/E2E/完整验收。

### 协调方整合与清理

- Task 完成后只报告分支、提交哈希、实际验证、未执行项和剩余风险，不自行合并或推送默认主分支。协调 Task 或用户读取该报告后，只核对任务范围、疑似秘密、任务明确要求的测试、工作树 clean 以及分支/Worktree 的精确登记；用户明确说“推送”或“发布”时，由 `$desktop-manage-git-lifecycle` 以普通 merge 汇总这些登记分支。
- Task Worktree 和临时分支在整合后仍保留于生命周期清单。Git 发布仅以本地默认主分支合并和 tag 复读成功为终点，不自动删除 Worktree、本地分支或远端分支。Task 本身不得删除其他 Task 资源；后续清理须由独立请求精确指定。

### 统一 Task 描述模板

以下各节都是必填项。创建者必须把占位句替换为当前任务的真实事实；不适用项保留并明确标为 `Not applicable`，不得删节后让执行边界变得含糊。

```markdown
目标：
完成一个明确、可独立验收的结果。

Task 绑定：

- Task key：填写派发前分配的不可变技术标识；它不进入显示标题，也不得填写返回后的 `threadId` 或 `clientThreadId`。
- Task 编号：填写同一 `hostId`/`projectId` 下当前与归档有效标题最大序号加一所得正整数；同一结果全程不变，缺号不回填。
- 显示标题：填写按 `Task {序号} | 已分配 | {单一结果}` 形成并传给 `create_thread.title` 的完整字符串；后续只替换进度字段。
- Git feature summary：Git Task 填写独立 ASCII kebab-case `feature-summary`，供 `$desktop-manage-git-lifecycle start --summary` 使用；非 Git 时标记 `Not applicable`。
- Codex 项目：填写名称、`projectId` 和保存项目完整路径。
- Git 绑定：填写 repository identity、起始分支和起始提交；非 Git 时标记 `Not applicable`。

工作方式：

- Git 项目使用独立 Codex Worktree，并在首次写入前由生命周期 `start` 创建和登记 `feature-*` 分支；非 Git 项目使用绑定项目的 Local 环境。
- 同一项目不得与另一写入型 active 用户可见 Task 并发。
- 先读取 `AGENTS.md` 及相关事实来源。
- 保护已有修改，不扩大范围。

当前事实：

- 列出已经确认的版本、环境和阻断原因；创建者只报告真实 `threadId` 或 `SETUP_PENDING`，不得伪造 Ready。

必须阅读的项目文档：

- 列出 `AGENTS.md`、`docs/AGENT_POLICY.md` 及本目标精确命中的事实来源。

实施范围：

- 列出允许修改的模块和预期结果。

禁止事项：

- 列出不可覆盖、不可删除、不可合并、不可发布和不可声明通过的内容。

验收标准：

- 编译通过；没有可编译目标时明确标记 `Not applicable`。
- 非空测试套件通过。
- 真实最终产物或目标行为已按任务范围验证。
- 文档与验证记录同步。
- 所有改动已提交。
- `git status` 干净。

交付：
报告分支、提交哈希、实际验证、未执行项和剩余风险；不要自行合并默认/集成分支。
```

## 初始化与持久化

- 当前 schema_version 固定为 4。五项能力、post_release_action、confirmed_by、confirmed_at 均已由本次表单确认，不接受 pending，不从上游继承批准。
- 用户可明确开启/关闭左侧 Task 或自动 Task 拆分；更新 user_owned_tasks 与确认元数据并在当日 ADR 记录前后值、原因、影响和恢复条件。相同目标值幂等 no-op，不重写元数据或 ADR。
- 开关只影响之后结果边界，不迁移、中断、删除或改挂既有 Task/Worktree。关闭后仍响应显式建 Task 请求，开启后仅新结果边界自动创建。
- $desktop-switch-post-release-action 只在 local_package / push_release_branch 间切换未来动作；原子更新受保护字段，保留五项能力及初始化确认，ADR 记录本次确认与影响。其他永久策略变更同样须用户确认和 ADR。
- 旧项目升级迁移只由 $desktop-upgrade-harness 处理；旧 schema 3 若仅缺发布后动作，保留五项能力要求用户补选。进行中的旧发布/推送先恢复或核对，不自动把新动作套用旧发布。

## 开发与构建

- 日常直接实施，只运行必要相关非空测试，不自动追加计划、全仓检查、构建、E2E 或人工复核。
- parallel_worktree_subagents 只控制当前 Task 内部受管并行 Worktree，不自动创建左侧 Task；当前关闭，读取型协作不形成写入 Worktree。
- 普通 GPUI 本地试包使用 $desktop-build-gpui-release 的 local 模式，不升级为发布候选，不生成发布日志、上下文、release/、提交或 tag。
- 发布候选需 clean 已发布默认主分支/tag，运行项目全部非空单元测试，每次单独解析 e2eSelection。当前请求明确时复用；否则先读 e2e_hint 建议再询问一次，产品/渠道硬要求优先。只把本次选择写候选证据，不改本策略。
- E2E 启用时只运行完整最终候选；禁用记录 Not run、原因和风险。acceptance_smoke 的允许不替代产品批准和完整候选要求。GPUI 不使用 Tauri 插件、前端测试或 Tauri 初始化 E2E Skill。
- macOS 签名默认 disabled/not-requested 且不探测身份；仅已批准配置、当次主动要求或渠道硬要求才启用，签名、公证、stapling 完整执行，不降级。Windows 运行验收只能在真实 Windows 宿主完成。
- 安全/隐私、不可逆操作、对外契约及产品/渠道硬要求不因 disabled 跳过门禁。发布当次 reviewSelection 独立解析，必要审查强制启用；普通语义审查不混入日常开发。
