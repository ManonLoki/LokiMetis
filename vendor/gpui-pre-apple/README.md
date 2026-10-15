# 本地 GPUI 圆角视口兼容层

本目录以 `provenance.json` 记录的官方 0.3.9 `.crate`（Zed revision 93d7456）为完整不可变来源。
`upstream/` 保留原文件、许可证与目录关系；`source-content.sha256` 覆盖全部原文件，路径相对 upstream 根。

人工维护入口为 Cargo.toml、src/lib.rs、build.rs、patches/ 和本说明。
共享标准库生成器同时验证固定清单 SHA、每个原文件 SHA、完整文件集、唯一替换片段和生成结果 SHA。
补丁仅写入 Cargo OUT_DIR/gpui-patched，不编辑 upstream，也不把整个 vendor 目录排除于工程门禁。

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


## macOS 可变字体（0.3.9 自 gpui-pre-macos 迁入）

上游 0.3.9 把 CoreText 文本系统移入本包并改名 `AppleTextSystem`；原 `font-native-identity-state`、`font-requested-weight-instance`、`font-native-registration-and-runs` 三份模板只改类型名后随之迁入，由 `src/font_patches.rs` 登记，`variable_font.rs`、`semantic_identity.rs`、`descriptor_identity.rs` 及回归、`test-fixtures/`、`fixture_verify.rs` 一并迁入。

本地补丁为请求字重建立真实 CoreText wght 实例；选择与 CTRun 反查统一使用实际轴、feature、cascade 和矩阵身份。缓存键只含纯 Rust 值，未知属性通过临时完整 CFEqual 比较已有字体，不缓存 CFType、不新增 unsafe Send/Sync、不用 CFHash 代替身份。原生字体直接 clone，避免经 CGFont 往返丢失上下文；原 shaping/raster 算法保持原样。

test-fixtures 的七份字体及 OFL 受固定清单与构建期完整文件集校验。来源见 fixture-provenance.json。项目不安装系统字体。独立原生验证的同一 19 项断言为旧实现 12 通过/7 失败、补丁后 19 通过；实际输出源码、构建脚本和测试二进制身份已分别核对。项目集成与 CU 证据另记于忽略的 release/gpui-parity，不能以单元结果宣称体验已完全一致。

维护检查：

```sh
cargo test --manifest-path vendor/gpui-pre-apple/Cargo.toml --lib \
  --features font-kit,test-support,parent-rounded-clip \
  --config "patch.crates-io.accesskit_macos.path=\"$PWD/vendor/accesskit_macos\"" \
  --config "patch.crates-io.gpui-pre.path=\"$PWD/vendor/gpui-pre\"" \
  --config "patch.crates-io.gpui-pre-apple.path=\"$PWD/vendor/gpui-pre-apple\"" \
  --config "patch.crates-io.gpui-pre-macos.path=\"$PWD/vendor/gpui-pre-macos\"" \
  --config "patch.crates-io.gpui-pre-windows.path=\"$PWD/vendor/gpui-pre-windows\"" \
  --config "patch.crates-io.gpui-pre-wgpu.path=\"$PWD/vendor/gpui-pre-wgpu\"" \
  -- --test-threads=1
```

以上测试从项目根运行；包装 crate 在工作区外，独立回归显式绑定同一组项目路径补丁，正式应用沿根 Cargo 补丁解析。没有证明同一 PostScript 名来自不同文件、自动 opsz、其它变化轴或任意额外 descriptor 属性；首次缓存未命中与未知属性比较均扫描已有字体，成本为 O(已加载字体数)。本次只启用 macOS 原样 Borrowed WOFF2 路径，Windows 暂保留静态字重。上游同时修正可变字重与上下文反查并通过同回归后，可移除此兼容层。
