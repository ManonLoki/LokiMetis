# macOS 可变字体兼容层

官方 gpui-pre-macos 0.3.8 的全部 17 文件原样保留于 upstream，含原 Apache-2.0 许可和归档 Cargo.lock。来源、归档摘要和完整文件清单见 provenance.json 与 source-content.sha256；固定源清单 SHA256 为 4b29969a342cfbc80229ca6690ec4f29a1954ddd50470a0106d794e669edf406。

共享标准库生成器校验原文件集、逐文件 SHA、唯一替换片段与生成结果 SHA。build.rs 只生成 OUT_DIR/gpui-patched，不改写 upstream。Cargo 公开依赖与特征保持同版本原包解析值，只有 build/lib 入口改为本地包装。

本地补丁为请求字重建立真实 CoreText wght 实例；选择与 CTRun 反查统一使用实际轴、feature、cascade 和矩阵身份。缓存键只含纯 Rust 值，未知属性通过临时完整 CFEqual 比较已有字体，不缓存 CFType、不新增 unsafe Send/Sync、不用 CFHash 代替身份。原生字体直接 clone，避免经 CGFont 往返丢失上下文；原 shaping/raster 算法保持原样。

test-fixtures 的七份字体及 OFL 受固定清单与构建期完整文件集校验。来源见 fixture-provenance.json。项目不安装系统字体。独立原生验证的同一 19 项断言为旧实现 12 通过/7 失败、补丁后 19 通过；实际输出源码、构建脚本和测试二进制身份已分别核对。项目集成与 CU 证据另记于忽略的 release/gpui-parity，不能以单元结果宣称体验已完全一致。

维护检查：

```sh
node vendor/gpui-patch-support/verify-source.mjs vendor/gpui-pre-macos
cargo test --manifest-path vendor/gpui-pre-macos/Cargo.toml --lib \
  --features font-kit,test-support \
  --config "patch.crates-io.accesskit_macos.path=\"$PWD/vendor/accesskit_macos\"" \
  --config "patch.crates-io.gpui-pre.path=\"$PWD/vendor/gpui-pre\"" \
  --config "patch.crates-io.gpui-pre-apple.path=\"$PWD/vendor/gpui-pre-apple\"" \
  --config "patch.crates-io.gpui-pre-macos.path=\"$PWD/vendor/gpui-pre-macos\"" \
  --config "patch.crates-io.gpui-pre-windows.path=\"$PWD/vendor/gpui-pre-windows\"" \
  --config "patch.crates-io.gpui-pre-wgpu.path=\"$PWD/vendor/gpui-pre-wgpu\"" \
  variable_font -- --test-threads=1
```

以上测试从项目根运行；包装 crate 在工作区外，独立回归显式绑定同一组项目路径补丁，正式应用沿根 Cargo 补丁解析。没有证明同一 PostScript 名来自不同文件、自动 opsz、其它变化轴或任意额外 descriptor 属性；首次缓存未命中与未知属性比较均扫描已有字体，成本为 O(已加载字体数)。本次只启用 macOS 原样 Borrowed WOFF2 路径，Windows 暂保留静态字重。上游同时修正可变字重与上下文反查并通过同回归后，可移除此兼容层。
