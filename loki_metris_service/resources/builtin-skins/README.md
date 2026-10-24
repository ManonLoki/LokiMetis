# 内置皮肤资源目录

此目录用于发行分支收录经过审核的内置纯主题与兼容皮肤。每项资源使用一个独立子目录。纯主题结构为：

```text
resources/builtin-skins/
└── <theme-id>/
    ├── theme.json
    ├── theme.css
    └── 变量引用的 PNG/JPEG 图片
```

兼容皮肤继续使用六文件结构：

```text
resources/builtin-skins/
└── <skin-id>/
    ├── theme.json
    ├── dream-skin.css
    ├── renderer-inject.js
    ├── qq2007-sky.png
    ├── avatar.png
    └── qqshow.jpg
```

子目录名必须与 `theme.json` 的 `id` 一致。新内置纯主题使用 `schemaVersion: 3` 严格元数据和受限 CSS 变量表，宿主样式与注入器仍由应用统一运行时提供；已有 `schemaVersion: 2` 主题继续兼容。兼容皮肤使用 `schemaVersion: 1`。三种格式都必须通过 Rust 资源校验、大小限制和注入生命周期检查。

GPUI 按根 `packaging/gpui.json` 的显式资源映射递归携带整个应用 resources 目录。运行时只扫描内置皮肤目录下通过清单与资源校验的皮肤子目录；它们来自只读包内资源，不复制到用户库，也不提供打开目录或删除操作。

内置皮肤直接使用文件夹，不使用 ZIP；此目录中的 README 等普通文件也会按资源映射进入应用包，但不作为可应用皮肤。不要依赖运行时目录筛选来排除打包内容。用户 ZIP 皮肤允许使用相同 ID，并通过来源字段与内置皮肤区分。
