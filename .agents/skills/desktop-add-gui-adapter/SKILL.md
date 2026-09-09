---
name: desktop-add-gui-adapter
description: 为共享 Rust core 建立唯一 Tauri 2 GUI 薄适配器，并按上游七项初始化配置与受保护的产品扩展接入固定桌面基线、设置页与条件能力。
---

# 增加 GUI 适配器

## 输入

1. 完整读取 `docs/RUST_GUI_TEMPLATE.md`、`docs/design_standards/README.md`、[GUI 基线](references/gui-baseline.md)、[React 前端基线](references/react-frontend-baseline.md) 与 [Mantine 指南](references/mantine-ui-guidelines.md)。
2. 要求根 Cargo metadata 固定为 `interfaces = ["gui"]`，共享 `<project-id>_core` 已存在，`docs/GUI_APP_PROFILE.md` 含唯一完整配置块。上游七字段及其相对顺序和值域必须精确匹配 GUI profile 契约；LokiMetis 另外保留受保护的 product-owned `sponsor_page = enabled`，位置在 `autostart` 后、`single_instance` 前，只由已批准的 GUI profile、ADR 与产品实现消费。
3. 产品规格尚未批准时只公开中性 `productDefinitionRequired=true` 状态，不发明业务功能、数据、远程地址或副作用。

## 实现

1. 建立 `<project-id>_gui`：Tauri 2 + Vite + React + TypeScript + Mantine UI + Tabler Icons + TanStack Router/Query + Jotai；使用 ESLint、Prettier、Vitest 与 Testing Library。依赖写入实际 member 或前端 manifest，不做全局安装。
2. GUI 只拥有桌面装配、协议解码、展示/交互状态、宿主能力和 core 调用映射。业务规则、值域、跨字段关系、用例、持久化策略与稳定错误都留在 core；core 不依赖 Tauri、WebView、React、路由或窗口类型。
3. 按 GUI 基线直接接入固定的 system-locale、window-state 与 dialog 子契约；它们不是独立 Skill，也不进入 profile。中央 Builder 的相对顺序为：条件 single-instance、条件 deep-link、固定 os、固定 window-state、固定 dialog、条件 notification、autostart、global-shortcut；每项恰好一次。dialog 只授予主窗口 `dialog:default`，不连带文件系统权限。
4. 严格按上游七字段 profile 调用托盘、通知、自启、单实例、深链接和全局快捷键 Skills。未选能力不得留下依赖、feature、配置、命令、ACL、状态、UI/i18n、路由、媒体或专属测试。`sponsor_page` 不是共享 Harness 能力，不调用额外 Skill，也不得据此复制通用赞助依赖、模板或资产。
5. 调用 `$desktop-prepare-gui-support-surfaces` 建立标题、固定设置页、i18n、三态主题与所选侧栏。应用/版本和候选内本地更新日志固定进入设置页；固定支持页和支持导航的允许集合只有设置页，不建立额外固定支持页、远程版本或产品数据传输管线。LokiMetis 的 product-owned 扩展只保留受保护产品实现已有的 `/settings` 底部微信支付与支付宝双收款码区，不建立 `/sponsor` 路由、侧栏/导航入口、赞助档位、联系人或共享 sponsor template assets，二维码继续位于产品自有路径而非受管 Skill 资产。
6. 主窗口首次或保存状态无效时使用 1440×900、最小 960×640、居中且防溢出；window-state 只恢复 `SIZE | POSITION | MAXIMIZED`。DMG 使用项目内 660×400 背景与固定图标落点。
7. 标题固定为 `{applicationName} v{version}`。设置页始终含应用/版本、语言、浅色/深色/跟随系统和本地更新日志；只按上游 profile 增加通知与自启 Switch。LokiMetis 另从受保护的 `SponsorPaymentPanel`、产品 i18n 与 `/brand-support/sponsor/` 图片消费已批准的底部双码区，不由中性设置模板生成或托管。页面会话状态使用应用根 Jotai store，仅在进程内跨路由保留。
8. 固定复制 `src-tauri/tauri.release.conf.json`，只映射根 `release-notes.json`；固定注册窄命令 `load_release_notes`，Rust/React 双边校验 schema、1 MiB、近五版、每类十条、最新在前和单个小写 `v`，由设置页呈现 loading/error/retry。生成的固定支持页面、导航与资源必须精确匹配设置页允许集合。
9. 所有远程能力、伴随进程、宽泛权限与产品专属依赖仍需下游明确批准；初始化默认无网络副作用。

## 验证

1. 运行 core 测试、完整前端测试和当前变更对应的 GUI 结构回归。首次创建时由外层初始化流程调用一次性结构检查与真实宿主 E2E；终端下游不得依赖已裁剪的初始化测试 Skill。
2. 结构检查必须覆盖固定 os/window-state/dialog、上游七字段 profile、每项条件能力的启用完整与禁用无残留、侧栏/设置/i18n、固定 release-notes 链路、固定支持页面/导航/资源精确允许集合，以及禁止的更新与统计能力完全缺席。LokiMetis 还要验证 product-owned `sponsor_page = enabled` 仍只对应 `/settings` 底部的微信支付与支付宝双码区，产品自有两张图片保持原路径和字节，且没有 `/sponsor` 路由、赞助导航、共享赞助模板或受管二维码资产。
3. 首次初始化的真实本机调试应用操作由外层初始化流程独占编排。正式候选、性能、签名、打包与验收只由各自专用 Skill 处理。macOS 发布默认由 `$desktop-prepare-release` 记录 `macosSigningSelection = disabled`、`macosSigningSource = not-requested` 并使用 `--no-sign`，不探测身份、证书、公证凭据或 profile；只有用户已配置过、当次主动要求或渠道硬要求时才启用。启用后签名、公证与 stapling 必须作为一个不可降级阶段完成，任一步失败都阻断；启用系统通知的 macOS 候选必须签名。

完成时报告固定基线、上游七项配置、受保护的 product-owned `sponsor_page` 扩展、条件能力、固定设置页本地更新日志、core 映射、实际测试与未验证宿主边界。
