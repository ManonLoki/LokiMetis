# GUI 支持界面差异

## 基线差异

- change_id：`FEAT-SETTINGS-GITHUB-REPOSITORY-LINK`、`FEAT-SETTINGS-SPONSOR-PAYMENT-CODES`
- required_version：`0.2.16`
- GUI profile：`system_tray = enabled`、`system_notification = enabled`、`autostart = enabled`、`sponsor_page = enabled`、`single_instance = enabled`、`deep_link = enabled`、`global_shortcut = disabled`、`sidebar_mode = compact`
- 受影响路由/组件：`/settings`、`SettingsPage`、`SponsorPaymentPanel`
- 与 `docs/GUI_APP_PROFILE.md` 的关系：赞助支持已启用，但按用户明确要求嵌入设置页底部；不建立独立 `/sponsor` 路由或侧栏入口。
- 可见行为与无障碍名称：应用信息显示带 GitHub 图标的“GitHub 仓库”/“GitHub repository”按钮；赞助区展示有本地化标题、说明、图注与替代文本的微信和支付宝收款码。
- i18n 键与英文回退：新增 `settings.github_repository_*` 与 `settings.sponsor_*`，继续使用 `en-US` 作为未知语言回退。
- core/GUI 所有权理由：仓库打开是桌面宿主机制，支付码是纯静态 React 展示；两者都不定义领域规则、权威状态或持久化，因此不进入 core。

## 本地媒体

- 更新日志：继续按既有固定候选资源合同处理，本次未修改。
- 赞助媒体：只使用用户在 2026-09-08 明确提供的两张收款码，不复制共享模板的赞助档位、背景、联系人或其它十张媒体。
- 来源与许可：来源为项目负责人 ManonLoki 在当前任务中提供的原始文件，并明确授权加入 README 与应用设置。
- bundle 路径：`/brand-support/sponsor/wechat-pay.png`、`/brand-support/sponsor/alipay.jpg`
- 微信支付：PNG，1304×1777，158684 字节，SHA-256 `ad07658afe3daf83d31c447c9f16aef057e83e09c0e4c3b1d493d3f5d9627442`
- 支付宝：JPEG，1260×1890，138749 字节，SHA-256 `421bd0a127dfbdc987f3b47c9de88dd00758ab3fd919bc03722607ebc68044b1`
- 敏感材料与处理限制：按收款用途视为用户主动公开的静态素材；不得解码、优化、重绘、重编码、记录内容或触发支付。
- 禁用时零残留证明：当前赞助支持为启用状态；若未来禁用，必须同时删除设置区、i18n、测试与两张 bundle 媒体。

## 外部链接或宿主能力

- 目的与用户触发器：用户点击应用信息中的 GitHub 按钮后，由系统默认浏览器打开项目仓库。
- 允许的 origin/路径：仅 `https://github.com/ManonLoki/LokiMetis`，不接受参数、用户输入、其它 origin、文件路径或协议。
- 权限与失败语义：Tauri opener capability 精确限定上述 URL；失败时不离开设置页，并显示本地化可重试提示。
- 秘密来源引用：无秘密、令牌或凭据。
- 超时、取消、回收与日志脱敏：单次用户点击调用系统 opener，不建立任务、轮询、持久状态或载荷日志。

## 验证清单

- [x] 设置页、文案与 profile 差异一致；独立 `/sponsor` 和侧栏入口缺席。
- [x] 设置页固定包含应用/版本、语言、主题、本地 release notes、已启用宿主开关、GitHub 入口与双收款码赞助区。
- [x] 设置页从候选本地资源读取近五版双语 release notes，失败可重试。
- [x] GitHub 事件绑定实际按钮，支付码父容器没有动作。
- [x] 两张品牌媒体与用户提供原图逐字节一致。
- [x] opener 只允许精确 GitHub 仓库地址，打开失败可见且不改变当前页面。
