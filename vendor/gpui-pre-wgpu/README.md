# GPUI WGPU 生成补丁

`upstream/` 完整保存官方 `gpui-pre-wgpu-0.3.8.crate` 的 14 个原文件，不允许手工编辑。归档 SHA-256 为 `439c5421cafe1a7cf68d0c0ae5cc82b26fad6b0e6345ac98d20acaf1f2463914`；来源及逐文件摘要由 `provenance.json`、`source-content.sha256` 固定。

`build.rs` 使用相邻 `gpui-patch-support` 的标准库生成器校验原包与清单、复制完整目录、逐个应用唯一匹配模板，并核对生成文件摘要。编译只通过 `src/lib.rs` 引入 `OUT_DIR/gpui-patched/src/gpui_wgpu.rs`，保持嵌套模块和相对 shader 路径。原包没有构建脚本或入口根属性，`root-prefix.before` 因此为空。官方依赖和 features 保持原值；官方 benchmark 指向 `upstream/benches/layout_line.rs`。

| 补丁模板 | 生成文件 | 目的 |
|---|---|---|
| `polychrome-layout` | `src/shaders.wgsl` | 在实例末尾增加 `rounded_clip_bounds`，与 Rust、Metal、DirectX ABI 同步 |
| `polychrome-distance` | `src/shaders.wgsl` | 以独立视口计算圆角距离，保留图片纹理坐标 |
| `webgl-polychrome-decoder` | `src/shaders_webgl.wgsl` | 将步幅由 24 改为 28 个字，并在末尾读取视口 |
| `polychrome-word-stride` | `src/wgpu_renderer.rs` | 同步原有 Rust ABI 步幅回归 |
| `shader-test-naga-import` | `src/wgpu_renderer.rs` | 用现有 wgpu 再导出运行原有 Naga shader 测试 |

每个模板由同名 `.before` / `.after` 构成；内容零匹配或多匹配均拒绝生成。升级时完整替换官方原包、重新固定来源清单，逐一复核模板与生成摘要。生成器、模板、shim、测试及本说明属于人工维护文件。

`tests/rounded_clip.rs` 是独立 integration target：解析并以无扩展能力的 Naga Validator 校验实际生成的 storage/WebGL 两种 shader，用成员偏移和结构大小对照 Rust `PolychromeSprite`。官方归档缺少 cosmic_text_system 单元测试引用的 Lilex/IBMPlexSans 字体，因此完整 `--lib` 测试仍因 fixture 缺失无法编译；本 target 不替代字体，也不禁用其断言。shader/ABI 验证不等于 GPU 像素验收；跨后端真实渲染由当前任务另行验证。
