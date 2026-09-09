# Tauri GUI 左侧栏标准

本标准叠加于 [Tauri GUI 通用设计标准](tauri_gui.md)。命中条件是 Tauri 2 + React + Mantine GUI 使用固定左侧导航，并已在 `docs/GUI_APP_PROFILE.md` 解析 `sidebar_mode`。侧栏始终属于 GUI adapter 展示层；不得把布局、路由高亮或折叠偏好下沉到 shared core。

## 共同结构

从上到下固定为：

1. 身份区：本地 `/app-identity/logo.png`，其后为居中版本号；只有 detailed 在身份区内部提供折叠 `ActionIcon`。
2. 分隔线。
3. 从上向下增长、可滚动的产品功能菜单。中性脚手架只有使用 `IconHome` 与 i18n 文案的“首页”。
4. 分隔线。
5. 底部固定支持菜单只包含 Settings。设置始终存在并承载应用/版本与本地更新日志；支持页面、路由与导航项必须匹配这一精确允许集合，不得新增未声明页面或入口。

侧栏 `position: fixed`、高度 `100dvh`，右侧使用 `1px` 语义边框。AppShell 的 `navbar.width` 必须引用当前侧栏宽度常量；`AppShell.Navbar` 自身 padding 为 `0`，内容偏移不得再写一份数值。

每个导航动作绑定在实际 `NavLink` 或 button 上，当前路由通过该菜单项自身的 `active` 状态高亮。所有功能与支持图标使用 `@tabler/icons-react` 命名组件；不得使用其他图标库、手写 SVG、字符或 emoji。可见标签走 i18n，完整名称保留在菜单项 `aria-label`。展示版本先移除已有 `v`/`V` 前缀，再只添加一个小写 `v`。

## 精简模式

`standard_id = tauri-gui-sidebar-compact-80-v1`

匹配条件：`sidebar_mode = compact`。这是现行唯一精简密度；旧 `136px / 56px Logo / 30px 图标 / 11px / 10em` 不是此模式的回退值，任何模板或升级都不得重新应用。

| 项目 | 固定值 |
|---|---|
| 栏宽 | `80px` |
| 侧栏内容内边距 | `6px`；不得用 Mantine `p="xs"` 的 `10px` 代替 |
| Logo | `36px` |
| 菜单图标 | `22px`，`stroke = 1.75` |
| 名称 | `11px`，`line-height: 1.25`，最多两行 |
| 菜单项最小高度 | `56px` |
| 图标与名称间距 | `4px` |
| 菜单项垂直 padding | `4px` |
| 身份区与菜单区间距 | `8px` |

精简模式没有折叠状态、折叠按钮、Tooltip-only 名称或折叠持久化。每个菜单项图标在上、名称在下且名称持续可见。Logo、当前图标与可见文字沿同一条垂直中心线居中，不得裁切。

compact `NavLink` 必须同时满足：

- `flex-direction: column`、`align-items: center`、左右 padding 为 `0`；
- left section 的 `margin-inline` 为 `0`，覆盖 Mantine 默认的尾部间距；
- body 和 label 都是 `width: 100%`、`text-align: center`；body `overflow: visible`；
- 可见名称为 `display: block`、`width: 100%`、`margin-inline: auto`，允许自然换行且最多两行；
- 禁止 `em`/`ch` 固定占位盒，包括 `inlineSize: 6em/8em/10em`。名称宽度只能来自菜单项可用内容宽度。

compact AppShell 必须把 `mode="compact"` 接入实际 `AppSidebarTemplate`，使 `navbar.width === APP_SIDEBAR_WIDTHS.compact`，并让 `AppShell.Navbar` padding 为 `0`。不得因模板仍保留 detailed 实现而把运行时切换为 detailed，也不得给 compact 增加折叠按钮。

## 详细模式

`standard_id = tauri-gui-sidebar-detailed-v1`

匹配条件：`sidebar_mode = detailed`。下游 AppShell 必须把 `mode="detailed"` 接入运行时，模板可以保留 compact 实现，但不得把已选 detailed 改回 compact，也不得应用任何精简密度。

| 状态 | 栏宽 | Logo | 菜单布局 |
|---|---:|---:|---|
| 展开（首次默认） | `248px` | `72px` | 图标在左、名称在右，名称可见 |
| 收起 | `76px` | `44px` | 只显示图标；名称由 Mantine `Tooltip` 补充 |

两态共同使用 `22px` 菜单图标和 `stroke = 1.75`。展开菜单项 `min-height: 44px`，横向 padding 使用 Mantine `sm`；收起菜单项左右 padding 为 `0`，left section 的 `margin-inline` 为 `0`。详细身份区使用 Mantine `p="xs"`，不得套用 compact 的 `6px` 内容密度。折叠按钮使用 `18px` Tabler 图标：展开时为 `IconChevronLeft`，收起时为 `IconChevronRight`。

实现只从下列受管常量读取两态事实，禁止在 JSX 中另写同值魔法数字：

- `APP_SIDEBAR_WIDTHS.detailedExpanded = 248`
- `APP_SIDEBAR_WIDTHS.detailedCollapsed = 76`
- `APP_SIDEBAR_LOGO_SIZES.detailedExpanded = 72`
- `APP_SIDEBAR_LOGO_SIZES.detailedCollapsed = 44`
- `APP_SIDEBAR_NAV_ICON_SIZE_PX = 22`
- `DEFAULT_DETAILED_SIDEBAR_COLLAPSED = false`
- `APP_SIDEBAR_COLLAPSED_STORAGE_KEY = "app.sidebar.detailed.collapsed"`

折叠状态属于设备级展示偏好。`readDetailedSidebarCollapsed()` 只有在独立 localStorage 键的值严格等于字符串 `"true"` 时返回收起；键缺失、非法、存储不可用或服务端渲染环境一律展开。它不得进入 Jotai 页面会话 store、URL、TanStack Query、shared core 或业务持久化。

状态必须由 AppShell 拥有，侧栏不得内部维护另一份状态：

1. AppShell 用 `readDetailedSidebarCollapsed()` 初始化状态，并用 `detailedSidebarNavbarWidth(collapsed)` 得到 `248` 或 `76`。
2. Mantine AppShell 的 `navbar.width` 与可观察的 `data-navbar-width` 同时使用该结果，`AppShell.Navbar` padding 为 `0`。
3. `AppSidebarTemplate` 接收 `detailedCollapsed`，点击 `data-testid="app-sidebar-collapse-toggle"` 的 `ActionIcon` 后只调用 `onCollapsedChange(nextCollapsed)`。
4. AppShell 更新状态并用 `persistDetailedSidebarCollapsed(nextCollapsed)` 保存偏好，使 fixed 侧栏宽度与主内容偏移在同一次交互中同步。

展开态菜单项设置 `data-navigation-layout="icon-with-label"`，渲染可见名称且不渲染 Tooltip。收起态设置 `data-navigation-layout="icon-only"`，不渲染可见名称节点，并使用 `position="right"`、`openDelay={0}` 的 Mantine Tooltip 补充名称；菜单项自身的 `aria-label` 始终完整保留。

折叠点击只能绑定在上述 `ActionIcon` 上。Logo、版本号或身份区父级点击不得切换状态；导航仍只绑定在实际 `NavLink`/button 上。展开态保持图标+文字横排，不得改成图标在上；收起态不得保留名称或固定 `em/ch` 占位盒。Logo、当前图标与可见文字保持对齐且不裁切。

精简模式的 `80/36/22` 数值不得传播到详细模式两档，详细模式的 `248/76` 与 `72/44` 也不得被当作精简回退。

## 偏离与验证

- 只有用户明确批准的产品特殊需求可以偏离像素或结构。实施前必须把标准标识、精确差异和关联 ADR 写入该产品的 `docs/GUI_APP_PROFILE.md`；没有 profile/ADR 的中性初始化不得自行偏离。
- compact 组件回归锁定所有表格数值、无折叠控件、无固定 `em/ch` 盒、完整 `aria-label`、NavLink 自身 active 与点击，以及 Logo/图标/文字的全宽居中样式。
- AppShell/结构门禁按 profile 锁定实际 `mode`、Navbar 同源宽度、Navbar padding `0`、fixed/`100dvh`/右边框、可滚动功能区和支持项顺序。
- detailed AppShell 回归锁定默认 `data-mode=detailed`、侧栏宽度与 `data-navbar-width` 均为 `248`、可见名称、`icon-with-label` 和“收起侧栏”按钮；点击身份区父级不得变化，点击 ActionIcon 后两处宽度同步为 `76`、名称节点消失、布局变为 `icon-only` 且偏好值为 `"true"`。
- detailed 模板回归同时覆盖 Tooltip 的 `right`/零延迟语义和重挂载后恢复收起偏好；compact 变更不得改写这些断言。
- GUI 初始化 E2E 在真实本机窗口复核当前模式：详细展开为图标+文字横排，收起后主内容立即扩展且名称进入 Tooltip；同时复核中心线、裁切、active/键盘行为和支持导航只有固定 Settings 项。模板单元测试不能替代可见像素复核。
