---
name: desktop-add-gui-single-instance
description: 为已选择 GUI 单实例能力的下游安装官方 single-instance 插件并恢复既有主窗口；未选择时不得接入。
---

# 增加 GUI 单实例

只在 `docs/GUI_APP_PROFILE.md` 的 `single_instance = enabled`，或初始化后产品范围明确批准单实例时使用。

## 固定契约

1. 根 `[workspace.dependencies]` 声明 `tauri-plugin-single-instance = "2.4.4"`；GUI member 只以 `workspace = true` 继承。`deep_link = enabled` 时根声明必须改为同一下界并精确启用 `features = ["deep-link"]`；未启用深链时不得预开该 feature。
2. 该插件必须是 Tauri Builder 注册的首个 plugin，且只注册一次。第二进程只通知并退出；回调只调用共享 `restore_main_window` 恢复既有 `main` 窗口，不记录、解析或转发参数与工作目录，也不触发业务动作。
3. 与 deep-link 同时启用时，官方 `deep-link` feature 在回调之前把 URL 交给 deep-link 插件；单实例回调仍使用 `_args`/`_cwd` 并保持纯恢复。scheme 白名单、URL 验证、冷/热启动分发由 `$desktop-add-gui-deep-link` 独占，不能在回调里复制解析。
4. Linux 默认 DBus ID 来自 `tauri.conf.json` 的 bundle identifier；Snap/Flatpak 必须同时声明并验证会话 DBus own/talk 权限与该 ID。只有已批准渠道的 package ID 与 bundle identifier 无法一致时，才允许改用官方 `Builder::new().dbus_id(...)` 显式绑定已批准 ID，仍须保持首 plugin 并在真实沙箱候选重验；中性初始化不猜测渠道 ID。禁用时依赖、回调、插件注册和双启动场景都必须缺席。
5. 必须保留 `single_instance_plugin_is_registered_first` 与 `second_launch_restores_existing_main_window` 两个固定回归，并以真实 E2E 证明只剩一个长期主进程和同一主窗口。

## 完成输出

报告版本/feature、首位且唯一注册、纯恢复回调、双启动 PID/窗口证据，以及禁用时的依赖和运行时缺席。
