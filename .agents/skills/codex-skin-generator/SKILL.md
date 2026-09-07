---
name: codex-skin-generator
description: 创建、改造或静态验证 LokiMetis 可管理的 Codex 桌面端纯主题与兼容皮肤。默认生成严格元数据、受限 CSS 变量与本地图片组成的纯主题包，由 LokiMetis GUI 统一提供宿主 CSS、注入、实例选择与恢复；也可维护旧六文件皮肤。
---

# Codex 皮肤生成器

## 读取顺序

1. 始终读取 [references/discovery.md](references/discovery.md)、[references/color-and-preview.md](references/color-and-preview.md) 与 [references/component-map.yaml](references/component-map.yaml)。
2. 修改、调试或验收拉取请求、站点、已安排、插件或设置时，必须读取 [references/feature-surfaces.md](references/feature-surfaces.md)，按页面独立维护锚点、状态和确定性报告。
3. 生成 LokiMetis 纯主题或兼容六文件皮肤时读取 [references/skin-contract.md](references/skin-contract.md)。
4. 用户提到照片窗、资料卡、相框、贴图、悬浮图片或左右装饰窗时读取 [references/photo-panels.md](references/photo-panels.md)。这类窗口是皮肤注入的扩展，不是 Codex 原生组件。
5. 进入实现或验收时读取 [references/design-and-qa.md](references/design-and-qa.md)。

## 启发式交互

1. 第一轮只取得四项必填输入：主题名称、主题说明、作者、期望风格。用户已提供的项不要重复询问；使用简洁的四项编号格式，不在这一轮询问主色、图片路径或颜色模式。
2. 收齐四项后单独追问用户选择“使用现有图片背景”还是“AI 生成背景”。选择现有图片时再索要绝对路径；选择 AI 时使用可用的图片生成能力，根据期望风格直接生成适合大面积裁切的背景。图片生成能力不可用时如实说明并索要现有图片，不得用占位图冒充成品。
3. 新版默认直接生成浅色和深色双模式，不再要求用户确认。只有用户主动明确要求单模式时才改为浅色或深色；不得根据效果图或系统当前模式静默改成单模式。
4. 双模式默认共用一张背景，不主动询问是否需要不同图片。只有用户明确提出浅色与深色使用不同图片时，才分别接收路径或生成图片，并在对应模式 CSS 中引用。
5. 从期望风格和背景证据推断主色、颜色角色、层级、密度、几何语言、图片裁切、字体气质、动效强度和组件覆盖范围，不要求用户逐项给色值。
6. 输出简短的“推断设计简报”和推荐配色；逐项标注 `明确`、`推断` 或 `默认`，解释颜色角色与对比度。不要先抛出完整问卷。
7. 除四项必填输入和背景来源选择外，只追问会显著改变实现且无法可靠推断的问题，一轮最多三个。其余使用可逆默认值并继续。
8. 用户确认方向或没有高影响缺口后直接实现；不要把低风险细节变成阻塞项。

## 实现流程

1. 默认生成 `schemaVersion: 3` 浅色与深色双模式纯主题：`theme.json` 保存严格元数据与颜色模式声明，`theme.css` 保存基础白名单变量和共享本地图片引用，`theme.light.css`、`theme.dark.css` 按声明模式提供增量变量。双模式先自动建立两套完整可用、语义一致且不是简单反色的配色；不要要求用户先给出两套色值。模式 CSS 支持高级的独立背景图片引用，但默认生成器继续共用基础背景且不主动提示；只有用户明确要求不同模式使用不同图片时才采用。用户明确选择单模式时只声明并生成对应模式。宿主选择器、普通属性和 JavaScript 仍由LokiMetis统一运行时负责。仅在用户明确要求旧六文件格式时，才按 `component-map.yaml` 维护包内选择器和脚本。
2. 区分三类对象：`codex-native` 是原生 Codex DOM，`skin-state` 是注入脚本添加到原生节点的状态，`skin-extension` 是皮肤自行创建的 DOM。
3. 按固定顺序选择锚点：A 级 `data-*`、ARIA、`role`、固定 id 与 HTML 属性；B 级稳定路由或具名 class；C/D 级组合、文本或结构。只有属性标记无法覆盖空/错误状态时才用路由，C/D 级只用于局部增强并记录降级行为。
4. 使用自有 class 或 `data-*` 表达组合状态；CSS 消费状态，不重复复杂 DOM 探测。
5. 保持注入幂等与卸载对称：样式根、扩展根、观察器、监听器、计时器、Blob URL、临时 class 和属性都必须可清理。
6. 扩展组件默认不拦截输入、不遮挡 Codex 工作流；用户要求交互时再增加键盘、焦点和可访问语义。
7. 同时处理明暗模式、窄/宽窗口、长内容、弹层、缩放和 `prefers-reduced-motion`。
8. 在真实 Codex 注入前生成设计预览，逐项切换首页、会话、拉取请求、站点、已安排、插件、设置和 other，检查配色、裁切、密度、通用控件和扩展位置；预览不能替代运行验证。
9. 功能页不能只依靠通用元素换色：为五类页面分别处理参考资料列出的列表/卡片、分栏、标签、表单、空错状态和弹层，并将选择器登记到组件 Map。
10. 不直接连接 Codex 调试端口或临时注入；真实宿主选择、启用、停止与恢复只在 LokiMetis 的“换皮”页面中执行。
11. 稳定锚点不等于卡片粒度；给前缀 ID、分区属性或布局容器添加背景、圆角和阴影前，先核对实机标签、角色与几何尺寸，并让预览 DOM 保持相同父子层级。
12. 最终目录通过正式校验后，提醒用户回到 LokiMetis 的“换皮”页面刷新资源库、选择目标 Codex 实例并启用。不得通过脚本调用 LokiMetis、连接 CDP、修改 Codex 设置，或根据进程顺序和账号标签自行推断目标。

## 轻量起点

也可以先在 LokiMetis“换皮”页点击“创建主题”，填写名称和作者后打开应用建立的用户主题目录。请保留 `theme.json` 中应用生成的 `id`；共享图片和基础视觉参数在 `theme.css` 修改，显式模式覆盖按格式契约放入对应模式 CSS。工程蓝图图片标注了预览与背景槽位、实际像素尺寸、宽高比和安全区，正式主题应替换为实际素材，并保持变量引用和真实图片格式一致；完成后可从用户资源卡片导出 ZIP。

使用生成器创建纯主题。背景支持 PNG 或 JPEG；纯主题只有 `preview` 与 `background` 图片槽位，不生成浮动装饰窗：

```bash
node scripts/create_skin.mjs \
  --output /path/to/output \
  --id aurora-console \
  --name "极光控制台" \
  --description "一套克制、清晰的极光工作台皮肤" \
  --author "作者名" \
  --color-modes both \
  --hero-image /path/to/background.png
```

名称、说明、作者和背景图均为生成命令必填；背景图应来自用户在第二阶段选择的现有文件或 AI 生成结果。`--color-modes` 可选，默认 `both`，也接受 `light` 或 `dark`。生成后只调整基础与已声明模式 CSS 中已有白名单变量的值；不得添加其他选择器、普通 CSS 属性、`@import`、`!important`、JavaScript、网络 URL、自由布局或未声明文件。通过校验的变量表会按基础与模式顺序追加到LokiMetis统一 CSS 后方，依靠 cascade 覆盖默认值；照片面板只属于明确选择旧六文件格式时的兼容扩展。

生成可交互预览：

```bash
node scripts/render_preview.mjs /path/to/skin --output /path/to/skin-preview.html
```

若环境提供浏览器或截图工具，打开预览并检查桌面与窄屏；否则交付 HTML 并如实说明未生成截图。

## 验证与交付

```bash
node scripts/validate_skin.mjs /path/to/skin
node scripts/audit_component_map.mjs /path/to/skin
```

验证器会按清单版本自动区分纯主题和旧六文件皮肤。所有 Skill 脚本只依赖 Node.js 内置模块，不要求 Python 或第三方包。

旧六文件皮肤的 `dream-skin.css` 还会经过静态性能门禁，规则来自真实 Codex 上的样式重算实测。以下写法会直接判为错误，必须改写：滚动固定背景（`background-attachment: fixed` 或 `background` 简写中的 `fixed`）、`transition: all` 与 `transition-property: all`、以通用选择器 `*` 作为规则主体。`@media (prefers-reduced-motion: reduce)` 下的通用无障碍重置是推荐写法，不受此限。`backdrop-filter`、`filter: blur()`、`box-shadow`、`:has()` 和以裸通用标签或裸属性结尾的宽泛选择器超过阈值时给出带实际数量的警告，需要按皮肤视觉意图复核后再决定是否收敛。校验器每次都会输出性能计数摘要，可直接用于审计既有皮肤。随后由用户在 LokiMetis GUI 中选择目标 Codex 实例，逐项验证首页、会话、拉取请求、站点、已安排、插件、设置、弹层、明暗模式和窄/宽窗口。不能访问的页面必须记录原因，不得用另一个功能页代替。重复注入不得产生重复节点；卸载后不得残留主题状态。

最终校验通过后提醒用户在 LokiMetis GUI 中启用；不得从 Skill 脚本发起应用。交付时说明输入证据、配色依据、推断与默认项、原生组件映射、启用的扩展、预览路径、脆弱选择器、实际验证范围和未验证项。静态检查或设计预览通过不得表述为真实 Codex 视觉与交互验证通过。
