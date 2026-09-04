---
name: desktop-prepare-release
description: 使用下游语义化版本方案准备可追溯的 GUI 发布：提交当前范围、生成本地 release notes，再从 clean HEAD 构建候选。
---

# 准备 GUI 发布

版本边界：Harness 模板使用根 `Version.md`，按 `Asia/Shanghai` 的 `YYYYMMDDHHMM` 时间版本发布；终端下游使用根 `Cargo.toml` 的 SemVer，不得继承 Harness `Version.md`。

1. 只接受明确发布请求。读取 `docs/RELEASE.md`、`docs/changelog/README.md`、Agent Policy、产品/ADR、当前版本状态、GUI profile、签名渠道与当前活动计划（若有）。根 metadata 必须固定 `interfaces = ["gui"]`。
2. 在任何提交或构建前解析本次 GUI 性能选择；产品/渠道硬要求优先，否则用户未明确时询问一次。E2E 选择留给构建 Skill 按同样规则解析。
3. 要求独立 Git 根，核对分支、HEAD、工作区与用户修改。不得覆盖、贮藏或吸收范围外变化。实际提交前调用 `$desktop-configure-git-commits` 补齐 repo-local 身份和模板。
4. 调用 `$desktop-manage-version check --phase release`；完成已批准范围、相关测试和触发的权威记录。若当前周期仅含普通缺陷修复或纯重构，则不创建、不补写也不汇总 Changelog。创建可审查本地提交，记录新的 clean `releaseHead`。
5. 从版本与当期 Changelog 生成 schema v2 根 `release-notes.json`，保留最近五版、每版功能优化/问题修复各十个完整双语对；运行生成器自检后单独提交。发布日志不得包含远程版本服务配置。
6. 再次要求 clean 且 HEAD 等于新的 `releaseHead`，然后调用 `$desktop-build-tauri-release`，传入本次性能选择。该 Skill 运行全量非空单元测试、构建 pending 候选并按当次 E2E 选择进入验收。
7. 源码或配置修复会使旧候选失效；在原发布授权范围内修复、测试、重新提交、刷新发布日志并从新 clean HEAD 重建。性能失败只有用户查看保留指标后才能明确 waiver。
8. 只有候选实际通过要求的验收和人工门禁，才由 `$desktop-manage-version` 完成正式发布周期；否则保持 pending/rejected。发布准备不授权 tag、push、上传、商店提交或外部发布。

输出版本、releaseHead、两个本地提交、release-notes 摘要、构建/性能/E2E 选择与真实候选状态；未通过的门禁明确保留风险。
