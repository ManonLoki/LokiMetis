---
name: desktop-review-core-boundaries
description: 按用户请求或已启用的发布审查，只读检查真实操作的 core、adapter 与驱动归属、依赖方向和测试证据；不自动修复或追加全仓治理。
---

# 审查 core 与接口边界

读取 [工程规则](../../../docs/ENGINEERING_RULES.md) 的 core-first、依赖、测试与机械检查章节，以及 [Rust 基线](../../../docs/RUST_CLI_TEMPLATE.md) 的职责归属。它们继续是硬规则；本专项审查是否触发，不改变日常实现必须遵守这些规则。

## 执行

1. 固定当前项目根、源码提交/工作区差异和审查范围。默认只读；已初始化下游或 Harness 工程分别遵守自己的边界，不猜测未提供的产品语义。
2. 从真实公开操作追踪“适配器操作 → core 用例 API → core 测试”，检查值域、跨字段关系、权限、幂等性、默认值、稳定错误和状态转换的归属；同时注明协议、展示、设备偏好与具体 I/O 驱动的接口/宿主专属性。单一接口也适用，不按代码行数判断薄层，不为假想消费者要求抽象。
3. 需要机械依赖证据时复用 [原检查器](../desktop-implement-change/scripts/check_core_first.mjs)，不复制实现。它只识别一个 `_core` 与同前缀 `_cli/_tui/_mcp/_gui` 的角色，命名或角色不符不能宣称通用架构通过。
4. 严格只读时使用已提供且与源码绑定的 Cargo metadata 快照：`node .agents/skills/desktop-implement-change/scripts/check_core_first.mjs --metadata-file <path>`。未绑定快照明确限制。`--workspace-root <root>` 路线会调用 Cargo metadata，可能联网或生成解析文件，只有当前上下文允许该工具副作用时使用；tracked 锁策略才带 `--locked`，不得宣称默认离线。
5. 图检查只证明确定的依赖方向，不能证明业务语义归属或测试已执行。核对已有真实测试结果，指出核心成功/最高风险失败覆盖缺口；不自动运行全仓测试、构建或 E2E。
6. 按风险报告具体位置、触发情形、影响和最小修复方向。发现问题只报告；修复转 `$desktop-implement-change` 或 `$desktop-refactor-code`，允许的硬规则例外另走已确认 ADR，不能由审查批准。

## 输出

报告源码范围、调用映射、职责与依赖结论、测试证据及未能证明项。发布审查启用时只承接其中 core/adapter 部分，其他发布检查与必要门禁仍由发布流程执行。
