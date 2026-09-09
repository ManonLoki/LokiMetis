---
name: desktop-extract-i18n-strings
description: 扫描已选 GUI 适配器的 React 前端与 Rust 原生文案，把硬编码用户可见字符串抽取为 i18next/react-i18next 与 rust-i18n 的翻译资源键，不改变默认语言下的可观察行为。仅在下游已通过 `$desktop-add-gui-adapter` 建立 i18n 技术栈后使用。
---

# 抽取硬编码文案为 i18n 配置

把 GUI 适配器中硬编码的用户可见文案迁移为 ADR-20260806-001 固定的 i18n 技术栈键值，不新建并行的国际化机制，也不触碰 core。

## 工作流程

1. 确认当前项目已选择 GUI 适配器且已接入 `i18next`/`react-i18next`（前端）与 `rust-i18n`（Rust 原生文案），可在 `.agents/skills/desktop-add-gui-adapter/references/gui-baseline.md` 与 `references/react-frontend-baseline.md` 核对固定技术栈；未接入时先完成基础接入，不在中途新建替代机制。
2. 直接实施不改变默认语言下可观察行为的纯文案迁移；若同时新增语言切换入口或修改默认语言探测逻辑，先确认产品边界，只在形成长期重要决定或硬规则例外时记录 ADR。
3. 扫描范围限定在 `<project-id>_gui` 前端 `src/`（JSX/TSX 文本节点、`label`/`placeholder`/`title`/`aria-label`/通知内容等字符串属性）与 Rust GUI 适配器层（托盘菜单、窗口标题、系统通知文案）。绝不扫描或修改共享 core；core 中出现的用户可见字符串是 core-first 边界问题，转 `$desktop-refactor-code` 评估，不在本 Skill 内直接改写业务代码所在 crate。
4. 排除非用户可见字符串：标识符、CSS 类名、`tracing`/日志文案、内部错误码、测试夹具、代码注释和开发调试专用文本；只处理真实渲染给最终用户的文案。
5. 为每处硬编码文案生成或复用稳定的层级翻译 key（如 `settings.language.label`），前端调用 `react-i18next` 的 `useTranslation`/`t()`，Rust 侧使用 `rust_i18n::t!()` 宏；翻译资源按功能域拆分文件存放，不得把 key 直接设为源文案本身的复制。
6. 只为基准回退语言（英文）和当前已批准维护的语言写入真实翻译；不得为未批准或未来语言臆造译文。缺失的其他已支持语言译文按 `docs/ENGINEERING_RULES.md` 第 3.3 节关于临时标记的规则记录明确原因和完成条件，并同步登记 `docs/TECH_DEBT.md`，不得裸写未说明原因的临时标记。
7. 替换源码中的字符串字面量为 i18n 调用后，运行前端和 Rust 相关测试确认默认语言渲染结果与迁移前一致；新增 key 需有测试或人工核对证据，不得只改资源文件不验证渲染。
8. 仅对本次触及文件执行适用硬门禁：前端源码超过 1000 行、翻译资源等其他人工维护文本超过 2000 行必须按真实职责拆分。501–1000 行前端和 501–2000 行其他文本的职责复核只在明确发布且当次 `reviewSelection: enabled` 时集中执行；日常文案迁移不自行增加软阈值审查，也不自动运行全仓文件行数门禁。
9. 只按 `docs/ENGINEERING_RULES.md` 的独立事件触发规则更新项目记忆；纯文案迁移属于不改变可观察行为的重构，默认不触发 Product Spec、ADR、Product Status 或 Changelog。

## 硬边界

- 不得新建第二套国际化机制或直接拼接语言判断分支替代 `i18next`/`react-i18next`/`rust-i18n`。
- 不得触碰共享 core；发现 core 内的硬编码用户可见文案时停止并转交 `$desktop-refactor-code` 评估 core-first 边界，不在本 Skill 内跨边界改写。
- 不得臆造未批准语言的翻译内容；缺失译文必须显式记录而不是留空或用机器翻译冒充已确认文案。
- 不得为了消除硬编码字符串而改变默认语言下的实际展示文案含义；迁移前后语义必须一致。
- 迁移不得引入新的公开命令、路由或协议变化；纯文案位置迁移不视为对外契约变化。

## 完成输出

报告扫描范围、抽取的字符串数量与对应翻译 key、新增或修改的资源文件、跳过的字符串及原因、已记录的缺失译文技术债、运行过的测试与检查，以及默认语言渲染结果的验证方式。
