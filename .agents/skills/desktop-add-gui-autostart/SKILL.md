---
name: desktop-add-gui-autostart
description: 为已选择 GUI 开机自启能力的下游安装官方插件、Rust 状态命令、设置开关和可恢复的真实宿主验证；未选择时不得接入。
---

# 增加 GUI 开机自启

只在 `docs/GUI_APP_PROFILE.md` 的 `autostart = enabled`，或初始化后产品范围明确批准开机自启时使用。该选择表示应用提供可发现的设置能力，不表示初始化时替用户注册登录项。

## 固定依赖与边界

- 根 `[workspace.dependencies]` 声明 `tauri-plugin-autostart = "2.5.1"`，作为经过验证的最低兼容稳定下界；GUI member 只在桌面 target dependencies 中以 `workspace = true` 继承。
- 使用官方 Rust API、`MacosLauncher::LaunchAgent` 和无附加启动参数的 `tauri_plugin_autostart::init(..., None)`。前端不安装 `@tauri-apps/plugin-autostart`，capability 不授予 `autostart:*`；WebView 只调用窄 Rust command。
- 插件只按 GUI adapter 的中央稳定顺序注册：启用时位于 single-instance/deep-link、三项 Rust-only 固定基线、dialog 固定 WebView 基线与可选 notification 之后、global-shortcut 之前，并且恰好注册一次；本 Skill 不自行争用首 plugin 或另建第二条 Builder 链。
- 默认启动行为是正常显示主窗口。不得擅自增加 `--hidden`、`--minimized` 或隐藏到托盘参数；只有产品明确批准且 `system_tray = enabled`、恢复路径通过真实宿主验证后，才能另行设计隐藏启动。
- `autostart = disabled` 时，根/member 依赖、插件注册、ManagerExt、命令、设置 Switch、翻译键、启动参数和专属测试必须缺席。

## 实施契约

1. 初始化插件但不调用 `enable()`。公开普通异步 Tauri command `get_autostart_enabled -> bool` 与 `set_autostart_enabled(enabled) -> bool`，内部通过 `tauri_plugin_autostart::ManagerExt` 获取 `app.autolaunch()`，并调用 `is_enabled()`、`enable()` 或 `disable()`；mutation 成功必须重新读取并返回最终 OS 状态。
2. OS 登录项注册状态是唯一权威事实，不建立 JSON、数据库、localStorage、Jotai 或 TanStack Query 的第二持久副本。设置页加载与每次 mutation 成功后都重新读取 OS 状态；新安装默认未注册，但发现既有注册时必须如实显示开启。
3. mutation 先保存操作前的 OS 状态。启用/禁用失败时返回稳定错误并重新读取实际状态；前端捕获后必须调用 get command，以重读结果同步界面，不得仅恢复组件先前值或乐观显示成功。即便插件操作部分生效后报错，OS 实际状态与先前 UI 不同也必须以 OS 为准。重复设置为当前状态必须幂等。插件初始化失败或状态不可读时保持应用可启动，并在设置页显示可恢复错误。
4. `/settings` 只在本能力启用时增加开机自启 `Switch`。开关事件只绑定 Switch 自身，父级不代理；执行期间 disabled，成功/失败通过 live region 可感知。不得把 capability 的 `enabled` 配置误显示成 OS 已开启。
5. 真实宿主 E2E 在操作前记录 `is_enabled()`，切换并重新读取 OS 登录项，随后无论成功、失败、超时或取消都恢复原状态并再次确认。测试创建的登录项不得遗留；无法恢复时阻断初始化并明确报告人工恢复步骤。

## 必须保留的回归

- `autostart_defaults_disabled_without_registration`
- `autostart_state_reads_operating_system_registration`
- `autostart_enable_disable_failures_are_observable`
- `autostart_commands_are_idempotent`
- 前端覆盖 OS 初始状态、Switch 自身点击、父级不代理、pending 禁用、失败后回滚
- 真实宿主场景 `autostart_e2e_restores_previous_registration`
- disabled 配置拒绝插件依赖、注册、命令、WebView ACL、Switch、翻译键与启动参数残留

## 完成输出

报告依赖下界、插件注册、Rust-only capability 边界、OS 权威状态、默认未注册、设置页状态机、宿主登录项恢复结果和未验证平台。不得把“提供开关”表述为已替用户开启。
