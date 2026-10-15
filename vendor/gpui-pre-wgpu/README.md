# GPUI WGPU 生成补丁

`upstream/` 完整保存官方 `gpui-pre-wgpu-0.3.9.crate` 的 14 个原文件，不允许手工编辑。归档 SHA-256 为 `5b1cdb26c1503bf9662ee8fbc5ab0b44b2deeee4b5c12dc67491ba6fb5fc544b`；来源及逐文件摘要由 `provenance.json`、`source-content.sha256` 固定。

`build.rs` 使用相邻 `gpui-patch-support` 的标准库生成器校验原包与清单、复制完整目录、逐个应用唯一匹配模板，并核对生成文件摘要。编译只通过 `src/lib.rs` 引入 `OUT_DIR/gpui-patched/src/gpui_wgpu.rs`，保持嵌套模块和相对 shader 路径。原包没有构建脚本或入口根属性，`root-prefix.before` 因此为空。官方依赖和 features 保持原值；官方 benchmark 指向 `upstream/benches/layout_line.rs`。

| 补丁模板 | 生成文件 | 目的 |
|---|---|---|
| `polychrome-layout` | `src/shaders.wgsl` | 在实例末尾增加 `rounded_clip_bounds`，与 Rust、Metal、DirectX ABI 同步 |
| `polychrome-distance` | `src/shaders.wgsl` | 以独立视口计算圆角距离，保留图片纹理坐标 |
| `webgl-polychrome-decoder` | `src/shaders_webgl.wgsl` | 将步幅由 24 改为 28 个字，并在末尾读取视口 |
| `polychrome-word-stride` | `src/wgpu_renderer.rs` | 同步原有 Rust ABI 步幅回归 |
| `shader-test-naga-import` | `src/wgpu_renderer.rs` | 用现有 wgpu 再导出运行原有 Naga shader 测试 |
| `test-font-regular-family` / `test-font-bold-family` | `src/cosmic_text_system.rs` | 仅修改原有 `cfg(test)` 家族发现测试的字体 include 路径 |
| `test-font-ibm-constant` / `test-font-lilex-constant` | `src/cosmic_text_system.rs` | 仅修改原有 `cfg(test)` 字体常量的 include 路径 |

每个模板由同名 `.before` / `.after` 构成；内容零匹配或多匹配均拒绝生成。升级时完整替换官方原包、重新固定来源清单，逐一复核模板与生成摘要。生成器、模板、shim、测试及本说明属于人工维护文件。

`test-fixtures/` 独立补齐官方归档未包含的三个字体及两份原样许可，来自同一 Zed revision `93d745668805029476e6d7fbb19652116fed51de` 的 `assets/fonts/lilex/`、`assets/fonts/ibm-plex-sans/`（与此前 0.3.8 所用 279fe07 逐字节相同，2026-10-15 复核）。五个文件共 596542 字节，逐项来源 URL、官方长度、Git blob SHA-1 与实际 SHA256 见 `test-fixtures-provenance.json`；字体和许可字节先通过官方元数据核对，再迁入项目。两套字体均为 SIL OFL 1.1，IBM 保留名称 “Plex”；完整原样许可随字体保留，不进行全局字体安装。

`test-fixtures-content.sha256` 的固定 SHA256 为 `9fd0d42f6f6fcd62a12d6968f37d16d76b04aa048d1dbfe01814181526b8f81b`。构建与独立 Node 校验均要求这五份原样字节完整匹配，拒绝缺失、修改、额外文件、父目录或文件链接及清单漂移。夹具清单独立于原包 14 项来源登记；生成副本只将四处测试 include 改为 `CARGO_MANIFEST_DIR/test-fixtures/...`，不改测试内容、断言或系统字体回退。字体、许可和测试均不写入原包，也不向 `OUT_DIR` 的父目录补文件。

`tests/rounded_clip.rs` 是独立 integration target：解析并以无扩展能力的 Naga Validator 校验实际生成的 storage/WebGL 两种 shader，用成员偏移和结构大小对照 Rust `PolychromeSprite`。该 target 不替代完整字体测试；恢复夹具后，macOS 上实际执行完整 `--lib --features test-support`，原有 40 项测试全部通过，无忽略或过滤。shader/ABI 和字体测试不等于 WGPU GPU 像素验收；跨后端真实渲染由当前任务另行验证。
