---
name: desktop-prepare-gui-support-surfaces
description: 为 GUI-only 下游按上游七项配置与受保护的产品扩展建立标题、侧栏、设置、语言、主题与固定本地更新日志。
---

# 准备 GUI 支持界面

使用 `$desktop-prepare-gui-support-surfaces` 只建立本地、按 profile 裁剪的支持界面。

## 输入与固定基线

1. 读取 `docs/ENGINEERING_RULES.md`、`docs/design_standards/README.md`、`docs/GUI_APP_PROFILE.md`、[界面所有权](references/gui-support-surfaces.md) 与 [设置页集成](references/settings-page.md)。
2. 要求 profile 完整保留上游七项：六个 `enabled|disabled` 条件能力和 `sidebar_mode = compact|detailed`，其相对顺序和值域不变；深链接启用时单实例也必须启用。LokiMetis 同一代码块在 `autostart` 与 `single_instance` 之间另有受保护的 product-owned `sponsor_page = enabled`，它只由既有产品文档和实现消费。
3. 所有 GUI 固定建立 `{applicationName} v{version}` 标题、本地 Logo→版本侧栏、含应用/版本与本地更新日志的 `/settings`、i18n、浅色/深色/跟随系统主题、system-locale、window-state 与 dialog。标题、应用元数据、侧栏和设置页不得包含联系人。LokiMetis 只在 `/settings` 底部保留产品自有微信支付与支付宝双收款码区，不增加固定页面或导航。
4. 设置页不建立联系人、隐私或统计控件。Harness 不接入应用内版本更新、在线检查、下载安装网络或相关资源；本地更新日志只从候选内资源读取。产品双码区只静态展示用户提供的本地图片，不解析、不重编码、不发起支付或网络请求。

## 实现

1. 严格消费上游七字段 profile。通知/自启 Switch、托盘文案、深链接与快捷键状态只随已启用能力进入运行时；禁用能力零依赖、零命令、零 UI/i18n、零资源残留。固定支持导航只能包含设置；中性及生产 GUI 不得生成诊断页面。`sponsor_page` 不是共享 Harness 能力：只复用 LokiMetis 受保护的 `SponsorPaymentPanel`、产品 i18n 与 `/brand-support/sponsor/` 两张原图，并保持它们在受管 Skill 资产之外；不得创建 `/sponsor`、赞助导航、档位、联系人或共享 sponsor template assets。
2. `compact` 精确使用 80px/6px/36px/22px/56px 基线且不可折叠；`detailed` 为 248px 展开、76px 收起、72px/44px Logo 与 22px 图标。折叠动作只绑定 ActionIcon，AppShell 宽度与内容偏移同源。
3. 所有 GUI 固定复制 `rust/release_notes.rs`、React loader/dialog 与 `tauri/tauri.release.conf.json`，通过窄命令 `load_release_notes` 从 `BaseDirectory::Resource` 异步读取根 `release-notes.json`。Rust/React 校验 schema v2、1 MiB、近五版、每类十条、完整双语对、最新在前和单个小写 `v`；设置页呈现 loading/error/retry。首次正式发布前资源缺失时显示可重试错误，不得使用编译时假数据。中性资源复制清单不得吸收 LokiMetis 的赞助组件、翻译或二维码。
4. 页面工作状态使用应用根 Jotai store，仅当前进程跨路由保留；宿主能力状态以 Rust/OS 为权威。按钮、链接与 Switch 的事件绑定自身，父容器不代理动作。
5. 初始化不创建 `docs/GUI_SUPPORT_SURFACES.md`。只有下游明确修改固定支持界面时，才从模板建立产品事实文档；Harness upgrade 将其视为 protected。LokiMetis 现有赞助差异已由受保护的 GUI profile、ADR 与产品实现定义，本 Skill 只尊重该事实，不生成或覆盖另一套模板事实。

## 验证

- 运行 SupportSurface React 测试、固定 release-notes Rust/React 合同与 GUI 生命周期结构检查；
- 验证两种侧栏、`{applicationName} v{version}` 标题、设置页应用/版本/本地更新日志、语言/主题、固定支持导航和能力零残留；
- 验证 LokiMetis 的 product-owned `sponsor_page = enabled` 只对应 `/settings` 底部微信支付/支付宝双码区，产品图片保持现有路径与字节，并负向检查 `/sponsor`、赞助导航、共享 sponsor template assets 和受管二维码资产；
- 负向检查禁止应用内版本更新与产品统计网络管线；
- 不把模板测试、调试页面或本地媒体检查表述为正式候选验收。

普通 GUI 本地试包走适用开发构建路线；用户明确请求正式发布候选时先交给 `$desktop-prepare-release`，由本次发布事实封存选择后再调用 `$desktop-build-tauri-release`。完成时报告上游七项配置、受保护的 product-owned `sponsor_page` 扩展、固定设置页、本地 release notes、实际测试与未执行候选验证。
