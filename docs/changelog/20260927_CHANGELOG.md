# 变更记录：本机用量与 Hooks 统计修复

> 记忆日期：2026-09-27

## Fixed

### 跨日与跨月用量窗口

- `change_id = BUG-USAGE-CROSS-DAY-20260927`
- `required_version = 0.3.9`
- 本机索引读取下界改由六个日历窗口的最早日期决定，避免固定近 30 日截断月初和整个上月的用量。隔离 SQLite 回归覆盖月末的本月/上月合计，以及次日零点的今日/昨日和月窗口换桶。

### Claude Desktop 与 WorkBuddyAI 用量

- `change_id = BUG-CLAUDE-DESKTOP-WORKBUDDY-USAGE-20260927`
- `required_version = 0.3.10`
- macOS Claude Desktop 固定内嵌会话目录纳入现有 Claude Code 数据源的快速发现；逐层限制枚举数量、拒绝链接和网络目录，并复用既有 transcript 签名与解析。WorkBuddyAI 对同源同调用的合法后续用量或积分观察只计最终事实，身份变化、数值回退和无法排序的冲突仍整组拒绝。

### Hooks 事件与多会话状态

- `change_id = BUG-HOOK-EVENT-SEMANTICS-20260927`
- `required_version = 0.3.11`
- WorkBuddy 和 CodeBuddy 纳入按会话聚合的状态机，避免一条会话结束清空其他活跃会话。Claude Code 补齐错误轮次、权限拒绝及用户应答后的状态迁移；Claude Code、WorkBuddy 和 CodeBuddy 按通知子类型区分权限等待与空闲提示，生成配置保留同名事件的多个 matcher。普通 `Stop` 后可由新的活动信号恢复，`StopFailure` 与已知旧轮次仍抑制迟到事件。

## Verification

- 日历边界 core 测试 1 项、隔离 SQLite GUI 测试 1 项通过。
- WorkBuddy core 测试 18 项、Claude 发现 GUI 测试 17 项、WorkBuddy GUI 测试 13 项通过。
- Hooks core 测试 113 项通过；core `cargo check` 与变更差异空白检查通过。
- 未运行真实客户端 GUI、安装候选构建或最终产物验收。没有 `turn_id` 的普通 `Stop` 之后，迟到 `PreToolUse` 与被 Stop Hook 续跑的新 `PreToolUse` 无法仅凭当前信号区分；当前按续跑处理。
