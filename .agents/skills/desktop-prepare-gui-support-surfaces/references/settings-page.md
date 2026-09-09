# 设置页与本地发布说明

`assets/gui-support/` 是所有 GUI 共用的固定本地设置资产。设置页及其本地更新日志链路无条件进入所有 GUI，不由 profile 裁剪。

LokiMetis 的 product-owned `sponsor_page = enabled` 不改变这份共享资产清单。受保护产品实现继续从 `loki_metis_gui/public/brand-support/sponsor/` 消费用户提供的微信支付与支付宝原图，并通过产品自有 `SponsorPaymentPanel` 与 i18n 把双码区放在 `/settings` 底部；组件、翻译和二维码都不得复制进 `assets/gui-support/` 或其它受管 Skill 资产。

## 资产

| 资产 | 用途 | 进入运行时条件 |
|---|---|---|
| `i18n/*.json` | 壳层、设置与 release-notes 文案 | 所有 GUI 固定 |
| `rust-i18n/*.yml` | 托盘原生双语文案 | 仅托盘启用 |
| `react/SettingsPageTemplate.tsx`、release-notes Rust/React 资源 | 应用、版本与本地更新日志 | 所有 GUI 固定 |
| `tauri/tauri.release.conf.json` | 将根 `release-notes.json` 映射到候选资源根 | 所有 GUI 保留；调试构建不用 |
| Sidebar、Shell、Theme、Settings 模板 | 固定 GUI 壳层 | 所有 GUI |

## 设置页

- `/settings` 是唯一固定支持页面；上游七字段初始化 profile 不包含页面开关。LokiMetis 额外保留的第八字段是受保护的产品扩展，只控制同一设置页底部已有的双码内容，不产生新页面。
- 产品名和版本来自当前打包事实，所有可见版本经共享 formatter 规范为一个小写 `v`；窗口标题固定为 `{applicationName} v{version}`。
- 页面固定展示应用、版本和本地“更新日志”按钮，不显示联系人或免责声明，也不提供在线版本检查或安装动作。LokiMetis 底部赞助区只静态展示微信支付与支付宝两张产品图片，不解析、不优化、不重绘、不重编码、不触发支付。
- 按钮事件绑定自身，外围 Paper/Group 不代理。`load_release_notes` 不接受路径参数，不授予通用文件系统权限。
- 发布日志最新在前，最多五版，每版功能优化/问题修复各十个完整 `zh-CN`/`en-US` 翻译对；未知语言回退英文。
- `load_release_notes`、loader、dialog 和设置页专属翻译键随所有 GUI 固定存在；首次正式发布前资源缺失时显示可重试错误。

## 回归

测试至少覆盖中英文、标题与设置页单 `v` 版本、设置页本地发布日志裁剪与失败重试、固定支持导航精确只有设置、父容器不代理、主题与键盘可达性。LokiMetis 的受保护产品测试另覆盖 `/settings` 底部微信支付/支付宝双码及可访问名称、现有产品路径与字节，并负向断言 `/sponsor`、赞助导航、共享 sponsor template assets 和受管二维码资产缺席。
