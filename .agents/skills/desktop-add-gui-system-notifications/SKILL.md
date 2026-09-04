---
name: desktop-add-gui-system-notifications
description: 为已选择 GUI 系统通知能力的下游安装并接入跨平台原生通知、权限偏好、设置开关和生命周期回归；未选择时不得接入。
---

# 增加 GUI 系统通知

只在 `docs/GUI_APP_PROFILE.md` 的 `system_notification = enabled`，或初始化后产品范围明确批准系统通知时使用。该选择只安装通知基础设施并在设置页提供入口，不批准任何产品通知触发器、标题、正文、动作或业务默认值。

## 固定依赖与边界

- 根 `[workspace.dependencies]` 声明 `tauri-plugin-notification = "2.4.0"` 与 `mac-usernotifications = "0.3.1"`，含义是经过验证的最低兼容稳定下界，不是精确锁定；GUI member 只用 `workspace = true`。`mac-usernotifications` 只放在 member 的 macOS target dependencies 中。
- Windows 与 Linux 使用官方 `tauri-plugin-notification` Rust API。macOS 使用 `mac-usernotifications` 0.3.1 的现代 `UNUserNotificationCenter` 异步 API：权限调用 `request_auth().await`，投递通过 `Notification::new()`/`default_sound()`/`send().await`，不得退回已废弃的 `NSUserNotificationCenter` 或吞掉投递结果。
- 通知插件只按 GUI adapter 的中央稳定顺序注册：启用时位于 single-instance/deep-link、三项 Rust-only 固定基线与 dialog 固定 WebView 基线之后，autostart/global-shortcut 之前，不得自行争用首 plugin。WebView 不安装 `@tauri-apps/plugin-notification`，也不授予 `notification:*` capability；只公开设置所需的窄 Rust command。有效 bundle identifier 和打包应用签名属于 macOS 原生通知验收前提。
- `system_notification = disabled` 时，根/member 依赖、macOS target 依赖、插件注册、worker、命令、事件、设置 Switch、翻译键和通知专属测试都必须缺席。其他已批准能力真实共用 Tokio feature 时不删除共用依赖。

## 实施契约

1. 在 GUI adapter 建立单个串行通知 worker。命令至少区分权限请求与投递，使用 MPSC 串行化，并以 oneshot 把权限结果返回设置命令。worker 的发送端与 `JoinHandle` 由应用 runtime state 拥有；`ExitRequested`、`Exit` 和 Drop 路径必须关闭通道并等待或取消任务，禁止 detached task。
2. 公开 `get_system_notification_setting -> bool` 与 `set_system_notification_enabled(enabled) -> bool` 普通异步 Tauri command；mutation 成功返回重新读取的最终持久化状态。应用级偏好默认 `false`；启动时不得主动弹权限框。由 `false` 切到 `true` 时先完成系统权限请求，只有明确授权后才原子持久化 `true`。拒绝、不可用、worker 关闭或持久化失败都保持原值并返回稳定错误，前端随后通过 get command 重读权威值。切到 `false` 只关闭应用偏好，不伪称撤回 OS 权限。
3. 偏好属于 GUI 宿主设置，而不是领域事实。优先复用已有 GUI 宿主设置文档；若不存在，建立带 schema 版本、4 KiB 上限、符号链接拒绝和异步安全写入的窄设置文件。不得用 React localStorage、Jotai 或 TanStack Query 创建权威副本。损坏、未知版本或缺失数据安全回退为 `false`。
4. 只向 Rust 产品适配层提供有界 `NotificationPayload { title, body }` 投递入口，不向 WebView 暴露任意通知内容命令。产品后来批准的触发条件和语言中立业务决定留在 core；GUI adapter 根据当前规范化 locale 生成用户可见标题/正文并排队。中性 scaffold 不发送测试通知，也不虚构产品消息。
5. 投递不阻塞或回滚已经成功的领域转换。权限、排队和投递失败映射为稳定错误，并通过单一可观察 GUI 事件或持久可读状态呈现；不得只写日志或把失败显示成已开启。日志不得记录通知正文中的个人数据或未脱敏业务载荷。
6. `/settings` 只在本能力启用时增加系统通知 `Switch`。开关直接绑定自身事件，周围 `Paper` 不代理点击；请求期间 disabled，成功时以 set command 返回的最终布尔值更新 checked，失败时调用 get command 重读权威值后回滚，并以 `role="alert"` 显示恢复动作；pending/成功状态使用 `role="status"`。初始化默认关闭，语言切换后文案立即更新。

## 必须保留的回归

- `system_notification_defaults_disabled`
- `system_notification_permission_precedes_persistence`
- `system_notification_delivery_failure_is_observable`
- `system_notification_channel_serializes_authorization_and_delivery`
- `system_notification_worker_is_owned_and_cancelled`
- macOS 条件测试 `macos_system_notifications_use_modern_user_notifications`
- 前端覆盖 Switch 自身点击、父级不代理、pending 禁用、授权成功开启、拒绝后回滚与错误反馈
- disabled 配置拒绝通知依赖、插件、命令、WebView ACL、Switch、翻译键和运行时残留

初始化调试 E2E 只验证默认关闭的设置项、结构和失败可见性；不得把未签名/未打包调试二进制当作 macOS 权限弹窗或横幅证据。真实通知投递留给安装候选验收。

## 完成输出

报告依赖下界、平台分流、插件顺序、Rust-only capability 边界、偏好文件与默认值、worker 所有权/回收、设置页状态机、测试，以及未完成的已安装候选平台验收。不要报告任何未获批准的产品通知内容。
