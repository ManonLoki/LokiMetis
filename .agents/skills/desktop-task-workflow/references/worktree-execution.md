# Worktree 环境与内部并行

本引用统一左侧 Task 的环境与当前结果内部的 Subagent Worktree。二者身份不同：前者由 `create_thread` 建立、使用项目 Task 编号；后者仅由内部协作工具创建，不调用 `create_thread`、不显示 Task 标题、不占编号。

## 左侧 Task 环境

Git Task 仅在 `task_worktrees: enabled` 时使用精确项目的 `environment.type=worktree`；关闭时使用 `environment.type=local`。非 Git 固定 Local。自动建 Task 关闭时直接在当前 checkout 工作，不能为创建 Worktree 隐式打开 `user_owned_tasks`。

Worktree 模式使用用户明确指定的已提交起点；未指定时取保存项目默认主分支的已提交 HEAD，不硬编码 `main/master`，不主动 fetch/pull。有前序结果时使用精确结果 SHA，不回到尚未整合的旧默认分支。派发记录 cwd、common-dir、registry、branch/HEAD 和输入 SHA；新环境必须 clean。Codex Worktree 可能 detached，产品写入前由 `$desktop-manage-git-lifecycle start --summary <feature-summary>` 建立并登记当前分支/Worktree。

Local 模式 cwd 精确等于保存项目规范化路径，Git 顶层、common-dir 与 primary registry 项一致。Local 没有 `startingState` 参数；使用冻结的实际 branch/HEAD，显式输入不符时先由已授权协调者完成安全交接，不自动 stash、丢弃或复制用户修改。同一 Local cwd 的业务 Task 串行：前序结束并交出可核对的提交后再开工，只读测试期间其他 Task 也不能改变被测 branch、HEAD 或文件。恢复漂移先复核实际输入，不把当前代码冒充原结果。

独立 Worktree 可在写入所有权不重叠、输入和依赖明确时并行，不设每项目一个 active 写入 Task 的限制。同一文件、目录祖先/后代或共享生成物存在重叠时，仅串行处理对应部分；不把局部冲突扩成整个项目禁止并行。

先一次解析项目和预留整批编号，再并行调用可独立创建的 Task/Worktree 工具、并行核对 ready 结果；依赖前序提交的项目只等待其所需输入。用户可见 Task 仍由受支持的 `create_thread` 建立，普通 `create_worktree`、`git worktree add` 或内部 Subagent 不能冒充其左侧身份。

## 内部单元的授权与准备

`parallel_worktree_subagents: enabled` 加当前请求已经明确授权并行，才创建内部写入单元。它与 `user_owned_tasks`、`task_worktrees` 独立：父结果可在合法 Local，也可在左侧 Task Worktree；内部单元不触发左侧子树。只有单 Agent 更适合的重叠部分串行，其余独立部分继续并行。

给每个单元定义 ASCII task/unit 技术标识、结果、一个或多个非空 repo-relative 写入所有权、依赖、必要检查与整合顺序。task 标识使用父结果的 `feature-summary`，不从 Task 显示标题生成。拒绝 `.`、绝对/越界路径、符号链接逃逸、同单元冗余祖先/后代和活动单元间重叠所有权。

`--project-root` 是保存项目 primary checkout；`--source-worktree` 是当前结果实际 Git 顶层，Local 时可相同。二者必须拥有相同规范化 common-dir，source 位于 registry 且附着具名生命周期分支；基线已提交、clean。单元分支为 `codex/unit-<task>-<unit>`，source 分支取 registry 真实值，不根据前缀或历史形态猜测。所有非 `inspect` 命令同时提供两个根路径。

## 创建与短锁

helper 保留原路径：`.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs`。从 source 的精确 cwd 为每个单元执行，多个所有权重复传 `--write-target`：

```text
node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs create --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit> --write-target <owned-path> [--write-target <owned-path> ...]
```

对无依赖单元并行启动 `create`，同步等待整批结果。共享短锁仅保护所有权预留/状态发布；基于冻结 SHA 的 `git worktree add` 在锁外进行。锁竞争自动有界等待，不能把正常并发转为人工审批；真实超时或不安全锁才报告稳定错误。Git 生命周期共用状态写入继续短时互斥，不把 checkout、业务操作、测试或等待子 Agent 放进项目长锁。

helper 自动登记精确 Worktree/branch 到 Git 生命周期。失败后复读精确登记；只有确定未登记才回收本次新建资源和自己的预留。已经登记或无法确认时保留 Worktree、分支与预留，尽力写入 `failed` 状态并报告恢复证据。项目/Task 共享容器始终保留，不能因容器一时为空就删除它，避免与正在创建的其他单元竞态。不覆盖其他单元，不自动暂存、stash 或提交用户修改。登记失败返回 `lifecycle_worktree_tracking_failed`；回滚不完整保留精确证据，不伪装成功。`creating`、`failed` 或遗留预留不得作为 ready 单元进入 guard、整合或收口。

## 单元执行与自主检查

把返回的精确 `worktreePath`、所有权、原授权和禁止范围交给对应内部 Subagent，明确它不是独占代码库。每个单元先从精确 cwd 自动执行 guard，通过后在授权范围内直接实施：

```text
node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs guard --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit> --write-target <intended-path> [--write-target <intended-path> ...]
```

guard 检查实际修改与已登记 ownership，不能替代宿主沙箱，也不是人工批准。新目标超出登记范围时先重新分配相关所有权；普通同范围编辑不重复完整创建流程。本地提交前复核实际差异、真实消息及必要开发检查，通过后自主提交，不逐次要求父方或用户批准。

完成后、整合前执行 postflight：

```text
node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs verify --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit>
```

postflight 核对登记 `baseHead` 至 HEAD 的 committed 路径，以及 staged、unstaged、untracked 和 rename 源/目标均在 ownership。越界由所属单元修复；不会因用户批准变为通过。单元交付变更文件、提交、实际开发检查、阻断项和整合说明。

## 整合与保留

协调方同步等待必需结果，按依赖顺序检查范围、差异和实际证据，使用已授权、可逆的普通 Git 操作整合提交；语义冲突由负责该部分的 Agent 解决，不让多个 Agent 竞态修改共享文件。不要求快进、祖先链、冻结 OID 或其他分支历史形态。仅运行本次必要回归，不因并行追加全仓检查、构建、冒烟、E2E 或正式验收。

从保存项目 primary cwd 读取单元状态：

```text
node .agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs inspect --project-root <absolute-project-root>
```

postflight 通过且 clean 的精确单元可从 source cwd 收口：

```text
node <absolute-project-root>/.agents/skills/desktop-run-parallel-worktrees/scripts/parallel_worktrees.mjs remove --project-root <absolute-project-root> --source-worktree <absolute-source-worktree> --task <task> --unit <unit>
```

`remove` 只移除本工作流单元状态，返回 `resourcesRetained: true`、`worktreeRetained: true`、`branchRetained: true`；已登记的 Worktree/branch 继续由生命周期保留。它不删除其他 Task 资源，也不因发布、推送或整合完成自动删除分支/Worktree。后续清理须有独立明确请求并复核精确资源身份。

用户已授权的发布、推送按原授权和专用入口执行；内部并行本身不授权额外外部副作用。单元完成不冒充正式验收、Git 发布或候选就绪。
