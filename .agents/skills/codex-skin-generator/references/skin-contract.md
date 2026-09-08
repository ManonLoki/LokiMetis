# 纯主题与兼容皮肤格式契约

## 纯主题包（默认）

新建纯主题目录包含严格元数据 `theme.json`、受限变量表 `theme.css` 和变量引用的 PNG/JPEG 图片。LokiMetis仍统一提供宿主选择器、普通 CSS 属性、页面探测、响应式布局、JavaScript 与卸载生命周期；通过校验的 `theme.css` 只在统一 CSS 后方覆盖固定变量。

```json
{
  "schemaVersion": 3,
  "type": "theme",
  "id": "aurora-console",
  "name": "极光控制台",
  "description": "清晰克制的极光工作台主题",
  "author": "作者名",
  "appearance": {
    "supportedColorModes": ["light", "dark"],
    "requirements": {
      "dark": { "codeThemeId": "codex", "accent": "#339CFF", "contrast": 60 }
    }
  }
}
```

- 顶层只允许可选 `$comment`、`appearance` 以及 `schemaVersion`、`type`、`id`、`name`、`description`、`author`。应用创建的 `id` 不得修改。
- `appearance.supportedColorModes` 只能是不重复且非空的 `light`、`dark`。缺少整个 `appearance` 时按历史契约仅支持浅色，并继续只使用 `theme.css`。
- 新主题生成默认直接声明双模式，不再追问用户是否保持默认；只有用户主动明确选择时才改为仅浅色或仅深色。双模式生成器应自动提供两套可用初始色板，而不是要求用户填写两套色值；该交互规则不改变清单和 CSS 的静态格式。
- 显式声明 `appearance` 后，`theme.css` 只包含 `html.codex-dream-skin` 基础块；每个已声明模式必须分别提供 `theme.light.css` 或 `theme.dark.css`，且文件只能包含对应模式的单一根选择器。存在未声明模式文件同样拒绝。
- 基础块完整声明预览与背景图片、`cover|contain`、横纵位置、圆角、模糊、侧栏/主体/顶部/编辑器/卡片透明度、边框宽度、阴影和底纹强度。图片值只接受带引号的根目录 `url("文件.png|jpg|jpeg")`。
- 显式模式契约下，基础块还要提供完整基础色板，保证进入未支持模式时仍有可用回退。模式 CSS 可以增量覆盖色板、背景图片、背景适配与位置、圆角、模糊、透明度、边框、阴影和底纹变量；模式背景使用 `--skin-background-image: url("文件.png|jpg|jpeg")` 引用主题根目录图片。仍不得覆盖预览图片、声明普通属性、使用 `@import`、`!important` 或重复项。
- `appearance.requirements.light|dark` 只允许用于已声明模式。可选字段为 `codeThemeId`、`accent`、`surface`、`ink`、`contrast`、`opaqueWindows`、`uiFont`、`codeFont` 与 `semanticColors.diffAdded|diffRemoved|skill`；颜色使用 `#RRGGBB`，对比度为 0 到 100。
- 未声明 `appearance` 的历史 v3 主题仍要求原 `theme.css` 同时包含基础、深色与浅色三个完整块，以保证既有 ZIP 无需迁移；其能力标记仍按仅支持浅色处理。
- 位置和透明度为 0% 到 100%；圆角为 0 到 32px，模糊为 0 到 40px，边框为 0 到 3px；变量表非空且不超过 64 KiB。
- 图片必须位于根目录、真实格式与扩展名一致、单文件不超过 16 MiB。目录只能包含 `theme.json`、基础/模式 CSS 及这些 CSS 实际引用的图片，不得包含子目录、脚本、网络资源或其他文件；多项图片变量可以引用同一文件。
- 运行时按基础、浅色覆盖、深色覆盖顺序把已校验变量表追加到统一 CSS 后方，通过 `data-dream-shell` 自动切换。原生层将基础及模式背景经 JSON 载荷传递并分别转换为 Blob URL，以内联同名变量选择当前模式背景；模式未覆盖时回退基础背景，停用时撤销全部 URL、监听器和变量。
- ZIP 可把文件放在根目录或唯一一层包装目录。LokiMetis只导入 `schemaVersion: 3` 新主题与旧版兼容皮肤包；`schemaVersion: 2` 过渡主题需要先转换为 v3。
- ZIP 的 ID 可以与用户库已有资源重复。安装时LokiMetis只改写暂存副本：目录与 ID 依次追加 `_1`、`_2`，显示名称同步追加 `(1)`、`(2)`；原 ZIP 和已有资源保持不变。
- 也可以把完成校验的目录直接发布到当前用户的 `LokiMetis/skins/<id>/`，无需制作 ZIP；应先在同一资源库的点号隐藏草稿目录完成全部文件和正式校验，再一次重命名为最终 ID。LokiMetis会自动发现合法目录，忽略草稿与无效目录。

以下六文件格式仅用于兼容既有皮肤。

## 目录

- 通用皮肤边界
- LokiMetis六文件格式
- `theme.json` 字段
- 照片浮窗扩展
- 载荷占位符
- 注入生命周期
- 安全与可移植性
- 打包检查

## 通用皮肤边界

通用 Codex 皮肤没有官方稳定 DOM API。它通常由宿主或 CDP 将 CSS、JavaScript 与图片数据注入 Codex 主页面。皮肤必须把 DOM 当作会变化的外部接口：探测能力、局部增强、缺失时降级，不得假定某个构建版本的深层结构永久不变。

一个可移植皮肤至少应包含：

- 受根 class 限定的 CSS。
- 幂等的注入入口与完整清理函数。
- 不依赖本机绝对路径的图片资源。
- 对目标 Codex 版本、页面和选择器假设的验证记录。

## LokiMetis六文件格式

生成或交付兼容LokiMetis的旧皮肤时，目录必须恰好提供以下运行文件：

| 文件 | 用途 | 当前限制 |
| --- | --- | --- |
| `theme.json` | 元数据、文本、颜色和布局配置 | UTF-8 JSON，最大 2 MiB |
| `dream-skin.css` | 宿主界面样式 | UTF-8 CSS，最大 2 MiB |
| `renderer-inject.js` | 注入、状态识别和卸载 | UTF-8 JavaScript，最大 2 MiB |
| `qq2007-sky.png` | 预览图与主视觉 | PNG，非空，最大 16 MiB |
| `avatar.png` | 资料/装饰图 | PNG，非空，最大 16 MiB |
| `qqshow.jpg` | 列表/辅助视觉 | JPEG，非空，最大 16 MiB |

固定文件名是当前LokiMetis载荷协议，不代表通用 Codex 皮肤必须采用 QQ 风格。新皮肤可以完全改变视觉，但在该运行格式下不能重命名这些文件。

ZIP 可以把六个文件放在根目录，或放在唯一一层包装目录中。不要加入符号链接、路径穿越条目或额外的嵌套清单。当前导入上限为 256 个条目、64 MiB 总解压体积。

仅为导入历史遗留包，LokiMetis允许 ZIP 缺少 `theme.json`。当其余五个固定运行文件齐全时，LokiMetis会在内部暂存目录生成兼容清单；生成器仍必须输出完整六文件，不能把该兼容行为当作新包规范。

## `theme.json` 字段

最低必需字段：

```json
{
  "schemaVersion": 1,
  "id": "aurora-console",
  "name": "极光控制台",
  "description": "一套克制、清晰的极光工作台皮肤",
  "author": "作者名",
  "image": "qq2007-sky.png",
  "friendCards": {
    "profileImage": "avatar.png",
    "listImage": "qqshow.jpg"
  }
}
```

约束：

- `schemaVersion` 必须为 `1`。
- `id` 长度为 1 到 64，只能包含小写 ASCII 字母、数字、`-` 或 `_`；目录名必须与它一致。
- `name` 与 `author` 去除首尾空白后不能为空，各不超过 80 个字符。
- `description` 去除首尾空白后不能为空，不超过 500 个字符。它是 Skill 创建标准，当前LokiMetis宿主可忽略该扩展元数据。
- `image`、`friendCards.profileImage`、`friendCards.listImage` 在LokiMetis格式中必须使用表内固定文件名。

模板注入脚本使用以下可选字段：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `colors.dark` | 颜色角色对象 | 深色模式的背景、面板、强调和文本令牌 |
| `colors.light` | 颜色角色对象 | 浅色模式的对应令牌 |
| `extensions.photoPanels` | 数组 | 最多两个皮肤扩展照片浮窗 |
| `previewContent` | 对象 | 设计预览中的导航、首页、消息和编辑器文本 |

为兼容旧皮肤，注入逻辑可以接受扁平 `colors.background` 等字段；新模板使用显式 `dark`/`light` 两套角色。每套支持 `background`、`panel`、`panelAlt`、`accent`、`accentAlt`、`text`、`muted` 和 `line`。

自定义字段可以存在，但只有注入脚本主动读取后才会生效。不要仅修改 JSON 并假定 CSS 会自动获得对应值。

## 照片浮窗扩展

`friendCards.profileImage` 与 `friendCards.listImage` 只是LokiMetis固定资源槽位。是否显示、显示在哪里以及表达什么，由 `extensions.photoPanels` 决定。照片浮窗属于皮肤自建 DOM，不是 Codex 原生组件。

完整字段与自然语言映射见 [photo-panels.md](photo-panels.md)。关闭扩展时仍需保留两个固定图片文件；生成器会使用微型占位资源满足六文件协议，但不会创建浮窗 DOM。

## 载荷占位符

LokiMetis构建载荷时会对 `renderer-inject.js` 做字面替换。旧六文件脚本必须各保留其六个传统占位符；统一主题运行时另使用变量表占位符。生成结果中不得残留 `__DREAM_SKIN_`：

| 占位符 | 替换内容 |
| --- | --- |
| `__DREAM_SKIN_CSS_JSON__` | JSON 编码后的完整 CSS 字符串 |
| `__DREAM_SKIN_THEME_CSS_JSON__` | v3 统一主题运行时使用的、已校验包内变量表；旧六文件脚本不需要 |
| `__DREAM_SKIN_ART_JSON__` | PNG 主图 data URL |
| `__DREAM_SKIN_AVATAR_JSON__` | PNG 资料图 data URL |
| `__DREAM_SKIN_FRIENDS_JSON__` | JPEG 辅助图 data URL |
| `__DREAM_SKIN_THEME_ASSETS_JSON__` | v3 纯主题已校验模式图片的 data URL 映射；旧六文件脚本不需要 |
| `__DREAM_SKIN_THEME_JSON__` | `theme.json` 对象字面量 |
| `__DREAM_SKIN_VERSION_JSON__` | 宿主定义的皮肤载荷版本字符串 |

不要把这些占位符写入注释示例，否则替换后可能产生无意义的大字符串或语法错误。

## 注入生命周期

为了与宿主卸载逻辑互操作，采用以下公共状态：

- 根 class：`codex-dream-skin`。
- 样式节点：`#codex-dream-skin-style`。
- 装饰根节点：`#codex-dream-skin-chrome`。
- 运行状态：`window.__CODEX_DREAM_SKIN_STATE__`。
- 禁用标记：`window.__CODEX_DREAM_SKIN_DISABLED__`。

状态对象至少暴露 `cleanup()`。注入新版本前先调用旧状态的清理逻辑或逐项停止旧资源。清理应覆盖：

1. `MutationObserver`、计时器、`requestAnimationFrame`、媒体查询和事件监听器。
2. 根 class、状态属性和内联 CSS 变量。
3. 样式节点、自建 DOM 和添加给原生节点的辅助 class。
4. 由 data URL 创建的所有 Blob URL。
5. 全局状态引用。

## 宿主兼容适配器

LokiMetis宿主从皮肤载荷 `1.2.0` 起可在注入前运行独立版本的兼容适配器。适配器不是皮肤包的一部分，不会改写六文件目录或 ZIP；旧皮肤仍可继续使用原有 class 选择器。

- 兼容状态为 `window.__BIFANG_CODEX_SKIN_COMPAT__`，当前适配器版本为 `5`。Codex 主表面优先使用唯一 `main[data-app-shell-main-surface]`，并保留唯一 `#root main` 的旧版回退，三条既有兼容规则及其规则标识保持不变。WorkBuddy 使用独立宿主适配器：只接受已验证的 WorkBuddy body 标记，把外壳、侧栏、主区、标题栏、详情区映射到自有别名，并从聊天根内的稳定工具栏标记与唯一语义编辑器派生 composer；不把 WorkBuddy 伪装成 Codex DOM。两类适配器都会释放已经脱离文档的自有别名节点，并忽略不包含兼容锚点的普通内容变化。
- 适配器只在稳定候选唯一时补充 `main-surface`、`app-header-tint` 与 `composer-surface-chrome`。
- 原生已有的旧 class 归宿主所有；清理只删除适配器自己添加的 class。
- 宿主可能在首次水合时重写稳定候选的 `class`；适配器会防抖恢复自有别名，皮肤不应自行增加全局 DOM 监听来补救该时序。
- 模糊、缺失或执行失败时跳过并报告稳定规则 ID，不允许根据文字、几何或构建哈希猜测，也不允许补写 `role` 或 ARIA。
- 皮肤不得依赖或修改兼容状态；需要新宿主锚点时应先维护组件映射，由受审查的应用版本发布规则。

## 安全与可移植性

- 只在经过宿主校验的 Codex 主页面执行。通用宿主应同时核对 `app://-/index.html`、标题、`#root` 和至少一个 Codex shell 标记。
- 不读取或发送会话内容，不加入遥测、远程脚本、凭据或外部服务调用。
- 不使用 `eval`、`new Function` 或字符串形式的定时器。
- 写入用户提供文本时使用 `textContent`；需要 CSS 字符串时使用 `JSON.stringify` 结果。
- 不持久修改 Codex 安装包。皮肤应可通过当前页面清理或重载恢复。
- Skill 目录、模板和生成结果不得引用创建机器的绝对路径。

## 打包检查

交付前确认：

- 六个文件存在、非空且格式匹配。
- 清单 ID 与目录名一致。
- JavaScript 可解析，CSS 花括号平衡。
- 六个载荷占位符齐全，没有未知占位符。
- 连续注入两次只有一个样式节点和一个装饰根节点。
- 卸载后公共状态、Blob URL 和自建节点均清除。
- ZIP 解压后 `theme.json` 位于根目录或唯一一层目录。
