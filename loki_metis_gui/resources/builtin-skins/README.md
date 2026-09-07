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

Tauri 只打包配置中明确登记的皮肤子目录。应用运行时直接从只读 bundle 资源目录扫描这些皮肤，不复制到用户资源库，也不向界面提供打开目录或删除操作。

内置皮肤直接使用文件夹，不使用 ZIP。此目录中的 README、ZIP 和其他开发文件不会进入应用包；用户 ZIP 皮肤允许使用相同 ID，并通过来源字段与内置皮肤区分。
