# Codex 功能页适配与实机验收

## 证据基线

- 当前基线：Codex `26.715.70719 (5650)`，Chromium `150.0.7871.124`，macOS。
- 已确认：主窗口 URL 为 `app://-/index.html`；新渲染窗口可能通过 `initialRoute` 查询参数携带路由；侧栏的“拉取请求”“站点”“已安排”“插件”是原生按钮。
- 已从当前安装包前端产物确认以下属性锚点：`#pull-request-inbox-search`、`[data-testid='library-file-thumbnail']`、`.automation-row`、`#automation-detail-panel-title`、`#plugins-page-search`、`#plugins-page-manage-search`、`[data-settings-panel-slug]` 与设置搜索框 `role='searchbox'`。
- 安装包静态证据用于补齐未加载状态，不等同于页面已视觉通过；实际注入报告必须逐页列出 `matched`、`missing` 与不可进入原因。

## 通用实施规则

1. 先在 LokiMetis 的“换皮”页明确选择目标实例；不要从 Skill 脚本读取或写入真实 Codex 页面。
2. 页面分类优先使用当前页面独有的 A 级属性，再读取 `initialRoute`、hash 或 pathname；不要依赖中文或英文可见文本分类。
3. CSS 先消费 `html[data-dream-surface]`，再使用本页属性锚点细化。空状态只有页面状态、没有内容锚点时仍应获得通用表面样式。
4. 页面导航导致渲染目标重建时，由 LokiMetis 的有界后台任务重新发现已验证页面；不得从 Skill 复制连接或注入逻辑。
5. 实机验收至少保存：Codex 版本、页面状态、匹配锚点、主区域数量、溢出数量、重复应用结果和卸载残留；无法观察的项目必须明确标为未验证。

## 拉取请求

### 结构与锚点

- 列表搜索：`#pull-request-inbox-search`。
- 详情标签：`[id$='-pull-request-summary-tab']`、`[id$='-pull-request-code-tab']`、`[id$='-pull-request-activity-tab']`。
- 详情面板：对应的 `[id$='-pull-request-*-panel']`，同时保留 `role='tab'` / `role='tabpanel'` 语义。
- 路由兜底：`/pull-requests`，只负责设置页面状态。

### 必做适配与验收

- 分别覆盖收件箱列表、搜索框、状态筛选、左右分栏、详情标签、代码差异、活动评论、审查对话框、空/加载/错误状态。
- 检查长仓库名、长分支名、代码横向滚动、详情面板窄化和分隔线拖动区域；不得用通用卡片预览替代代码审查页。

## 站点

### 结构与锚点

- 页面搜索与空状态标记：`#appgen-site-search`。
- 已加载卡片缩略图：`[data-testid='library-file-thumbnail']`。
- 当前前端模块名为 `appgen-library-page`，不要把模块名写进 CSS 选择器。
- 路由兜底：`/sites`，用于空列表、首次加载和错误状态。

### 必做适配与验收

- 覆盖空状态、站点卡片、缩略图、筛选/排序、创建动作、访问状态、详情或站点设置弹层。
- 缩略图必须有固定比例与 `object-fit`；检查破图、长标题、无站点和卡片密集排列。不得把预览中的虚构卡片当成真实站点验证。

## 已安排

### 结构与锚点

- 列表行：`.automation-row`；这是 B 级具名业务 class，失效时降级到通用列表语义。
- 详情标题：`#automation-detail-panel-title`。
- 详情区域：`[aria-labelledby='automation-detail-panel-title']`。
- 路由兜底：`/automations`。

### 必做适配与验收

- 覆盖建议、任务列表、选中/暂停状态、运行历史、详情分栏、编辑表单、来源插件、立即运行与删除确认弹层。
- 检查时间/重复规则换行、禁用开关、历史空状态和表单校验；不得触发“立即运行”、保存或删除来证明样式有效。

## 插件

### 结构与锚点

- 浏览搜索：`#plugins-page-search`。
- 搜索框的粘性背景层当前带 `.bg-token-main-surface-primary` 与向下延伸的 `::after` 渐隐层；需要露出皮肤背景时，只能在插件页并结合 `:has(#plugins-page-search)` 透明化，失配时保留原生背景。
- 管理搜索：`#plugins-page-manage-search`。
- 分区与卡片：`[id^='plugins-search-']`、`[id^='plugins-marketplace-']`。
- 上述前缀 ID 是分区锚点，可能覆盖搜索栏下方的整段内容；只用于识别、边界色或子级限定，不能直接当插件卡片填充背景、圆角和阴影。
- 路由族：`/skills`、`/skills/manage/plugins` 与插件详情；路由只作空/错误页兜底。

### 必做适配与验收

- 覆盖浏览/管理标签、搜索、已安装与市场卡片、启用开关、详情、加载/空/错误状态和市场管理动作。
- 验收时只读查看开关和动作按钮；不要安装、移除、升级或授权插件。检查长说明、图标缺失、窄屏单列和弹层。
- 实机探测时核对分区锚点的 `tag`、`role`、宽高和视口占比；若锚点接近主内容宽度，任何表面填充必须下沉到其列表项子级。

## 设置

### 结构与锚点

- 设置分区入口：`[data-settings-panel-slug]`，当前版本可见值包括通用、外观、插件、Skills、MCP、浏览器与计算机使用等分区。
- 设置搜索：`input[role='searchbox']`。
- 设置页没有 `<main>`，注入外壳必须回退到 `#root`；左侧栏是 `div.app-shell-left-panel`，样式不得限定为 `aside`。
- 设置主内容位于唯一 `[data-app-shell-focus-area="main"]` 内，当前实色内容表面是其双层子容器；统一主题只在 `settings` 页面状态下覆盖该表面，结构失配时保留原生背景。
- 权限等开关分组当前使用 `section` 内具名 `.border-default` 容器并包含 `[role="switch"]`；只对同时满足这些条件的分组应用卡片背景和分隔线，普通按钮边界不得被当作卡片。
- Windows 最顶部应用菜单由唯一 `[role="menubar"]` 标识，父容器没有稳定属性；可使用 `#root :has(> [role="menubar"])` 做 C 级局部覆盖，菜单项优先使用固定 `application-menu-trigger-*` ID。
- 设置导航：`nav[aria-label]` 仅作为局部语义样式，不按本地化文本判断页面。
- 路由兜底：`/settings`。

### 必做适配与验收

- 覆盖顶部应用菜单、左侧分区导航、搜索、主内容表面、分组卡片、表单行、单选/复选/开关、选择器、外部链接、说明文本、对话框和错误提示。
- 检查导航折叠、长说明、键盘焦点和窄屏；默认仅做只读视觉验收，不改变账户、权限、MCP、浏览器或计算机使用设置。

## 确定性报告格式

每次真实 Codex 调试输出以下 JSON 字段；未取得的字段保留为空并附 `reason`：

```json
{
  "host": {"version": "", "chromium": "", "targetUrl": ""},
  "surface": "pull-requests",
  "detection": {"method": "attribute|initialRoute|hash|pathname", "value": ""},
  "anchors": {"matched": [], "missing": [], "details": []},
  "layout": {"mainRegions": 0, "horizontalOverflow": 0},
  "lifecycle": {"firstInstall": {}, "secondInstall": {}, "cleanup": {}, "screenshot": null},
  "result": "observed|passed|failed|not-reachable",
  "reason": ""
}
```

`observed` 只表示只读 DOM 采集成功。只有 `result=passed` 且锚点、布局和生命周期字段均来自同一目标版本时，才能表述为该页面真实验证通过。
