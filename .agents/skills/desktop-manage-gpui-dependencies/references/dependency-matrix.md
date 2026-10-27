# GPUI 版本与能力矩阵

核对日期：2026-10-19。这里是工程参考；目标项目根 Cargo 和成员 target 条件为权威，不声称 registry 最新版本。

| 角色 | 中性 renderer 的声明下界 | 已有产品采用的声明参考 | 选择条件 |
| --- | --- | --- | --- |
| 组件与图标 | `gpui-kit 0.7.1` | `0.7.3` | 固定原生 GUI；避免另装不兼容 GPUI 副本 |
| 编译期词典 | `rust-i18n 4.2.0` | `4.2.4` | 固定中英词典；更高产品下界不回退 |
| 系统语言 | `sys-locale 0.3.2` | `0.3.2` | 固定真实系统语言 |
| 结构化日志 | `tracing 0.1.44`、`tracing-subscriber 0.3.23`、`tracing-appender 0.2.5` | 同左 | 端侧单 subscriber、有界日志；core 仅 facade |
| 托盘 | `tray-icon 0.26.1`，关闭默认 features | `=0.26.1` + 路径补丁 | profile 启用；renderer 仍限 macOS/Windows；Linux KSNI 需另有真实接线 |
| 自启 | `auto-launch 0.6.0` | 同左 | profile 启用；启动不自动注册 |
| 单实例 IPC | `interprocess 2.4.4` | `2.4.5` | profile 启用；私有锁及有界唤醒消息 |
| 窗口桥 | `raw-window-handle 0.6.2` | 同左 | renderer 仅 Windows 按需继承；产品按真实平台扩展 |
| 全局快捷键 | `global-hotkey 0.8.0` | 同左 | renderer 限 macOS/Windows；空 action contract 零注册 |

以上非 `=` 版本是 Cargo 兼容范围，不能把声明下界差异描述为实际安装降级。中性 renderer 的历史版本表见 [adapter 依赖参考](../../desktop-add-gpui-adapter/references/dependency-baseline.md)，它不替代当前产品清单。

有实际需求再选的解决方案：异步 I/O 可由适配器拥有一个 Tokio runtime；图片可按格式裁剪 `image`/`png`；结构化持久化可按需求选择 Serde 与数据库；CDP、ZIP、HTTP/WebSocket 不属于 GPUI 必选栈。具体版本只取目标清单，不把某产品的数据库、桌宠、CDP 或整套系统 API features 传播到中性模板。

Linux 原生方案分别核对：StatusNotifierItem/会话 D-Bus 托盘（如 `tray-icon/ksni`，不强塞 GTK 循环）、X11/XWayland 原生窗口、Wayland Portal 快捷键与用户批准。声明依赖、实际后台服务接线、注册回执、目标宿主验收是四项独立证据。解除 renderer 限制必须先有完整中性实现与回归，不能只删除 guard。

本项目Linux实际继承`ashpd 0.13.13`（`async-io,global_shortcuts`）与`x11rb 0.13.2`（`xkb`及GUI的`shape,randr`）；ashpd的global_shortcuts仅准备热键协议，其复用的zbus已用于托盘探测与通知，x11rb已有窗口/工作区/输入区域消费者。当前KSNI、窗口及freedesktop通知的接线方案见[原生宿主引用](../../desktop-add-gpui-adapter/references/linux-native.md)，历史候选与当前双架构打包的[验证范围](../../../../docs/LINUX_GPUI_TOOLCHAIN.md#已验证能力与证据索引)不改变中性依赖矩阵或产品版本。

中性依赖只按真实消费者选择并裁剪 features，兼容案例的完整组合不加入默认；具体版本和 target 条件仍取当前项目 Cargo。

锁策略为 `ignored` 时，本机 Cargo.lock 只代表一次解析；`tracked` 时使用受跟踪锁与 `--locked`。path patch 即使包版本相同也会改变内容身份，报告来源摘要与补丁摘要，不能只报 SemVer。
