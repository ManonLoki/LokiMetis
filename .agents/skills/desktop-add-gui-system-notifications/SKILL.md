---
name: desktop-add-gui-system-notifications
description: 为已选择 GUI 系统通知能力的下游安装并接入跨平台原生通知、权限偏好、设置开关和生命周期回归；未选择时不得接入。
---

# 增加 GUI 系统通知

只在 `docs/GUI_APP_PROFILE.md` 的 `system_notification = enabled`，或初始化后产品范围明确批准系统通知时使用。该选择只安装通知基础设施并在设置页提供入口，不批准任何产品通知触发器、标题、正文、动作或业务默认值。

## 固定依赖与边界

- 根 `[workspace.dependencies]` 声明 `tauri-plugin-notification = "2.4.0"` 与 `mac-usernotifications = "0.3.1"`，含义是经过验证的最低兼容稳定下界，不是精确锁定；GUI member 只用 `workspace = true`。`mac-usernotifications` 只放在 member 的 macOS target dependencies 中。
- Windows 与 Linux 使用官方 `tauri-plugin-notification` Rust API。macOS 使用 `mac-usernotifications` 0.3.1 的现代 `UNUserNotificationCenter` 异步 API：先通过 `get_notification_settings().await` 读取 `AuthorizationStatus`，只有 `NotDetermined` 才调用 `request_auth().await` 并复读状态；`Authorized`、`Provisional`、`Ephemeral` 才视为已授权，`Denied` 与 crate 返回的 `Unknown`（归一化为受限/未知状态）都不得再次弹权限框。投递通过 `Notification::new()`/`default_sound()`/`send().await`，不得退回已废弃的 `NSUserNotificationCenter` 或吞掉投递结果。
- 通知插件只按 GUI adapter 的中央稳定顺序注册：启用时位于 single-instance/deep-link、两项 Rust-only 固定基线与 dialog 固定 WebView 基线之后，autostart/global-shortcut 之前，不得自行争用首 plugin。WebView 不安装 `@tauri-apps/plugin-notification`，也不授予 `notification:*` capability；只公开设置所需的窄 Rust command。有效 bundle identifier 和打包应用签名属于 macOS 原生通知验收前提。
- `system_notification = disabled` 时，根/member 依赖、macOS target 依赖、插件注册、worker、命令、事件、设置 Switch、翻译键和通知专属测试都必须缺席。其他已批准能力真实共用 Tokio feature 时不删除共用依赖。

## 实施契约

1. 在 GUI adapter 建立单个串行通知 worker。命令至少区分权限请求与投递，使用 MPSC 串行化，并以 oneshot 把权限结果返回设置命令。worker 的发送端与 `JoinHandle` 由应用 runtime state 拥有；`ExitRequested`、`Exit` 和 Drop 路径必须关闭通道并等待或取消任务，禁止 detached task。
2. 公开 `get_system_notification_setting -> bool` 与 `set_system_notification_enabled(enabled) -> bool` 普通异步 Tauri command；mutation 成功返回重新读取的最终持久化状态。应用级偏好默认 `false`；启动时不得主动查询权限、弹权限框或打开系统设置。由 `false` 切到 `true` 时必须先完成系统权限请求；macOS 必须在同一个串行 worker 操作中先读取 `AuthorizationStatus`：`Authorized`/`Provisional`/`Ephemeral` 直接进入持久化；`NotDetermined` 才请求权限并复读；`Denied`、归一化的 `Restricted`/`Unknown` 或请求后仍未授权时，调用无 WebView command、无用户 URL 参数的 Rust-only 受控 opener，以固定 `x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=` 前缀和当前 `AppHandle` 的 `app.config().identifier` 打开当前应用的 macOS Notifications 设置，然后保持偏好为 `false` 并返回稳定错误。opener 只能接收内部 `AppHandle`，必须使用固定系统入口、检查实际退出/打开结果，不能接受任意 URL、shell 字符串或外部 bundle identifier；打开失败返回另一稳定可观察错误。只有明确授权后才原子持久化 `true`；macOS 的明确授权必须来自首次读取或请求后的复读结果。权限状态读取失败、拒绝、受限、opener 不可用、worker 关闭或持久化失败都保持原值，前端随后通过 get command 重读权威值。切到 `false` 只关闭应用偏好，不伪称撤回 OS 权限。
3. 偏好属于 GUI 宿主设置，而不是领域事实。优先复用已有 GUI 宿主设置文档；若不存在，建立带 schema 版本、4 KiB 上限、符号链接拒绝和异步安全写入的窄设置文件。不得用 React localStorage、Jotai 或 TanStack Query 创建权威副本。损坏、未知版本或缺失数据安全回退为 `false`。
4. 只向 Rust 产品适配层提供有界 `NotificationPayload { title, body }` 投递入口，不向 WebView 暴露任意通知内容命令。产品后来批准的触发条件和语言中立业务决定留在 core；GUI adapter 根据当前规范化 locale 生成用户可见标题/正文并排队。中性 scaffold 不发送测试通知，也不虚构产品消息。
5. 投递不阻塞或回滚已经成功的领域转换。权限、排队和投递失败映射为稳定错误，并通过单一可观察 GUI 事件或持久可读状态呈现；不得只写日志或把失败显示成已开启。日志不得记录通知正文中的个人数据或未脱敏业务载荷。
6. `/settings` 只在本能力启用时增加系统通知 `Switch`。开关直接绑定自身事件，周围 `Paper` 不代理点击；请求期间 disabled，成功时以 set command 返回的最终布尔值更新 checked，失败时调用 get command 重读权威值后回滚，并以 `role="alert"` 显示恢复动作；pending/成功状态使用 `role="status"`。初始化默认关闭，语言切换后文案立即更新。

## 必须保留的回归

- `system_notification_defaults_disabled`
- `system_notification_permission_precedes_persistence`
- `macos_notification_authorization_status_precedes_request`
- `macos_notification_request_is_limited_to_not_determined`
- `macos_denied_or_restricted_notification_opens_settings`
- `macos_notification_undetermined_after_request_opens_settings`
- `macos_notification_settings_targets_current_app`
- `macos_notification_settings_open_failure_is_observable`
- `system_notification_delivery_failure_is_observable`
- `system_notification_channel_serializes_authorization_and_delivery`
- `system_notification_worker_is_owned_and_cancelled`
- macOS 条件测试 `macos_system_notifications_use_modern_user_notifications`
- 前端覆盖 Switch 自身点击、父级不代理、pending 禁用、授权成功开启、拒绝后回滚与错误反馈
- disabled 配置拒绝通知依赖、插件、命令、WebView ACL、Switch、翻译键和运行时残留

初始化调试 E2E 只验证默认关闭的设置项、结构和失败可见性；不得查询或污染真实授权决定、打开系统设置，也不得把未签名/未打包调试二进制当作 macOS 权限弹窗、设置跳转或横幅证据。真实权限、设置恢复和通知投递留给已签名安装候选验收。

## 完成输出

报告依赖下界、平台分流、插件顺序、Rust-only capability 边界、偏好文件与默认值、worker 所有权/回收、设置页状态机、测试，以及未完成的已安装候选平台验收。不要报告任何未获批准的产品通知内容。
