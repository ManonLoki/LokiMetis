# GUI 支持界面所有权

## 固定归属

| 事实或行为 | 所有者 |
|---|---|
| 产品展示名、版本、标题、侧栏、固定设置、主题与条件赞助页 | GUI adapter 与 `docs/GUI_APP_PROFILE.md` |
| 业务规则、值域、状态转换和稳定错误 | shared core |
| system-locale、window-state、dialog 与条件桌面能力 | 各自 GUI 宿主 Skill |
| 赞助支持联系方式、赞助内容、支付码和本地路径 | `assets/brand-support/`；只由赞助页消费 |
| 正式发布日志内容 | 根 `release-notes.json`；构建时只读嵌入 |
| 产品专属支持界面差异 | 下游 `docs/GUI_SUPPORT_SURFACES.md` |

## 页面合同

- `/settings` 固定存在，包含应用/版本、本地更新日志、语言和三态主题；通知/自启 Switch 严格随八项 profile 中对应能力存在。
- `/about` 与 `about_page` 必须缺席；中性及生产 GUI 不生成 `/test` 或 `TestPage` 诊断页面；`/sponsor` 只在 `sponsor_page` 启用时存在并进入导航。
- 标题固定为 `{applicationName} v{version}`；标题、应用元数据、侧栏与设置页不包含联系人。QQ 只可作为赞助支持联系方式出现在已选赞助页。
- 设置页与模板没有隐私或统计开关；本地更新日志不建立远程版本服务，Harness 不创建对应传输管线。
- 所有用户可见文案进入 i18n；未选能力的翻译键和运行时资源必须缺席。

## 安全

- 默认零远程支持内容，桌面 bundle 不保存服务端秘密、发布私钥或共享口令。
- 外部链接必须独立批准，由系统浏览器打开且可识别、可键盘访问。
- dialog 只授予 `dialog:default`，路径选择不自动授权读取或写入。
- 赞助媒体只做本地静态展示，不能隐式创建支付副作用。

## 测试

验证八字段 profile、两个侧栏、固定设置/主题/i18n、固定 release-notes 双层校验、条件赞助路由/媒体、About 零残留与启用/禁用零残留；媒体清单必须与 12 个源图片逐字节匹配。
