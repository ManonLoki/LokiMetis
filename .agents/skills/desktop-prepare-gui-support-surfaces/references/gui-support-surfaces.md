# GUI 支持界面所有权

## 固定归属

| 事实或行为 | 所有者 |
|---|---|
| 产品展示名、版本、标题、侧栏、固定设置与主题 | GUI adapter 与 `docs/GUI_APP_PROFILE.md` |
| 业务规则、值域、状态转换和稳定错误 | shared core |
| system-locale、window-state、dialog 与条件桌面能力 | 各自 GUI 宿主 Skill |
| 正式发布日志内容 | 根 `release-notes.json`；构建时只读嵌入 |
| 产品专属支持界面差异 | 下游 `docs/GUI_SUPPORT_SURFACES.md` |
| LokiMetis `sponsor_page` 扩展、双码组件/i18n 与图片 | 受保护的 `docs/GUI_APP_PROFILE.md`、ADR 和产品实现；不属于受管 GUI 支持资产 |

## 页面合同

- `/settings` 固定存在，包含应用/版本、本地更新日志、语言和三态主题；通知/自启 Switch 严格随上游七项 profile 中对应能力存在。LokiMetis 的 product-owned `sponsor_page = enabled` 只在该页底部追加受保护产品实现已有的微信支付与支付宝双收款码区。
- 中性及生产 GUI 不生成诊断页面；固定支持导航只能包含设置。产品赞助扩展不得产生 `/sponsor` 路由、赞助侧栏/导航入口、档位、联系人或共享 sponsor template assets。
- 标题固定为 `{applicationName} v{version}`；标题、应用元数据、侧栏与设置页不包含联系人。
- 设置页与模板没有隐私或统计开关；本地更新日志不建立远程版本服务，Harness 不创建对应传输管线。
- 所有用户可见文案进入 i18n；未选能力的翻译键和运行时资源必须缺席。双码区的文案、组件和两张图片继续由产品实现持有，不得复制进本 Skill 的 i18n、模板或受管资产。

## 安全

- 默认零远程支持内容，桌面 bundle 不保存服务端秘密、发布私钥或共享口令。
- 微信支付与支付宝收款码只按受保护产品事实做本地静态展示，不解析、不优化、不重编码，也不创建订单、账户、权益或点击代理。
- 外部链接必须独立批准，由系统浏览器打开且可识别、可键盘访问。
- dialog 只授予 `dialog:default`，路径选择不自动授权读取或写入。

## 测试

验证上游七字段 profile、两个侧栏、固定设置/主题/i18n、固定 release-notes 双层校验、固定支持导航仅含设置，以及条件宿主能力的启用完整与禁用无残留。LokiMetis 另以受保护产品测试验证 `sponsor_page = enabled` 只对应 `/settings` 底部双码区，现有产品图片路径与字节保持不变，并确认 `/sponsor`、赞助导航、共享 sponsor template assets 和受管二维码资产缺席。
