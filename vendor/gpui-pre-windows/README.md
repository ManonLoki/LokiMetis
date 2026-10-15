# 本地 GPUI 圆角视口兼容层

本目录以 `provenance.json` 记录的官方 0.3.9 `.crate`（Zed revision 93d7456）为完整不可变来源。
`upstream/` 保留原文件、许可证与目录关系；`source-content.sha256` 覆盖全部原文件，路径相对 upstream 根。

人工维护入口为 Cargo.toml、src/lib.rs、build.rs、patches/ 和本说明。
共享标准库生成器同时验证固定清单 SHA、每个原文件 SHA、完整文件集、唯一替换片段和生成结果 SHA。
补丁仅写入 Cargo OUT_DIR/gpui-patched，不编辑 upstream，也不把整个 vendor 目录排除于工程门禁。

Windows 原生命中同时核对当前 `WS_THICKFRAME`，动态锁定桌宠后顶部和最大化区域不能沿用创建期缓存。
首次外部原生显示会消费隐藏创建的初始位置，后续 GPUI 激活不重放过期几何；生成文件均由固定 SHA 校验。

`PolychromeSprite` 末尾新增 16 字节圆角视口 bounds；原图片矩形、祖先矩形 mask、UV、透明度和图集键保持独立。
原 paint_image 与 emoji 沿用原几何，新入口 ImageRoundedClip/paint_image_with_rounded_clip 显式保存原始 viewport。
Metal、HLSL、WGSL/WebGL 必须同时使用这一 ABI；macOS 验证不能替代 Windows 实际渲染验证。

维护验证：

```sh
node vendor/gpui-patch-support/verify-source.mjs vendor/gpui-pre vendor/gpui-pre-apple vendor/gpui-pre-windows vendor/gpui-pre-wgpu
rustc --edition=2024 --test vendor/gpui-patch-support/generator.rs -o /tmp/loki-gpui-generator-tests
/tmp/loki-gpui-generator-tests --test-threads=1
```

生成验证不是产品候选验收；真实 Window 与 GPU 像素测试须使用相应已启用 test-support 的目标。
