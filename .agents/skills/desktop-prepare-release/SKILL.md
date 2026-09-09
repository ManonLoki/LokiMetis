---
name: desktop-prepare-release
description: 使用下游语义化版本方案准备可追溯的 GUI 发布：提交当前范围、生成本地 release notes，再从 clean HEAD 构建候选。
---

# 准备 GUI 发布

版本边界：Harness 模板使用根 `Version.md`，按 `Asia/Shanghai` 的 `YYYYMMDDHHMM` 时间版本发布；终端下游使用根 `Cargo.toml` 的 SemVer，不得继承 Harness `Version.md`。

1. 只接受明确发布请求。读取 `docs/RELEASE.md`、`docs/changelog/README.md`、Agent Policy、产品/ADR、当前版本状态、GUI profile、签名渠道与当前活动计划（若有）。根 metadata 必须固定 `interfaces = ["gui"]`。
2. 在任何发布元数据写入、提交、测试或构建前解析本次候选选择：
   - `reviewSelection: enabled | disabled`：当前请求已明确时直接复用；安全、隐私、不可逆操作、对外兼容契约或产品/渠道硬要求强制 `enabled` 并记录来源；否则询问一次。启用时稍后形成结构化通过证据，关闭且无硬要求时记录 `reviewStatus: Not run`、非空 `reviewReason`/`reviewRemainingRisk`，且不得生成 `reviewEvidence` 或 `reviewedSourceCommit`。
   - `performanceSelection: enabled | disabled`：当前请求已明确时复用；产品/渠道硬要求强制启用，否则询问一次。关闭时形成非空公开原因和剩余风险，且不得生成探针、证据、阈值、豁免或运行时绑定字段。
   - 目标含 macOS 时解析 `macosSigningSelection`/`macosSigningSource`。来源按 `channel-required > requested > configured > not-requested` 唯一化；没有已批准配置、当次主动要求或渠道硬要求时默认 `disabled/not-requested`，形成原因/风险且不得探测本机身份、证书、公证凭据或 profile。非 macOS 为 `not-applicable`。`system_notification = enabled` 的 macOS 候选不能关闭签名，冲突必须在任何提交前停止。
   三项选择都不写入通用持久策略，也不从 E2E 推断；同一发布的修复/进程中断重跑复用原选择，新发布重新解析。E2E 选择留给构建 Skill 独立解析。
3. 要求独立 Git 根、具名 attached 分支与可解析 40 位 HEAD，核对工作区和用户修改。不得覆盖、贮藏或吸收范围外变化。用 `release_git.py inspect` 取得同时绑定 `branch` 与 `head` 的 `statusSha256`；复核后切分支、移动 HEAD 或字节变化都必须重新 inspect。实际提交前调用 `$desktop-configure-git-commits` 补齐 repo-local 身份和模板。
4. 调用 `$desktop-manage-version check --phase release`；发布准备不得另算或手工覆盖开发阶段已确定版本。完成已批准范围、相关测试和触发的权威记录。若当前周期仅含普通缺陷修复或纯重构，则不创建、不补写也不汇总 Changelog。使用 `release_git.py commit` 只提交全部且仅有已复核路径，正常运行 hooks；helper 在提交前重验分支、HEAD、状态摘要和暂存区，提交后要求同分支、直接非 merge 子提交且 committed diff 与检查快照逐字节一致，拒绝 hook 夹带。记录 clean `sourceHead`。
5. 落实本次发布审查。`reviewSelection: enabled` 时审查从上次正式发布提交（首发则仓库起点）到 `sourceHead` 的累计范围，覆盖行为正确性、core/adapter 边界、对外契约、职责/规模候选和未清理临时标记；Harness 源运行 `python3 -B scripts/validate_harness.py --release-review`。只有全部完成才记录 `reviewStatus: passed`、基线、`reviewedSourceCommit = sourceHead`、结构化 `reviewEvidence` 与无秘密摘要；发现问题必须修复、测试、重新提交后重审。`disabled` 时不得借机械提交门禁伪称已审查。
6. 从上次正式发布提交之后到 `sourceHead` 的真实差异、适用 Changelog 和缺陷事实生成 schema v2 根 `release-notes.json`；普通缺陷修复即使未触发 Changelog，也进入“问题修复”。保留最近五版、每版功能优化/问题修复各十个完整双语对，运行 `check` 与双语 `render`，发布日志不得包含远程版本服务配置。确有变化时作为独立可审查提交；无变化不创建空提交。
7. 再次要求具名分支、clean 且锁定新的 `releaseHead = HEAD`。把 `reviewSelection`/状态/适用证据或 `Not run` 风险、`performanceSelection`/来源/关闭风险和 `macosSigningSelection`/来源/关闭风险作为本次候选信封锁定。调用 `$desktop-build-tauri-release`；构建只读消费该信封，不得重新询问、翻转选择或制造证据。该 Skill 运行全量非空单元测试、构建 `pending` 候选并按当次 E2E 选择进入验收。
8. 源码或配置修复会使旧候选、审查和运行证据失效；在原发布授权范围内修复、测试、重新提交、刷新发布日志，并从新 clean HEAD 重审和重建。性能失败只有用户查看保留指标后才能明确 `waived`，不能改成 `Not run` 或 `passed`。
9. 只有 `$desktop-verify-delivery` 已把完整原子候选集合标记为 `Milestone accepted`，才进入纯只读就绪复核；否则保持 `pending`/`rejected`。就绪复核不回写 tracked Product Status、Work Plan 或 Verification，也不调用 `$desktop-manage-version finalize-release`。只有真实渠道发布成功后，发布执行方才以精确已发布版本和源码提交调用该命令并独立触发后续受管记录。发布准备不授权 tag、push、上传、商店提交或外部发布，除非用户另行明确授权。

输出版本、sourceHead/releaseHead、实际提交、release-notes 摘要、审查/性能/macOS 签名/E2E 选择、真实候选状态与未通过门禁；不得把目录存在或 `pending` 候选称为发布就绪。
