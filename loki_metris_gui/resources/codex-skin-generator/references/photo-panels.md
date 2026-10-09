# 照片浮窗扩展

纯主题不支持照片浮窗或自由布局字段，也不会创建 `#codex-dream-skin-chrome`。早期纯主题包中的 `images.profile`、`images.gallery` 只作为读取与导出兼容字段保留，统一运行时不会显示。以下结构化配置仅适用于旧六文件兼容皮肤。

## 身份边界

两个照片浮窗是皮肤注入到 `#codex-dream-skin-chrome` 的 `skin-extension`，Codex 原生界面没有对应组件。不得为它们寻找或声称存在 Codex 原生选择器。

默认用途：

- `profile`：人物、头像、资料卡、专辑封面或主视觉小窗。
- `gallery`：海报、好友列表、歌词、作品集或辅助图片窗。

这些名称只是资源槽位，不限定视觉风格。用户自然语言决定标题、说明、边框、位置、尺寸和出现页面。

## 自然语义映射

| 用户描述 | 配置映射 |
| --- | --- |
| “左上角放我的照片” | `assetSlot: profile`、`anchor: left-top` |
| “下面再放一张海报” | `assetSlot: gallery`、`anchor: left-bottom` |
| “完整显示，不要裁脸” | `fit: contain` |
| “铺满相框” | `fit: cover`，先确认允许裁切 |
| “只在聊天时出现” | `visibleOn: [chat]` |
| “首页也显示” | `visibleOn: [home, chat]` |
| “宽屏才显示” | 设置 `minViewportWidth`，默认 `1280` 或按布局推断 |
| “像贴纸，没有标题” | `title: ""`、`caption: ""`，CSS 使用无标题框架 |
| “复古资料卡” | 保留标题与说明，在 CSS 中实现框架；不要改变组件身份 |

## 结构化配置

在 `theme.json` 中最多配置两个面板：

```json
{
  "extensions": {
    "photoPanels": [
      {
        "id": "profile",
        "enabled": true,
        "assetSlot": "profile",
        "title": "关于我",
        "caption": "保持专注",
        "anchor": "left-top",
        "offset": { "x": 24, "y": 48 },
        "size": { "width": 304, "height": 360 },
        "fit": "contain",
        "visibleOn": ["chat"],
        "minViewportWidth": 1450
      }
    ]
  }
}
```

字段约束：

| 字段 | 值 | 说明 |
| --- | --- | --- |
| `id` | 唯一小写标识 | 用于 `data-panel-id`，不是 Codex ID |
| `enabled` | 布尔值 | `false` 时不得创建 DOM |
| `assetSlot` | `profile` 或 `gallery` | 分别映射 `avatar.png`、`qqshow.jpg` 的 Blob URL |
| `anchor` | `left-top`、`right-top`、`left-bottom`、`right-bottom` | 相对于 Codex 主内容外壳定位 |
| `offset.x/y` | 0 到 2000 | 对应边缘偏移，单位 px |
| `size.width/height` | 80 到 1200 | 稳定尺寸，单位 px |
| `fit` | `contain` 或 `cover` | 图片适配策略 |
| `visibleOn` | `home`、`chat`、`pull-requests`、`sites`、`automations`、`plugins`、`settings`、`other` 的非空数组 | 页面状态由注入脚本判断 |
| `minViewportWidth` | 320 到 4000 | 低于阈值时隐藏，避免遮挡 |
| `title/caption` | 字符串 | 通过 `textContent` 写入；空值不渲染对应节点 |

## 实现不变量

- 扩展根使用 `pointer-events: none` 和 `aria-hidden="true"`；纯装饰图片不进入键盘顺序。
- 面板节点使用 `.skin-photo-panel`，通过 `data-panel-id`、`data-anchor`、`data-fit` 和 `data-visible-on` 暴露状态。
- 图片使用 `<img>` 与 Blob URL，不把 data URL 拼入 HTML。
- `enabled: false`、资源为透明占位或视口不足时不应影响 Codex 布局。
- 默认不修改会话宽度。若面板占用工作区，AI 必须为对应断点显式预留空间并验证编辑器、滚动区和设置页。
- 设置页归为 `settings`，无法识别的页面归为 `other`；除非用户明确要求，不在功能页、设置页或未知页面显示扩展。
- 用户要求按钮、搜索、折叠等交互时，将其视为新的交互扩展，补充可访问语义，不复用纯照片面板协议硬塞交互。

## 验证

1. 关闭两个面板，确认扩展根为空且 Codex 布局不变。
2. 分别只启用一个面板，确认资源槽位、标题和锚点正确。
3. 同时启用两个面板，检查互不重叠且不遮挡原生操作。
4. 切换首页、会话、拉取请求、站点、已安排、插件、设置和 other，逐项检查 `visibleOn`。
5. 跨越 `minViewportWidth`，确认隐藏和恢复不重复创建节点。
6. 重复注入与卸载，确认图片 Blob URL 和面板 DOM 全部清理。
