---
schema_version: 2
confirmed_by: ManonLoki
confirmed_at: 2026-09-05
decision_mode: reuse_then_infer_then_ask
superpowers: disabled
milestone_smoke: enabled
milestone_e2e: disabled
---

# Agent 运行策略

本文件是 LokiMetis 的 Agent 能力偏好、完整候选冒烟偏好和候选构建 E2E 建议默认值的唯一持久事实来源。项目负责人已确认采用推荐预设；GUI 发布性能选择不持久化，每次发布单独解析。

## 字段语义

- `superpowers: disabled`：不得调用或遵循名称以 `superpowers:` 开头的 Skill。
- `milestone_smoke: enabled`：只在完整真实候选验收中，对适用候选运行冒烟测试。
- `milestone_e2e: disabled`：只作为每次发布候选构建询问 E2E 时的建议默认值；不能代替当前候选的明确选择。
- `decision_mode: reuse_then_infer_then_ask`：先复用当前请求中的明确值，再依据本文件给出建议；仍无法解析时询问一次。

`enabled` 表示能力在适用流程中启用，`disabled` 表示默认不启用。安全、产品和渠道硬要求优先，持久偏好不能授权凭据、支付、生产数据、发布或不可逆副作用，也不能把失败或未执行改判为通过。

## 日常开发与本地试包

- 日常开发直接实施，只运行本次变化需要的相关非空单元/回归测试和最小替代检查。本文件不自动触发持久计划、全仓检查、构建、冒烟、E2E 或完整验收。
- Windows GUI 普通本地安装试包由 `$desktop-build-tauri-local-install` 处理。它不要求 clean HEAD，不创建发布候选，不提交、不安装，也不解析候选 E2E 或性能选择。

## GUI 发布候选

- 每次显式候选构建在任何测试或编译前解析当次 E2E 选择；当前请求没有明确值时，以 `milestone_e2e` 作为建议默认值询问一次。该选择只对当前候选有效。
- 每次 GUI 发布独立解析 `performanceSelection: enabled | disabled`；它没有持久默认，也不能从 E2E 选择推断。
- 性能启用或产品/渠道硬要求时，由 `$desktop-test-gui-release-performance` 对同一 clean HEAD 的 release-profile 探针执行门禁。只有可豁免的纯指标失败才能在用户明确确认后记为 `waived`；不得改判为通过。
- 性能关闭且无硬要求时，manifest 记录 `performanceStatus: Not run`、原因和剩余风险，不生成性能证据或运行时绑定。
- 候选构建运行 Rust workspace 与前端全部非空单元测试，再构建真实 Tauri GUI 产物。E2E 启用或渠道要求时，只在最终候选形成后执行；关闭时记录 `Not run` 和剩余风险。
- 明确发布请求可以授权流程复核并本地提交归属明确的完成改动，但不会授权 tag、push、上传、商店提交或渠道发布。

## 策略变更

永久改变本文件中的选择必须由项目负责人确认；若变化形成长期决定，应按同日 ADR 记录原因、影响和恢复条件。临时任务约束不得静默改写本文件。
