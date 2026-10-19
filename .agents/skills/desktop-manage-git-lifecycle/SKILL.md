---
name: desktop-manage-git-lifecycle
description: 在 Harness 源或终端下游 Git 项目中登记开发分支、按明确请求推送，以及在本地主分支合并并创建 tag 完成发布；发布后推送是独立操作。
---

# 管理 Git 生命周期

使用 Node.js `>=24.21.0` 自包含标准库 helper 管理 Git 资源。新功能、问题修复或其他会写入仓库的开发工作开始前运行 `start`；发布前明确要求推送时运行 `publish`；用户要求“发布”时运行 `release`。下游本地发布开始时把已确认的 `post_release_action` 冻结为该次 `postReleaseAction`；发布完成后，冻结的 `push_release_branch` 触发 `push-release`。Harness 源保留 `null`，只在用户当次明确要求推送且已发布提交能证明 Harness 身份时调用同一 `push-release`；推送不属于 Git 发布完成门禁。`local_package` 选择由适用的本地打包 Skill 执行。

## 命令

从项目根运行：

```text
node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs inspect --project-root . [--remote <name>]
node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs start --project-root . --summary <ascii-kebab> [--remote <name>]
node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs track-worktree --project-root . --worktree <absolute-path> [--remote <name>]
node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs publish --project-root . [--remote <name>] [--also-remote <name>]...
node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs release --project-root . --version <version-without-v> --release-context-sha256 <sha256> [--date YYYYMMDD]
node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs push-release --project-root . --remote <name>
node .agents/skills/desktop-manage-git-lifecycle/scripts/git_lifecycle.mjs check-post-release --project-root . --action <local_package|push_release_branch>
```

`start` 创建并切换到 `feature-<summary>-<Asia/Shanghai YYYYMMDD>`。本地同名分支存在时依次尝试 `-2`、`-3`；当前已经位于本周期登记的同摘要分支时返回幂等结果。上一次发布后的首个新周期必须先复核本地默认主分支与 tag 都精确指向已发布 HEAD，并从该 HEAD 建新分支；即使从旧 Task Worktree 发起，也不以它的陈旧 HEAD 为基线。此前尚无发布时从当前 HEAD 开始。当前 Worktree 即使处于 detached HEAD 也可直接开始；若它不是主 Worktree，helper 会在创建分支后把规范化精确路径和新分支一起登记。创建本地分支不要求配置远端。

`publish` 的 `--remote` 与生命周期 `state.remote` 表示发布前推送的唯一主远端。选择时优先沿用本周期已经登记的精确名称；显式传入不同名称会以 `remote-conflict` 失败。没有登记值时，`publish` 才依次选择 `origin`、唯一已配置远端，仍有歧义时要求显式指定。`release` 不接收远端或本地模式参数，也不解析、登记或访问远端。`push_release_branch` 是未来发布开始时冻结远端路线的持久授权；调用 `push-release` 前从唯一已配置远端确定具名 `--remote`，多个远端时向用户确认精确目标。需要多个远端时，用户须分别授权并逐次调用。流程不创建远端、不填写地址，也不处理凭据。

发布成功而新开发周期尚未开始时，`publish` 必须拒绝执行并提示 `push-release`，防止它只移动已发布主分支而漏推 tag。新周期开始后，`publish` 继续处理该周期发布前的分支推送。

只有 `publish` 接受可重复的 `--also-remote <name>`。每个补充远端都必须来自用户对本次推送的明确授权；不得因为仓库已经配置、名称看似常见或主远端冲突而自动加入。helper 在首个 push 前解析全部远端和各自 advertised default branch，任一名称不存在、默认分支不可解析、与主远端重复或补充项重复时零 push 失败。补充远端只在未完成 `publish` 的 `pendingPublish` 中临时保存冻结目标与确认进度，成功即清除；它们不改变 `state.remote`，不参与 fetch、merge、release、tag 或清理，也不授权创建/配置远端或凭据。

`track-worktree` 用于后来显式创建的内部单元 Worktree；它自行读取指定 Worktree 的具名分支，要求绝对路径和相同 Git common-dir，并把精确路径与分支加入本周期清单。主 Worktree、默认分支、分离 HEAD、其他仓库或发生所有权冲突的登记都会失败。

`publish` 要求可解析主远端，并在任何主分支写入前确认全部登记分支仍存在、全部登记 Worktree 都没有未提交数据。即使命令从关联 Task Worktree 发起，它也会依据同一 Git common-dir 自动转到主 Worktree 执行后续操作。它只从主远端获取并普通合并其默认分支的当前提交，再按创建顺序对尚未合入的登记分支执行普通 `git merge --no-edit -m <中文合并消息>`，保持主 Worktree 切换在该默认分支。合并完成后先把同一最终 HEAD、全部目标顺序和初始确认进度原子写入 `pendingPublish`，再向主远端的 advertised default branch 非强制 push 并复读，随后按参数顺序把该 HEAD 非强制 push 到每个补充远端各自的 advertised default branch 并逐个复读；每项确认后立即保存进度，全部确认才清除 journal。补充远端的不同默认分支名不会改变本地主分支或触发 fetch/merge。登记分支缺失、脏工作区、合并冲突或任一远端结果无法确认都会停止，绝不静默漏掉数据。它不创建标签，也不清理任何资源。

跨远端推送不是原子操作。主远端或较早补充远端成功、后续补充远端失败时，错误必须如实说明可能已经成功的前序范围、当前失败目标或阶段，以及后续目标可能尚未尝试，不能回滚或把整次调用宣称为零写入；用户可使用相同目标参数进行幂等重试。重试只复读已冻结目标并继续尚未确认的目标，不重新解析默认分支、fetch、merge 或计算新 HEAD；已确认目标若漂移则停止。Git push 非零退出只能判为结果不确定，除非远端复读已精确命中冻结 HEAD。仅含主目标的 `publish` 为兼容既有机器调用保留历史稳定 code `push-rejected`；该标识不得被解释为服务端已确定拒绝，实际结果仍按远端复读判定。

`release` 必须接收 `--release-context-sha256 <sha256>`。下游在首个发布状态写入前检查合法策略 schema v4/v5 与正文并冻结当前动作，重试只能沿用 `pendingRelease.postReleaseAction`；Harness 源保留 `null`。tracked `.harness/release-context.json` 是版本、日期、预期 tag、`sourceHead`、本地默认主分支及审查结果的唯一冻结上下文，不含推送或打包选择。命令从关联 Task Worktree 发起时，helper 先在该调用 Worktree 校验当前 HEAD 的上下文 blob 与工作树字节，再按同一 Git common-dir 路由到主 Worktree。Harness 源还须在任何合并或状态写入前读取 Git common-dir 中的受管当前分钟取号凭证，核对 `Version.md` 精确字节和时间版本；手工改写时间版本或自造上下文不能代替取号。任何合并或创建 tag 前必须核对 version/date/expectedTag/defaultBranch 与 CLI 和生命周期状态一致；不匹配则零发布副作用失败。整合得到 final HEAD 后、冻结 `pendingRelease.head` 之前，必须再次确认该 HEAD 包含同一上下文摘要。

`release` 只在本地默认主分支按登记顺序普通合并本周期开发分支，切换到该分支并冻结 final HEAD，然后创建或复读本地轻量标签 `v{version}-{YYYYMMDD}`。`--version` 在任何状态写入前必须是 Harness 12 位时间版本或 Minor/Patch 位于 `0..99` 的稳定三段版本；下游新发布还必须高于 `lastRelease.version`，不能以新日期 tag 重发同一或更低版本。冻结 final HEAD 前，该 HEAD 的 `release-notes.json` 必须通过更新日志 schema 且首条版本等于本次版本。只有主分支 HEAD、本地 tag 和 tracked 上下文精确复核通过才返回 `released`。全过程不列举、fetch、push、复读或删除远端 ref，不打包，也不删除登记分支或 Worktree。中断重试沿用固定版本、上下文摘要与已冻结 HEAD；同名 tag 指向其他提交时失败。发布成功后把本周期精确资源清单保存于 `releasedResources`，重置活动周期；旧资源不会在下一周期被重新合并或自动清理。

`push-release --remote <name>` 是发布后的独立步骤：下游必须有最近一次发布冻结的 `lastRelease.postReleaseAction = push_release_branch`；Harness 源仅凭本次用户授权，并从已发布提交复核 12 位时间版本、`Version.md` 和活动实例化入口后，允许 `postReleaseAction = null`。未绑定的旧下游发布及冻结的 `local_package` 均在本地 ref 或远端变化前拒绝。它只读取最近一次已完成发布记录中冻结的 HEAD 与 tag，确认本地主分支及 tag 仍一致，每次调用预读指定远端 advertised 默认主分支、小写 `release` 分支和同名 tag，要求唯一 symbolic HEAD 与默认主分支 OID 一致；默认主分支缺失、无法解析时零 push 失败；同名远端 tag 冲突，或本地/远端存在 `Release` 等大小写变体分支时零 push 失败，防止大小写不敏感文件系统把旧大写分支当作 `release` 复用。再创建或安全推进本地 `refs/heads/release`：已有 ref 只有指向本生命周期精确记录的旧发布 HEAD、且是本次 HEAD 祖先、没有被任何 Worktree 检出时才可移动；异源 ref 失败关闭。随后依次非强制推送该 HEAD 到远端 advertised 默认主分支、远端 `refs/heads/release` 和同一 tag，每步均复读确认再继续。此命令同时更新远端 advertised 默认主分支，不在 tag 后新增提交；本地和远端 `release` 分支直接指向已发布 HEAD。远端拒绝或复读失败时如实报告已确认的默认主分支/release、尚未尝试的后续引用或不确定结果；本地 `Released` 不回滚，重试无需 fetch、merge 或重算 HEAD；没有跨调用的目标 journal，每次调用重新解析具名远端默认主分支并在返回前核对名称未漂移。`push-release` 一次只接收一个 `--remote`，不接收 `--also-remote`。

`push-release` 成功返回前还须重新核对本地 `refs/heads/release`、远端默认主分支名称未在本次调用中变化，远端默认主分支、`release` 分支和 tag 仍指向冻结 HEAD；任一漂移报告不确定，保留本地 `Released`。远端名先通过严格语法校验，不能把选项形态的名称传给 Git 子进程。

`check-post-release --project-root . --action local_package|push_release_branch` 是只读门禁：检查最近发布冻结的动作、主分支 HEAD 与 tag；旧发布没有动作快照时失败。选定本地打包路径在运行候选构建前必须先通过此检查；独立构建请求不受该选择限制。Git 发布本身不创建 `Release` 或其他中转分支；发布后的远端路线使用精确小写 `release` 分支。发布合并不要求线性历史或快进，也不设置租约、原子推送或保护分支门禁；远端 `release` 分支仍按非强制 push 拒绝非快进更新。普通合并冲突、脏数据保护和标签身份复核始终是 Git 安全检查。发布不因未配置远端而阻断；此处也不执行资源删除。后续若需清理，必须另行明确授权，并依据 `releasedResources` 的精确身份先核对，不能扫描名称前缀或强制删除其他工作。

合并消息固定为中文“合并开发分支 <branch>”或“合并远端默认分支 <remote>/<branch>”，显式传 `-m`；不改变普通合并、父提交关系、冲突与恢复机制，快进不生成额外提交。

## 状态与输出

已完成的 v2 发布迁移必须复读其 tag 所指提交中的 `.harness/release-context.json` blob，并要求 SHA-256 等于旧记录的 `releaseContextSha256`。缺少该 blob 或摘要冲突时失败关闭，保留旧状态文件供人工核对。

状态只保存在 Git common-dir 下的 `agent-first-harness/git-lifecycle.json`，不会写入项目受跟踪目录。当前 `schemaVersion: 4`；v1 不兼容。没有进行中发布或推送的 v2 状态可安全迁移：既有合法 v2 状态缺少可空 `pendingPublish` 时先按 `null` 读取，迁移已完成发布前还须核对本地 tag 与主分支；v2 的 `pendingRelease` 或 `pendingPublish` 非空时拒绝自动改释，保留原字节供人工恢复。v4 记录适用的发布前推送主远端、本地默认主分支、本周期精确分支和 Worktree、待完成发布、最近一次成功发布的 `lastRelease`，以及历次 `releasedResources` 精确资源快照。`pendingRelease` 与 `lastRelease` 除 tag、版本、日期、默认主分支、HEAD 与 `releaseContextSha256` 外还保存当次 `postReleaseAction`，仅可为 `local_package`、`push_release_branch` 或 Harness 源/旧发布的 `null`；旧 v3 静止发布安全迁移为 `null`，不得用新策略追认旧发布。v3 若仍有 `pendingRelease` 或 `pendingPublish`，失败关闭且保持原状态字节，须先按旧流程恢复或人工核对。`release` 在合并/tag 前落盘 pending 的摘要、tag/日期/版本，HEAD 尚未冻结时允许为 `null`；冻结后仅沿用同一 HEAD。`publish` 在首个 push 前以 `pendingPublish` 保存冻结 HEAD、有序目标和逐项确认进度，全部确认才清除。所有会改变分支、状态或远端的命令使用同一 common-dir 短时互斥；`inspect` 只读取状态，不迁移落盘。

所有正常结果和错误都是单行 JSON。错误返回非零退出码及稳定 `code`，消息不包含 Git 标准错误、远端 URL 或凭据。Git 子进程固定为非交互模式。
