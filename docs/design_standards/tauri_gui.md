# Tauri GUI 通用设计标准

`standard_id = tauri-gui-common-v1`

本标准在界面为 Tauri 2 + React + Mantine 时命中。它约束 GUI adapter 的展示和纯交互层，不建立业务规则、持久业务状态或 core 依赖反向引用。固定左侧栏还必须叠加 [Tauri GUI 左侧栏标准](tauri_sidebar.md)。

## 设计原则

- 任务优先：首屏先突出当前任务，再呈现辅助信息和诊断细节。
- 克制一致：同一语义使用同一组件、颜色、密度和反馈；不靠额外阴影、颜色或动画制造层级。
- 渐进披露：低频设置、帮助和诊断进入专用页面或展开区。
- 可恢复：加载、空态、错误、禁用和破坏性操作都说明当前状态与下一步。
- 内容驱动：中英文、200% 文本缩放和窄窗口不能造成内容裁切。
- 无障碍默认开启：键盘、可见焦点、语义名称、对比度、live region 与 reduced motion 是实现基线。

## Theme 与布局

- React 根只挂载一个 `MantineProvider`。theme 在组件外的稳定模块中定义；语言、主题和已批准的设备偏好留在 GUI adapter。
- 主题使用显式 `localStorageColorSchemeManager` 与稳定 key，默认跟随系统并允许 `light`、`dark`、`auto`；存储失败回退安全默认，不得阻断启动。亮暗模式都定义页面、surface、主/次文字、边框和强调色语义。
- 主色、字体、字号、行高、间距、圆角、阴影、断点和组件默认值先进入 `createTheme`。品牌色只从已批准的 `docs/GUI_APP_PROFILE.md` 派生；页面、组件和 CSS 不复制品牌 hex，也不固定只适合一种主题的字体或色值。自定义色必须提供当前 Mantine 版本要求的完整色阶，并验证 hover、active、对比和暗色行为。
- 样式优先级固定为：theme token/组件 `defaultProps` → Mantine props/简单 style props → CSS Modules 与 Mantine CSS variables → Styles API → 仅由运行时测量得到的一次性内联 `style`。不要用 `!important`、高优先级全局 CSS 或 Mantine 私有 DOM 选择器兜底。
- 页面按 `page → surface → section → field → action` 组织。DOM、视觉和键盘顺序保持一致；弹层关闭后焦点回到触发器。
- 普通文字对比至少 `4.5:1`，大号文字、必要非文字信息和焦点指示至少 `3:1`。颜色不能是状态的唯一线索。
- 交互目标至少满足 `24 × 24 CSS px` 或对应间距例外；高频主操作优先接近 `40 × 40 CSS px`。
- 主窗口首次启动，或持久状态缺失、损坏、离开所有当前显示器工作区时，以 `1440×900` 逻辑像素、最小 `960×640`、居中和避免溢出作回退；后续启动只恢复合法的尺寸、位置和最大化状态，必须忽略上次的隐藏、最小化、全屏和装饰状态。该窗口不得与 `660×400` 的 macOS DMG 安装卷窗口混淆。最小窗口尺寸必须与最窄可用布局一致，不能依赖最大化完成核心任务。

| 领域 | 语义与约束 |
|---|---|
| 间距 | `xs/sm` 用于同组细节和控件内部，`md` 用于常规组件，`lg/xl` 用于区块和页面 |
| 圆角 | 控件使用 `sm/md`，普通 surface 使用 `lg`，强品牌容器才使用 `xl` |
| 阴影 | 普通内容优先边框；浮层、拖拽对象或临时脱离页面的区域才使用 `xs/sm/md` 轻阴影 |
| 品牌色 | `brand` 只用于主动作、选中、焦点和少量强调，不替代正文或所有状态色 |
| 中性色 | `gray/dark` 统一表达页面、surface、边框和主/次文字，不为每页建立近似灰色 |
| 状态色 | success/warning/danger/info 必须同时有文字、图标或语义属性，颜色不能独立承载含义 |

- focus ring 不得关闭；sticky header、overlay 和滚动容器不能遮挡焦点。动画尊重 `prefers-reduced-motion`，且不能成为唯一状态提示。标题按 `h1 → h2 → h3` 连续组织，不为视觉大小跳级。
- 按等价 `320 CSS px` 宽度、200% 文本缩放和用户覆盖文本间距先保证单向回流，再扩展列数与留白。断点只处理真实结构变化；少量变化使用 Mantine 响应式 props，复杂或复用布局使用 CSS Modules，大列表不逐项生成响应式 style props。
- 文案、按钮、表单和状态区域允许自然换行，不固定高度裁切中文或英文。整个页面不得产生横向滚动；只有确需二维浏览的数据区域可以局部横向滚动。

## 图标、文案与版本

- 图标直接使用 `@tabler/icons-react` 的命名组件。存在适用图标时不得引入其他图标库、手写 SVG、字符或 emoji；数据图形本身不属于图标替代范围。
- 用户可见文案进入 `i18next`/`react-i18next`，Rust 原生界面文案进入 `rust-i18n`。不得通过字符串拼接组成可翻译句子，core 保持语言无关。
- 可见版本在展示边界先移除已有 `v`/`V` 前缀，再添加且只添加一个小写 `v`。机器版本字段保持无展示前缀。
- 窗口标题固定为 `{applicationName} v{version}`；不得附加未声明的反馈渠道或其他标识。
- 仅图标按钮必须有可访问名称。Tooltip 只能补充可见提示，不能替代 `aria-label` 或等价名称。

## 组件与交互

- 导航使用链接或语义 button，操作使用 button；不得用 `div onClick` 模拟控件。
- 点击处理器绑定在实际拥有动作的 `NavLink`、Button、`Switch`、`Checkbox` 或菜单项上。Card、表格行、单元格和其他父容器不得代理子控件动作。
- 当前路由的选中态由导航项自身表达；焦点、hover、active 和 disabled 必须可区分。
- Button 文案使用“动词 + 对象”。同一操作组只保留一个主动作：主动作用 brand filled，次要动作用 default/outline，低优先级用 subtle/link；危险动作使用 danger 且不能与品牌主色混淆。
- `Paper`/Card 只承载一个明确主题，避免卡片套卡片。提交期间使用 loading/disabled 防止重复提交，完成后提供可感知反馈。破坏性操作显示对象和影响范围，不可轻易恢复时二次确认，并让取消保持安全默认。
- 首次加载、后台刷新、提交、成功、空态、可重试错误、权限错误和系统错误分别呈现，不把失败显示成空数据或成功。
- 系统通知和开机自启 Switch 只在 `docs/GUI_APP_PROFILE.md` 对应能力启用时出现，初始化运行状态默认关闭。异步切换期间禁用控件，成功后才更新；权限拒绝或 OS 注册失败时恢复实际状态并就近显示可操作错误。系统通知的应用偏好与开机自启的 OS 注册状态不得混为同一权威来源。
- 全局快捷键启用但 contract 为空时不显示占位设置或占用键位。contract 声明固定绑定时显示需求给出的 chord、动作与逐项真实注册状态；声明 `user-configurable` 时才提供录制、取消和清空，位置由已批准信息架构决定，不强制放在设置页。未绑定、已注册、已配置但注册失败必须可区分，状态失败不能伪装成已注册。
- 活动选项卡、查询/筛选、排序和分页等页面工作状态由应用根 Jotai store 在当前进程内跨路由保留；不得写入 localStorage、URL 或 core，也不得镜像 TanStack Query 数据。window-state 只持久原生窗口几何，不得借此恢复页面、查询或业务状态。只有成功查询到大于第 1 页的空页才回退第 1 页。

### 表单、数据与浮层

- 每个输入都有可见 label；placeholder 只给示例。helper 描述要求，error 描述问题与恢复动作，两者不混成常驻红字。表单默认单列，相关短字段才并排；主提交动作位于末尾，服务器错误靠近表单或相关字段。原生 Mantine 输入足够时不自建底层控件，只有真实复杂表单需要时才评估 `@mantine/form`。
- 列表和表格区分首次加载、后台刷新、空数据与失败；后台刷新保留已有内容并显示局部进度。表格内 `Switch` 只由其自身点击或键盘操作切换，点击行或单元格不得切换。
- Tooltip、Popover、Menu、Modal 和 Drawer 必须能由键盘打开与关闭，并验证 Escape、焦点圈定以及关闭后返回触发器。Tooltip 不承载唯一说明或可访问名称。

### 状态与反馈

| 状态 | 必需信息与默认呈现 |
|---|---|
| 首次加载 | 说明正在加载什么，使用接近最终结构的 Skeleton 或有名称的进度 |
| 后台刷新 | 保留旧内容，并显示局部更新进度 |
| 提交中 | 标明执行中的动作，以触发 Button loading/disabled 防重 |
| 成功 | 就近说明完成内容和必要的下一步 |
| 空态 | 说明为什么为空，并提供至多一个主动作 |
| 可重试错误 | 说明失败项、数据是否保留以及如何重试 |
| 权限错误 | 说明缺少的条件和安全恢复入口 |
| 系统错误 | 说明能否继续和数据是否安全，技术细节只进入脱敏日志 |

- 只有真实需要时才增加 `@mantine/notifications`，每个 React 根只挂载一个容器。Toast 不能承载字段错误、长说明或唯一完成证据。
- 应用更新、强制更新、产品统计和远程遥测不属于可选界面状态；页面、弹层、Toast、设置项和隐藏路由均不得为其保留占位。

## 无障碍与国际化

- 目标为 WCAG 2.2 AA；至少验证 landmarks、连续标题、键盘全流程、可见焦点、可访问名称、状态消息以及正确的 name/role/value。图像有信息时提供有意义替代文本，纯装饰图从辅助技术隐藏。
- 当前 locale 同步到文档根 `lang`。变量、复数、日期、数字与单位使用完整翻译模板和明确 locale 格式化；不得拼接句片段。
- 未移动焦点的异步结果、提交结果和错误使用 `role="status"`、`role="alert"`、`aria-live` 或等价机制，并验证只播报一次。错误文案说明影响与恢复动作，不显示原始异常、令牌、内部路径或堆栈。

## 固定支持界面

- GUI profile 字段集合恰好为 `system_tray`、`system_notification`、`autostart`、`single_instance`、`deep_link`、`global_shortcut`、`sidebar_mode`；前六项只取 `enabled|disabled`，末项只取 `compact|detailed`。
- `/settings`、应用/版本、本地更新日志、语言、三态主题和完整亮暗语义主题始终存在；默认设置页只包含这些固定内容和按 profile 启用的系统能力设置，不预置未声明区块。本地更新日志只读取候选内资源，不联网检查或安装更新。
- system-locale 和 window-state 是所有 GUI 的不询问 Rust-only 固定基线，不是 `docs/GUI_APP_PROFILE.md` 开关，不直接向 WebView 暴露通用 OS 或窗口状态 API。
- dialog 是所有 GUI 的不询问固定 WebView plugin 基线；主窗口仅授予精确 `dialog:default` 以覆盖 message、open、save 全部官方对话框类型，不使用 wildcard、deprecated alias、deny 项或额外文件系统权限。
- 中性初始化的固定支持页面、路由和导航项允许集合恰好为 Settings（`/settings`），不得生成未声明页面、隐藏路由、入口、运行时组件、媒体或专属测试；产品功能页只按已批准规格新增，开发诊断只存在于测试工具与测试夹具。系统托盘、系统通知、开机自启、单实例、深链接和全局快捷键只按 `docs/GUI_APP_PROFILE.md` 的明确选择存在。`deep_link = enabled` 必须同时有 `single_instance = enabled`。未选能力不得保留依赖、feature、插件/生命周期、配置、命令、ACL、Switch/状态、翻译键、入口或专属测试。
- 支持菜单只包含固定 Settings 项，所有用户可见标签走 i18n；不得为未声明的反馈渠道增加入口或页面内容。
- 系统通知能力不自动批准产品通知内容或触发器；开机自启能力不表示已经注册登录项；深链接的中性绑定只恢复窗口，全局快捷键的中性能力保持空动作/零绑定，两者都不批准业务 payload 或副作用。应用更新、产品统计和远程遥测固定缺席；其他真实业务出站能力仍需独立产品批准与安全配置。

## 最小回归

- Testing Library 从角色、名称和真实用户操作验证，不以 class、内部 DOM 或大快照代替语义断言。
- 控件与周围父级区域分别点击，证明父级不代理动作；导航项分别验证 active、键盘焦点和完整可访问名称。
- 条件系统设置 Switch 分别验证默认/OS 初始状态、pending 防重、成功、权限或系统失败回滚、live region，以及 disabled 配置下源码与翻译缺席。全局快捷键空 contract 覆盖零 UI/零注册；固定策略覆盖逐项只读状态、注册失败和退出注销；可编辑策略另覆盖录制、取消、清空、规范化重复、失败恢复、live region 与录制 token 清理。所有策略都拒绝 WebView 保存权威绑定。
- 共享 theme 组件在真实 `MantineProvider` 下测试亮暗模式、中文、英文、文本缩放和窄窗口行为。
- 固定设置页回归覆盖应用/版本、候选内本地更新日志的加载/失败/重试和单 `v` 展示，并确认标题、元数据、设置页及支持导航只包含允许集合的内容，没有未声明入口或页面专属资产。
- 页面会话状态用同一根 store 验证跨路由保留，用新 store 验证真正退出后恢复默认，并覆盖空页回退与错误不回退。独立的 window-state 回归覆盖尺寸/位置/最大化恢复、忽略可见性、离屏回退和首次启动默认，不把窗口持久化当作页面会话存储。
- 按实际变化覆盖 loading/empty/error、键盘、焦点恢复、禁用防重、live region、i18n、`320 CSS px` 回流、200% 文本缩放、文本间距覆盖、对比度与 reduced motion。
- 检查没有散落品牌 hex、任意间距、重复 Provider、多个 filled 主动作、Query 数据的 Jotai 镜像或未经批准的 Mantine 扩展包。
- 固定侧栏使用其专属标准中的组件测试、结构门禁和真实初始化 E2E。

## 官方参考

- [Mantine Theme object](https://mantine.dev/theming/theme-object/)
- [MantineProvider](https://mantine.dev/theming/mantine-provider/)
- [Mantine Styles API](https://mantine.dev/styles/styles-api/)
- [Mantine responsive styles](https://mantine.dev/styles/responsive/)
- [WCAG 2.2](https://www.w3.org/TR/WCAG22/)
