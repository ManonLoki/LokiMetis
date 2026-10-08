# Mantine 列表页设计标准

`standard_id = mantine-list-view-v2`

本标准命中 React 19.2+、TypeScript、Mantine UI 9.x 中用于检索、浏览或管理重复记录的列表页、数据表格、后台列表和搜索结果页。它与 [Tauri GUI 通用设计标准](tauri_gui.md) 组合使用；本文件只覆盖列表查询、表格呈现、行交互和列偏好。实现或审查使用 `$mantine-list-view`。

## 状态所有权

- `page`、`pageSize`、sort 和已应用 filter 以类型化 URL query 为当前权威。原始 URL 含任一 owned key 时，只采用合法 URL 值并以默认补齐；完全无 owned key 时才读取稳定 `listId` 隔离的 sessionStorage。parser 抛错或快照损坏均安全回退。
- canonical URL 在首次查询前以一次 `replace` 写回，并保留同路由不属于本列表的 query 参数。筛选、排序和 pageSize 变化与 page=1 原子更新；隐藏排序列和越界页的自动纠正只 replace，不污染 Back 历史或产生非法首请求。
- 业务行、总数、错误和请求结果只由 TanStack Query v5 缓存，不写入 Jotai、URL 或 Web Storage。query key 包含稳定 listId、非 PII 结果 scope、规范化筛选、sort、pageSize 和 page。
- 列顺序/显隐是设备偏好。localStorage key 为稳定 app namespace + listId + 非 PII preference scope；schemaVersion 只在 payload 中，升级使用显式 migration 并清理登记的 legacy key。查询或业务行不得混入列偏好。

## 数据与分页契约

- 页码式 offset 是唯一分页模型。pageSize 固定 `10 | 20 | 50 | 100`，兜底 20；Mantine Pagination 的 `total` 是总页数。
- page、total、schemaVersion、数值业务 id 和 offset 都必须是安全整数。列表显式声明 `businessIdType`，成功响应验证所有页业务 id 类型一致、items 数组、pageSize/total/剩余数量、当前可信有效页非空和业务 key 唯一；矛盾响应是契约错误。
- 服务端默认排序必须确定；每个允许的排序字段由 TypeScript 非空联合类型表达，并对同值记录追加稳定业务 id tie-breaker，避免 offset 跨页重复或遗漏。
- 成功、非 fetching、非 placeholder 响应才可纠页：超出非零末页 replace 到末页，totalPages=0 规范为 page=1。loading、error 或 placeholder 不纠正。
- 分页使用 responsive layout 或经 320 CSS px/200% 验证的紧凑 fallback；具备本地化 navigation landmark、首末前后、页码/当前页名称，以及可见 live 结果摘要“第 X–Y 条，共 Z 条”。

## 表格、排序和窄屏

- 表头粘滞只用 Mantine `<Table stickyHeader stickyHeaderOffset={共享布局常量}>`；禁止手写 sticky。`Table.Thead` 必须使用稳定 CSS Module 类，并令其 `th` 以 `background-color: var(--mantine-color-body)` 提供不透明语义背景，避免滚动时数据行透出或与表头重合；不得用 `transparent` 或只适配单一色彩模式的固定颜色。二维内容只在命名、可聚焦、有 focus ring 的 `Table.ScrollContainer` 内横向滚动，页面根不得横向滚动。
- 排序固定 asc→desc→none；排序变化原子 page=1。当前表头同时有准确 `aria-sort` 和可见 Tabler 方向图标，图标 `aria-hidden`；none 时不设置 aria-sort。表格外显示当前排序摘要和清除入口，窄屏隐藏次要列时仍可感知。
- 列 schema 的 columnId 和可访问名称非空，columnId 唯一，每个 sortField 只属于一列；恰好一个 `responsiveRole="primary"`。primary、声明存在的 status/actions 必须设置 `required: true`，并和所有 required 列一样不得设置 `hideBelow`；次要列才可响应式隐藏。
- 唯一 primary 业务单元格为 `th scope="row"`，每行由业务提供非空可理解名称。React key 使用带 id 类型前缀的稳定业务 id，不用 index；行本身不因视觉高亮自动变成链接或按钮。

## 数据行视觉与交互

- 所有真实数据行按 DOM 数据顺序固定浅/深/浅/深交替，不把表头、Skeleton、空态或错误态计入条纹。
- 浅色主题：浅行为 `var(--mantine-color-white)`，深行为 `var(--mantine-color-gray-1)`；深色主题：浅行为 `var(--mantine-color-dark-7)`，深行为 `var(--mantine-color-dark-6)`。
- 鼠标 hover 或任一行内控件 `:focus-within` 时，以 `gray-3`/`dark-4` 提供更强高亮；focus-within 同时使用主题主色内轮廓，forced-colors 使用系统 Highlight。selected 另有语义状态，但 hover/focus 仍覆盖为强反馈。
- 行高亮只提供位置反馈，不能成为状态或选择的唯一编码。行内控件保留自己的可见焦点；没有动作的行不增加 tabIndex 或 click handler。
- 翻页 placeholder 旧行只能只读显示：整行 inert/aria-disabled，选择、分页、批量和所有副作用控件禁用；单元格 renderer 接收并遵守 `interactive=false`。

## 四态和错误

| 状态 | 必需表现 |
|---|---|
| 首次加载/纠页 | 状态文案 + 恰好 pageSize 行结构化 Skeleton，槽位 key 稳定；表格 aria-busy=true |
| 筛选无结果 | 播报“当前条件无结果”并显示真实有效的重置筛选 |
| 全局无数据 | 使用不同文案；只显示业务提供的可执行下一步，不能给无效重置按钮 |
| 错误 | 业务分型失败范围；可重试错误才显示 retry，403/终态契约错误不显示假恢复 |
| 正常/刷新 | 真实数据、结果范围、总数和分页一致；后台进度局部表达 |

TanStack Query v5 的 placeholder 只覆盖新 key pending。换页 error 后显示新页错误，不把旧页冒充新页；同 key background refresh error 才保留当前数据并就近呈现。

## 选择和业务动作

- 每个列表声明 none/current-page/cross-page。启用选择时必须提供可访问批量 action bar、选择摘要和清空入口，不能只有复选框。
- current-page 在查询组合或页码变化时清空；同 key 成功刷新后裁剪响应中已消失 id。cross-page 只保存 id，并声明 base query 变化的清空/保留策略和可见范围。
- 批量动作由服务端重新校验 id、权限和当前状态；客户端选择不是授权。行内控件与父行事件分别测试，不得因冒泡重复执行。

## 列偏好与拖拽

- 读取偏好时验证 payload，显式迁移旧 schema，丢弃未知/重复 id、追加新列并恢复必显列；损坏、迁移异常或版本不匹配时重写默认。提供重置列。
- 列配置只改变视图，不改变后端字段、权限或导出契约。列面板使用有界滚动，避免长列清单溢出视口。
- 拖拽固定用 `@dnd-kit/core` 与 `@dnd-kit/sortable` 的 pointer/keyboard sensor。命名手柄之外提供至少 24px 的左移/右移按钮；两条路径复用更新逻辑并播报新位置。

## 最小验证

- 纯函数：URL/session 优先级、保留无关 query、单次 replace、safe integer、响应矛盾、id/key、稳定偏好 key、迁移/损坏/legacy 清理、列角色、selection prune。
- 组件：排序三态与图标/aria-sort、四态、placeholder inert、结果摘要、分页名称、主行表头、选择/action bar、pointer/keyboard/按钮列移动与 live feedback。
- 视觉/浏览器：浅深条纹、hover/focus/selected/forced-colors，320 CSS px、200% 文本缩放、中英文、长值、Tab/Enter/Space、键盘横向滚动和 Back/Forward/refresh。

完整 Required/Conditional/N/A 判定使用 `$mantine-list-view` 的 `references/checklist.md`；未运行真实浏览器、服务端或 E2E 时如实标记，不以模板契约测试冒充产品验收。
