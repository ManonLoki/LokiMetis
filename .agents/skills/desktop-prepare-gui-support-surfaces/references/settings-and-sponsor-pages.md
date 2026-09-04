# 设置页、赞助页与品牌媒体

`assets/brand-support/` 是 GUI 产品家族共享的本地品牌依赖。设置页及其本地更新日志链路固定进入所有 GUI；赞助页面和媒体是否复制只由 `docs/GUI_APP_PROFILE.md` 的 `sponsor_page` 决定。

## 资产

| 资产 | 用途 | 进入运行时条件 |
|---|---|---|
| `brand-support-profile.json`、`react/brandSupportProfile.ts` | 品牌、赞助支持联系方式、赞助档位、支付码与安全本地路径 | 品牌身份固定；赞助内容只在赞助页消费 |
| `i18n/*.json` | 壳层、设置、赞助与 release-notes 文案 | 设置文案固定；赞助文案按能力裁剪 |
| `rust-i18n/*.yml` | 托盘原生双语文案 | 仅托盘启用 |
| `react/SettingsPageTemplate.tsx`、release-notes Rust/React 资源 | 应用、版本与本地更新日志 | 所有 GUI 固定 |
| `tauri/tauri.release.conf.json` | 将根 `release-notes.json` 映射到候选资源根 | 所有 GUI 保留；调试构建不用 |
| `react/SponsorPageTemplate.tsx`、`media/sponsor/*` | 三档赞助内容与两个支付码 | 仅赞助页启用 |
| Sidebar、Shell、Theme、Settings 模板 | 固定 GUI 壳层 | 所有 GUI |
| `media-manifest.json` | 12 个 sponsor 图片的摘要与用途 | 只用于复制/构建核验 |

## 设置页

- `/settings` 固定存在；`/about` 和 `about_page` 必须缺席。
- 产品名和版本来自当前打包事实，所有可见版本经共享 formatter 规范为一个小写 `v`；窗口标题固定为 `{applicationName} v{version}`。
- 页面固定展示应用、版本和本地“更新日志”按钮，不显示 QQ、其他联系人或免责声明，也不提供在线版本检查或安装动作。
- 按钮事件绑定自身，外围 Paper/Group 不代理。`load_release_notes` 不接受路径参数，不授予通用文件系统权限。
- 发布日志最新在前，最多五版，每版功能优化/问题修复各十个完整 `zh-CN`/`en-US` 翻译对；未知语言回退英文。
- `load_release_notes`、loader、dialog 和设置页专属翻译键随所有 GUI 固定存在；首次正式发布前资源缺失时显示可重试错误。

## 赞助页

- `/sponsor` 只在 `sponsor_page = enabled` 时存在。
- QQ 仅可作为赞助支持联系方式在本页显示，不得进入标题、应用元数据、侧栏、设置页或其他产品页面。
- 只使用 `/brand-support/sponsor/` 下打包本地绝对路径，拒绝远程 scheme、协议相对 URL、反斜杠和 `..`。
- 支付二维码必须逐字节复制，不优化、重绘、解码或重编码；页面必须提供本地化可访问名称。
- Sponsor 卡片在窄宽度可换行，不用固定最小宽度或透明点击遮罩；视频若增加则必须有 controls、字幕和文字稿。
- 静态展示不代表真实交易授权。

## 回归

测试至少覆盖中英文、标题与设置页单 `v` 版本、设置页本地发布日志裁剪与失败重试、About 零残留、父容器不代理、赞助三档/双支付码、QQ 仅赞助页、远程路径拒绝、主题与键盘可达性，以及启用/禁用赞助运行时资源边界。
