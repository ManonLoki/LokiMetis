---
name: desktop-prepare-gui-support-surfaces
description: 为 GUI-only 下游按八项配置建立标题、侧栏、设置、语言、主题与固定本地更新日志，并按选择建立赞助页。
---

# 准备 GUI 支持界面

使用 `$desktop-prepare-gui-support-surfaces` 只建立本地、按 profile 裁剪的支持界面。

## 输入与固定基线

1. 读取 `docs/ENGINEERING_RULES.md`、`docs/design_standards/README.md`、`docs/GUI_APP_PROFILE.md`、[界面所有权](references/gui-support-surfaces.md) 与 [设置/赞助集成](references/settings-and-sponsor-pages.md)。
2. 要求 profile 恰有八项：七个 `enabled|disabled` 条件能力和 `sidebar_mode = compact|detailed`，不得含 `about_page`；深链接启用时单实例也必须启用。
3. 所有 GUI 固定建立 `{applicationName} v{version}` 标题、本地 Logo→版本侧栏、含应用/版本与本地更新日志的 `/settings`、i18n、浅色/深色/跟随系统主题、system-locale、window-state 与 dialog。标题、应用元数据、侧栏和设置页不得包含 QQ 或其他联系人。
4. 设置页不建立联系人、隐私或统计控件。Harness 不接入应用内版本更新、在线检查、下载安装网络或相关资源；本地更新日志只从候选内资源读取。

## 实现

1. 严格消费 profile。通知/自启 Switch、赞助路由、托盘文案、深链接、快捷键状态与媒体只随已启用能力进入运行时；禁用能力零依赖、零命令、零 UI/i18n、零资源残留。`about_page`、About 路由、入口和资源必须缺席；中性及生产 GUI 也不得生成 `/test` 路由或 `TestPage` 诊断页面。
2. `compact` 精确使用 80px/6px/36px/22px/56px 基线且不可折叠；`detailed` 为 248px 展开、76px 收起、72px/44px Logo 与 22px 图标。折叠动作只绑定 ActionIcon，AppShell 宽度与内容偏移同源。
3. 所有 GUI 固定复制 `rust/release_notes.rs`、React loader/dialog 与 `tauri/tauri.release.conf.json`，通过窄命令 `load_release_notes` 从 `BaseDirectory::Resource` 异步读取根 `release-notes.json`。Rust/React 校验 schema v2、1 MiB、近五版、每类十条、完整双语对、最新在前和单个小写 `v`；设置页呈现 loading/error/retry。首次正式发布前资源缺失时显示可重试错误，不得使用编译时假数据。
4. `sponsor_page = enabled` 时逐字节复制 `media/sponsor/*`，使用 profile 的品牌、赞助支持联系方式、档位与支付码；禁用时这些媒体不得进入应用 bundle。QQ 只允许作为赞助支持联系方式在赞助页显示；静态支付码不授权订单、账户、权益或自动支付。
5. 页面工作状态使用应用根 Jotai store，仅当前进程跨路由保留；宿主能力状态以 Rust/OS 为权威。按钮、链接与 Switch 的事件绑定自身，父容器不代理动作。
6. 初始化不创建 `docs/GUI_SUPPORT_SURFACES.md`。只有下游明确修改固定支持界面时，才从模板建立产品事实文档；Harness upgrade 将其视为 protected。

## 验证

- 运行 SupportSurface React 测试、固定 release-notes Rust/React 合同与 GUI 生命周期结构检查；
- 验证两种侧栏、`{applicationName} v{version}` 标题、设置页应用/版本/本地更新日志、语言/主题、条件赞助路由、About 与测试诊断页零残留，以及能力零残留；
- 核对 12 个 sponsor 源图片的 MIME、尺寸、字节数与 SHA-256；
- 负向检查禁止应用内版本更新与产品统计网络管线；
- 不把模板测试、调试页面或本地媒体检查表述为正式候选验收。

完成时报告八项配置、固定设置页、条件赞助页面/媒体、本地 release notes、QQ 仅赞助页边界、实际测试与未执行候选验证。
