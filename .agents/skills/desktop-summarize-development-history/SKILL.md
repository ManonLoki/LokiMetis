---
name: desktop-summarize-development-history
description: 按用户请求只读追溯当前项目的 ADR、Changelog、产品规格、状态、计划与 Git 历史，解释开发脉络、现状和有证据的未来方向；不整理或改写项目记忆。
---

# 汇总开发历史

在用户要求回顾开发历程、决策沿革、版本变化、产品来龙去脉或未来方向时使用。本 Skill 只读分析，不因日常开发、构建、升级或发布自动运行。Harness 源只分析 Harness 自身工程演进；具体产品必须在唯一终端下游项目根目录分析。

## 取证范围

1. 确认当前项目的唯一根目录；有 Git 时再确认 Git 根、分支与 HEAD。优先使用用户指定的时间、版本、主题或模块范围。用户未限定时，有 Git 的项目以当前分支可达历史为范围；其他分支、远端或外部资料只有用户明确要求才纳入，并分别标明范围。工作树未提交内容可以作为当前状态的补充，但必须与已提交历史分开。非 Git 项目只依据当前仍存在的本地资料，并明确历史追溯深度受限。
2. 按问题读取存在的 `docs/adr/README.md`、`docs/changelog/README.md`、`docs/product_spec/README.md`、`docs/project_status/README.md`、`docs/work_plan/README.md` 及各自最新文件；只读实际存在的资料。项目记忆按日只保留最新文件；有 Git 且需追溯旧日期时，按相关路径和时间范围使用 `git log`、`git show`，不得把当前快照当成完整历史。Harness 若有 `ADR_history.md` 或 `CHANGELOG_history.md`，可作为过期条目原文的辅助证据，仍须核对后续取代决定，并在 Git 可用时核对提交历史；下游不要求有这两份文件。
3. 依次核对相关 ADR 的状态、取代范围与恢复条件，Changelog 的真实变化、Product Spec 的已批准边界、Product Status 的当前状态、活动 Work Plan 的未完成步骤及 `docs/TECH_DEBT.md` 的仍开放限制。需要判断实际完成或验证时，再查对应提交、测试或 Verification 证据；计划、占位和中性脚手架不能证明已交付。Changelog 不收录普通缺陷修复，缺少条目不能证明没有修复。

## 分析与呈现

- 按时间给出起点和关键转折，解释“当时为什么决定、后来哪些内容被取代、现在实际适用什么”。每项关键结论标注可定位的 ADR ID、Changelog 日期、文件路径和/或提交哈希；对通过 Git 找回的已删除旧记录注明 `git show` 的提交与路径。
- 分开陈述已决定、已实现、已验证、仍在计划、待决和推测。未来预期只能从最新已批准 Product Spec、当前状态/活动计划、ADR 调整条件与开放技术债推导；推测必须写成推测，不替用户批准路线或承诺时间。
- 指出资料互相矛盾、历史缺口和未验证平台；无法证实的内容写“无记录”或“无法判定”。按用户问题控制篇幅，并给出足以复核结论的证据位置。
- 默认只在回复中汇总。不调用 `$desktop-curate-harness-memory`，不改写 Product Spec、ADR、Changelog、Status、Plan、Verification 或版本状态。用户明确要求保存时，先确定独立报告位置；报告只引用权威资料，不成为新的权威产品记忆。
