# 左侧 Task 合同

本引用只定义用户可见 Task 的创建、身份、进度和恢复。是否自动创建取项目 `user_owned_tasks`；环境取 `task_worktrees`，其值由 Agent Policy 保存。关闭自动 Task 时普通工作在当前聊天执行，无需先补建 Task。明确的新建请求只授权该结果。

## 标题与编号

唯一标题为 `Task {序号} | {当前进度} | {单一结果}`。序号是无前导零的正整数；分隔符两侧各一个 ASCII 空格；单一结果非空、单行、首尾无空白且不含 `|`。例如 `Task 8 | 运行中 | 左侧 Task 默认关闭并支持开关`。`threadId`、`clientThreadId`、内部 `task_name` 和 Git ref 都不能生成或反填编号。

每批创建先用 `list_projects` 按规范化完整路径确定保存项目、精确 `projectId`、宿主 `hostId` 和 Git 状态；再用 `list_threads(limit=50)` 清点当前及 pinned 记录，并沿同宿主 `list_archived_threads` 的 `nextCursor` 读完归档。仅 `kind=codex`、宿主/项目精确匹配且满足本标题合同的记录有效。取最大有效序号加一；空历史从 1，缺号不回填，不识别任何历史标题格式。内部 Subagent、其他项目/宿主和不合规标题不占项目序列。

一批独立结果一次预留连续编号，然后并行派发；不逐个等待前一 Task 完成才创建下一项。其他协调者可能同时派发时，先明确本批编号所有者，避免竞态重复分配。历史无法可靠枚举时保留已有结果和编号，只暂停需要新编号的派发，不阻止已合法绑定 Task 的工作。

## 自动完成创建与绑定

创建者准备 [描述模板](task-description.md) 的真实范围、基线和授权后，对每个结果只调用一次 `create_thread`，显式传入 `title="Task {序号} | 已分配 | {单一结果}"`、`target.type=project` 与精确项目。Task 位于所属项目且非 pinned；不能用 projectless 或内部 Subagent 冒充左侧 Task。

创建和绑定是自动技术检查，不要求人工确认 RESOLVED、DISPATCHED、VERIFIED 或 BOUND。按以下顺序完成：

1. 解析项目、编号、结果、允许写入路径和环境选择；无依赖的分配请求并行发出。
2. 保留返回的 `hostId`、真实 `threadId` 或 `clientThreadId` 以及派发前稳定 Task key。只有 `clientThreadId` 时报告 `SETUP_PENDING`；不能把它传给需要 `threadId` 的工具或重复创建原结果。
3. 取得真实 `threadId` 后，用 `list_threads` 对账精确项目、非 pinned、标题、cwd 和 ready 状态。目标已合法进入运行中不构成错误。
4. Git 环境确认保存项目和实际 cwd 的规范化 Git common-dir 相同，当前顶层在 `git worktree list --porcelain` 中、起始 HEAD 正确且新环境 clean；非 Git cwd 精确等于保存项目。Worktree/Local 细节见 [工作树执行](worktree-execution.md)。检查通过直接开工，不等待新的审批口令。

每次开工、恢复或身份/环境改变时核对一次真实绑定，不在每次资料阅读、普通编辑或已授权提交前重复完整创建流程。Task 自己已有未提交修改时按原所有权继续，不把自己产生的 dirty 状态误判为新建环境失败；先保护与区分用户原有修改。新建环境基线和其他任务资源仍必须精确核对。

## 进度与完成

进度只取 `已分配`、`运行中`、`检查中`、`已完成`。派发后已分配；绑定通过并开始工作进入运行中；真实进入本次必要检查时进入检查中；同范围返工回运行中；结果、必要开发检查及适用提交完成后才已完成。

每次真实转换最多调用一次 `set_thread_title`，再用真实 `threadId` 的 `list_threads` 有界复读；编号与单一结果保持不变。标题更新失败如实报告最后确认状态，不无限重试、不重建 Task、不推翻已有真实结果。检查结果不能因人工批准改成通过，开发回归不能代替正式验收。

写入型 Git Task 完成时确认自己的范围内变更已提交、结果可核对；同一 Worktree 中仍存在用户修改时保留并说明，不能为追求 clean 撤销它们。只读结果的提交/编译为 `Not applicable`，无候选时不自动构建或 E2E。

## 原 Task 恢复

恢复原结果沿用 Task key、编号、结果和派发时冻结环境，不因开关切换迁移。先使用受支持的 `move_thread_to_project`、`move_thread_to_sidebar_section` 或 `handoff_thread` 修复真实归属、置顶和 cwd，再有界复读；工具成功回执不能代替复读。handoff 改变真实 ID 时保留工具证明的迁移链，不凭标题相似接管。

已有授权允许修复绑定时直接执行，不重新索要同一批准。pending → 真实线程必须有宿主映射或用户提供并复读一致的真实 `threadId`；无可靠映射只暂停该待分配结果，保留原 Task 和证据，其他已绑定且无依赖的任务继续。真实不安全的项目/基线冲突先修复；无法修复时明确报告所缺事实，不新建替代 Task 碰运气。
