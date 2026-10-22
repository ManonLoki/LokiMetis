# vendor 目录说明

本文解释 `vendor/` 为什么存在、它如何接入根工作区、每个子目录各自承担什么职责、补丁如何被生成和校验，以及维护和移除的条件。事实来源为根 `Cargo.toml`、各包 `Cargo.toml`／`build.rs`／`provenance.json`／`README.md`、`vendor/gpui-patch-support/`、`docs/ENGINEERING_RULES.md` 第 2.3 节和 [GPUI 圆角兼容层说明](../gpui-pre/README.md)。本文只是汇总，与上述文件冲突时以它们为准。

## 1. vendor 的定位

LokiMetris 是 GPUI 原生桌面应用，GUI 通过 `gpui-kit 0.7.3` 间接依赖 Zed 发布到 crates.io 的 `gpui-pre 0.3.9` 系列包（`gpui-kit → gpui-pre-platform → gpui-pre-macos / gpui-pre-windows / … → gpui-pre-apple / gpui-pre / gpui-pre-wgpu`），macOS 辅助功能再经 `gpui-pre-macos` 依赖 `accesskit_macos 0.26.3`。

产品复刻过程中有若干行为无法在应用层实现，必须改框架本身（图片独立圆角视口、父圆角裁剪、按需帧调度、可变字体 wght 实例、macOS 窗口 zoom／全屏／reopen 事件、AccessKit 标题层级等）。`vendor/` 的作用就是在**不 fork、不改版本号、不手改第三方源码**的前提下，对这几个固定版本的 crate 施加一组可审阅、可复现、失败关闭的补丁：

- 根 `Cargo.toml` 的 `[patch.crates-io]` 把七个 crate 名重定向到 `vendor/` 下的同名本地包；其余依赖链（`gpui-kit`、`gpui-pre-platform`、`gpui-pre-linux` 等）仍从 crates.io 解析，不感知替换。
- 每个本地包的 `upstream/` 原样保存官方 `.crate` 归档的全部普通文件，逐字节不可变。
- 补丁以"原文片段 → 替换片段"模板形式登记，构建时由仅使用标准库的生成器在 Cargo `OUT_DIR` 内生成完整补丁镜像；源码树里不存在被修改过的上游文件。
- 来源（归档 SHA-256、固定清单 SHA-256、逐文件 SHA-256）、补丁匹配唯一性、生成结果 SHA-256 三层校验任一失败即终止编译。

`[workspace].exclude` 把这七个包排除在工作区成员之外：它们不参与 `cargo test --workspace`，自身的测试需按各包 README 显式运行（见第 6 节）。`vendor/` 不是 `cargo vendor` 产物，与 `.cargo/config` 的 `source.vendored-sources` 无关。

## 2. 目录总览

| 目录 | 替换的 crate | 上游来源 | 作用 / 维护入口 |
| --- | --- | --- | --- |
| `gpui-patch-support/` | 无（共享维护代码） | — | 标准库生成器、scene补丁、`patch-inputs.mjs`与只读验证器 |
| `gpui-pre/` | `gpui-pre 0.3.9`（lib名 `gpui`） | zed@93d7456，160文件 | 公共图元、圆角/父裁剪、按需帧、AX、hitbox与焦点；`build.rs`及`src/*_patches.rs` |
| `gpui-pre-apple/` | `gpui-pre-apple 0.3.9`（`gpui_apple`） | zed@93d7456，19文件 | Metal/图集回收/父裁剪及迁入的CoreText字体；`build.rs`及Metal/font登记 |
| `gpui-pre-macos/` | `gpui-pre-macos 0.3.9`（`gpui_macos`） | zed@93d7456，15文件 | AppKit菜单、reopen/zoom/全屏及需求位；`build.rs` |
| `gpui-pre-windows/` | `gpui-pre-windows 0.3.9`（`gpui_windows`） | zed@93d7456，28文件 | DirectX/HLSL圆角ABI、实时命中与外部显示；`build.rs` |
| `gpui-pre-wgpu/` | `gpui-pre-wgpu 0.3.9`（`gpui_wgpu`） | zed@93d7456，14文件 | Linux所用WGPU及WebGL的圆角ABI、测试夹具；`build.rs` |
| `accesskit_macos/` | `accesskit_macos 0.26.3` | 官方归档15文件；来源登记18项含额外三份许可 | Heading/层级/整数值；独立`build.rs`与`verify-source.mjs` |
| `tray-icon/` | `tray-icon 0.26.1` | 官方归档26文件 | Windows接受非负成功HRESULT；共享生成器，入口为`build.rs`，见[包说明](../tray-icon/README.md) |

精确补丁与输出文件集合取各包登记模块和 `Spec.generated_hashes`，不在概览复制总量。所有 `gpui-pre*` 包的Zed源revision一致（`93d745668805029476e6d7fbb19652116fed51de`；0.3.8为`279fe070…`，2026-10-15升级），ABI变更必须跨后端同步。当前依赖及平台条件另见[技术选型清单](../../docs/TECHNOLOGY.md)。

## 3. 统一的工作机制（GPUI 五包及 tray-icon）

### 3.1 每个包的文件分层

| 层 | 内容 | 可否手改 | 规模门禁 |
|---|---|---|---|
| `upstream/` | 官方 `.crate` 全部普通文件（含原 `Cargo.toml`、`Cargo.toml.orig`、`Cargo.lock`、`LICENSE-APACHE`、`src/`、`examples/`…） | 否 | 原样第三方，按 `source-content.sha256` 登记后豁免 |
| `provenance.json` | 下载 URL、归档 SHA-256、清单 SHA-256、文件数、字节数、Zed revision、清单格式定义 | 升级时整体重写 | 人工维护 |
| `source-content.sha256` | 排序的 `sha256  相对路径` 行，路径相对 `upstream/` | 升级时整体重写 | 人工维护 |
| `Cargo.toml` | 以上游规范化清单为底，`[lib] path = "src/lib.rs"`、`build = "build.rs"`、`publish = false`、examples／tests／benches 路径改指 `upstream/…`，另加本地 feature 与测试 target | 是 | 人工维护 |
| `src/lib.rs`（shim） | 保留 crate 根属性，`include!(OUT_DIR/gpui-patched/src/<entry>.rs)` | 是 | 人工维护 |
| `src/*_patches.rs` | `patch!` 宏登记的补丁组（仅 gpui-pre、gpui-pre-apple 拆分成模块；其余直接写在 `build.rs`） | 是 | 人工维护 |
| `src/*.rs`、`tests/*.rs`（其它） | 自有新增模块（如 `element_transform.rs`、`variable_font.rs`、`atlas_retirement.rs`）及回归测试，由生成树中的补丁以 `mod` 引入或由 `Cargo.toml` 登记 | 是 | 人工维护 |
| `patches/<名称>.before` / `.after` | 一对一的精确原文与替换文本 | 是 | 人工维护 |
| `patches/root-prefix.before` | 上游入口文件开头的 crate 根属性文本；shim 已声明这些属性，生成时从入口中剥离 | 是 | 人工维护 |
| `build.rs` | 组装 `Spec` 调用共享生成器；随后执行上游原有构建步骤（Metal／HLSL 编译、Windows manifest、leak-detection cfg） | 是 | 人工维护 |

### 3.2 构建时发生什么

`gpui-patch-support/generator.rs` 的 `generate(manifest, &Spec)` 按以下顺序执行，任一步失败即 `panic` 终止编译：

1. 读取 `source-content.sha256`，其整体 SHA-256 必须等于 `Spec.source_manifest_sha256`（同时固定在 `build.rs` 与 `provenance.json`）。
2. 递归遍历 `upstream/`，拒绝符号链接、特殊文件、非 UTF-8 或越界路径；实际文件集必须与清单**完全相等**（不多不少），每个文件 SHA-256 必须匹配。
3. 在内存中对入口文件剥离 `root_prefix`（必须是精确前缀）。
4. 按登记顺序逐个应用 `Patch`：`before` 在目标文件中必须**恰好出现一次**，用 `replacen(…, 1)` 替换为 `after`。零匹配或多匹配都失败。
5. 对 `Spec.generated_hashes` 列出的每个文件核对生成后 SHA-256；被改动的文件集合（含入口）必须与该列表**完全一致**——漏列或多列都失败。
6. 清空并重建 `OUT_DIR/gpui-patched/`，把整个上游树（含未改动文件）按原相对路径写出。模块声明、`include_str!` 的相对资源（shader、字体）因此无需改路径。
7. 登记 `cargo:rerun-if-changed` 于 `upstream/`、`source-content.sha256`、`patches/`、`../gpui-patch-support`。

随后各 `build.rs` 继续上游自身的构建逻辑，但输入改为生成树：

- `gpui-pre`：`GPUI_LEAK_DETECTION`／`leak-detection` cfg；Windows 下 `windows-manifest` feature 从 `生成树/resources/windows/` 嵌入资源。
- `gpui-pre-apple`：`cbindgen` 读取生成树的 `vendor/gpui/src/{scene,geometry,color,window,platform}.rs` 与 `src/metal_renderer.rs` 产出 `scene.h`；再用 `xcrun metal`／`metallib` 编译生成树的 `src/shaders.metal`（或 `runtime_shaders` 拼接）。启用 `parent-rounded-clip` 时额外在头文件顶部写 `#define GPUI_PARENT_ROUNDED_CLIP 1`，使 C 宏与 Rust feature 同步。
- `gpui-pre-windows`：release 且目标为 Windows 时用 `fxc.exe`（`GPUI_FXC_PATH` → PATH → 最新 Windows SDK）编译生成树的 HLSL。
- `gpui-pre-apple`：生成前经 `fixture_verify.rs` 校验 `test-fixtures/` 七份字体与 OFL 的完整文件集，再生成。
- `gpui-pre-wgpu`：先校验 `test-fixtures/` 五文件（三字体 + 两 OFL，共 596 542 字节，清单 SHA 固定于 `FONT_FIXTURE_MANIFEST_SHA256`），再生成。

### 3.3 为什么用"模板替换"而不是 `.patch` 文件或 fork

- 不依赖宿主 `patch`／`git` 命令，不访问网络，不新增 Cargo 依赖；Rust 生成器与 Node 验证器都只用标准库／内置模块。
- 唯一匹配比行号 diff 更能抵抗上游漂移：上游一变，补丁立刻失败而不是悄悄错位。
- 生成结果 SHA 把"补丁集合整体效果"钉死：任何人改了某个 `.after` 都必须同步更新 `generated_hashes`，评审能一眼看到。
- 原样上游与人工代码物理分离，使 `docs/ENGINEERING_RULES.md` 的行数门禁能精确豁免原样第三方（按清单逐文件 SHA 匹配），而补丁、shim、生成器、测试仍照常计数。

## 4. `gpui-patch-support/`：共享维护代码

| 文件 | 作用 |
|---|---|
| `generator.rs` | `Patch`、`Spec`、`generate`；内含自测（临时夹具下验证三层校验、只写 OUT_DIR）。各包 `build.rs` 用 `#[path]` 引入。 |
| `sha256.rs` | 纯标准库 SHA-256，供生成器及 wgpu 字体夹具校验复用。 |
| `scene_patches.rs` + `scene_patches/` | `gpui-pre` 与 `gpui-pre-apple` 的上游 `scene.rs` 字节相同（前者在 `src/scene.rs`，后者在 `vendor/gpui/src/scene.rs`），24 对补丁只维护一份，`all(path)` 以包内路径实例化。两包的 `generated_hashes` 中 `scene.rs` 摘要应保持一致，精确值取两包 `build.rs`。 |
| `verify-source.mjs` | 独立Node验证器：核对provenance、清单、上游文件集和入口前缀，经`patch-inputs.mjs`按真实顺序读取共享`all(path)`、模块`PATCHES`、`patch!`与内联`Patch`，复现替换并比较`generated_hashes`；WGPU另核字体夹具。全程只读。 |
| `README.md` | 维护约定摘要。 |

`patch!("目录/", "名称", "目标路径")` 宏约定 `目录/名称.before` 与 `.after` 同名成对，`名称` 同时用于错误信息。同一目标文件上的补丁组顺序不可调换（后组的 `before` 可能依赖前组产生的上下文）。

## 5. 各包详述

### 5.1 `gpui-pre/`（GPUI core）

入口 `src/gpui.rs`；lib 名 `gpui`，因此下游 `use gpui::…` 不变。补丁按四组加共享 scene 组登记，`build.rs` 注明组顺序：scene → 图片帧 → 父裁剪 → 按需帧 → 元素变换。

| 组（登记文件） | 数量 | 改动文件 | 产品可见能力 |
|---|---|---|---|
| scene（共享） | 24 | `src/scene.rs` | `PolychromeSprite` 末尾新增 16 字节圆角视口 bounds；`ParentClipId`／`ParentClipNode` 类型、各图元的父裁剪字段、已用圆角半径链 |
| `image_frame_patches.rs` | 57 | `window.rs`、`platform/test/window.rs`、`app/test_context.rs`、`window/a11y.rs`、`window/a11y/debug.rs`、`view.rs` | `Window::frame_animation_time()`；`ImageRoundedClip` 与 `Window::paint_image_with_rounded_clip()`（原 `paint_image`／emoji 几何不变）；`Window::focus_generation()`；`Window::defer_draw_in_parent()`（保留 AX 父链与 prepaint mask 的 opt-in deferred）；`Window::with_hitbox_exclusions()`（prepaint 期间的互斥命中矩形）；test-support 下 `simulate_accessibility_activation/deactivation`、`hit_test_ids_for_testing` |
| `parent_clip_patches.rs` | 39 | `window.rs`、`view.rs`、`style.rs`、`styled.rs`、`elements/div.rs`、`debug_overlay.rs`、`app/test_context.rs` | `Styled::clip_rounded_children()` opt-in：子元素按祖先圆角链在 GPU 侧裁剪；hitbox／deferred／视图缓存携带父裁剪链 |
| `demand_patches.rs` | 12 | `window.rs`、`svg_renderer.rs`、`randomized_element_tree.rs` | 帧重入唤醒器、deferred presentation 状态、sustain 调度；两处仅在 `frame-demand-regression` feature 下隔离上游无法在独立包中编译的测试 |
| `element_transform_patches.rs` | 36 | `window.rs`（30）、`view.rs`（4）、`element.rs`、`window/a11y/debug.rs` | 新模块 `src/element_transform.rs`：`Window::with_element_transform()` 对元素绘制、命中与 AX 边界施加正比例缩放＋平移，不改布局与纹理缓存键 |

`build.rs`末尾另登记`label-value`、`style-tests-perf-attribute`、`test-window-frame-demand-access`，维护Label值映射、独立包测试属性与平台真实帧需求观测，按声明顺序一并应用。

本地 feature：`parent-rounded-clip`（当前仅 macOS Metal，其它目标 `compile_error!`）、`frame-demand-regression`。新增测试 target `tests/rounded_clip_window.rs`（需 `test-support`）；`src/frame_demand_tests.rs` 由补丁引入。

### 5.2 `gpui-pre-apple/`（Metal）

入口 `src/gpui_apple.rs`。补丁 = 共享 scene 24 + `metal_patches.rs` 36 + `font_patches.rs` 3：

- `atlas-retirement-*`（8）：新模块 `src/atlas_retirement.rs`，跟踪未完成的 GPU 编码 epoch，图集条目退休后等待在途编码完成才真正归还物理分配，防止 CPU 覆盖 GPU 正在读取的区域（见 `docs/performance/gpu-01`）。
- `metal-clip-distance`（1）：`shaders.metal` 以独立圆角视口计算距离。
- `parent-metal-*`（26）：渲染器侧（10）绑定 `ParentClipNode` 缓冲并在 path 中间通道前后重绑；shader 侧（16）把各图元 fragment 拆成 unclipped／parent-clip 两路。`*-unclipped-split` 是无条件的函数拆分，`*-parent-clip` 与 renderer 绑定受 `GPUI_PARENT_ROUNDED_CLIP`／`feature = "parent-rounded-clip"` 守卫。

Feature `parent-rounded-clip` 透传到 `gpui/parent-rounded-clip`；根 `Cargo.toml` 注释说明 GUI 在 macOS 显式依赖 `gpui_apple = { features = ["parent-rounded-clip"] }` 只为统一开启此特性，源码不直接引用该包。测试模块：`rounded_clip_tests.rs`（总是）、`metal_test_support.rs`／`parent_clip_tests.rs`／`geometry_metal_tests.rs`／`skin_parent_only_metal_tests.rs`（仅 feature 开启），均为真实离屏 Metal 像素测试。

另有 `parent-metal-surface-test-field`（1）：0.3.9 上游新增的 Surface 渲染测试直接构造 `PaintSurface`，开启父裁剪时补齐 `parent_clip` 字段。

可变字体（`src/font_patches.rs`，3）：0.3.9 上游把 `text_system.rs`／`open_type.rs` 从 gpui-pre-macos 移入本包并改名 `AppleTextSystem`；`font-native-identity-state`、`font-requested-weight-instance`、`font-native-registration-and-runs` 只改类型名后迁入，为请求字重建立真实 CoreText `wght` 实例，选择与 CTRun 反查统一用轴／feature／cascade／矩阵身份；自有模块 `variable_font.rs`、`semantic_identity.rs`、`descriptor_identity.rs` 及其测试同步迁入。附属资产 `test-fixtures/` 七份 Geist／Geist Mono 字体与 OFL（来源见 `fixture-provenance.json`，清单 `test-fixtures.sha256`）由 `fixture_verify.rs` 在构建期核验，仅供测试，不安装系统字体。

### 5.3 `gpui-pre-macos/`（AppKit 平台层）

入口 `src/gpui_macos.rs`。24 个补丁直接写在 `build.rs`，按主题（可变字体 3 个已于 0.3.9 随上游 text_system 迁入 gpui-pre-apple）：

| 主题 | 补丁 | 改动文件 | 行为 |
|---|---|---|---|
| 菜单 | `menu-action-ownership`、`menu-command-unlock`、`menu-validation-unlock`、`menu-tests` | `platform.rs` | 主菜单与 Dock 菜单按完整动作值复用稳定索引；命令分派与 `validateMenuItem` 先 `drop` 平台锁再调用应用回调，附 `src/menu_tests.rs` |
| 应用唤起 | `reopen-registration`、`reopen-event` | `platform.rs` | 注册 `applicationShouldHandleReopen:hasVisibleWindows:`；有 handler 时每次转发，返回 NO 阻止默认恢复 |
| 窗口缩放 | `zoom-target-registration`、`zoom-target-event` | `window.rs` | `windowShouldZoom:toFrame:` 发布私有 `LokiMetrisGPUIWindowWillZoomNotification`（userInfo 含 `targetFrame`） |
| 全屏失败 | `fullscreen-failure-registration`、`fullscreen-failure-event` | `window.rs` | 进入／退出全屏失败时发布 `LokiMetrisGPUIWindowFullScreenTransitionFailedNotification` |
| 按需帧（14） | `window.rs` 上 9 个：`demand-window-{state,init,source,waker}`、`demand-unocclude`、`demand-screen-change`、`demand-activation-consume`、`demand-layer-consume`、`demand-main-dispatch`；`display_link.rs` 上 5 个：`demand-subscriber`、`demand-vsync-gate`、`demand-subscribe-{argument,state}`、`demand-frame-source` | `window.rs`、`display_link.rs` | 新模块 `frame_demand.rs`：每窗口原子需求位，CVDisplayLink 线程只观察、有需求才投递主队列（见 `docs/performance/案例-2`） |

附属资产：`licenses/NOTICE.md` 为本地修改声明；字体夹具已随可变字体迁入 gpui-pre-apple。

### 5.4 `gpui-pre-windows/`（DirectX 平台层）

入口 `src/gpui_windows.rs`。`build.rs`登记9个补丁：HLSL布局/圆角距离与调试生成树shader路径，以及`live-resize-*`、`live-maximize-*`、`external-show-*`维护原生边缘命中、最大化和外部显示几何。父圆角裁剪尚未接入HLSL，真实渲染按候选另行验证。

### 5.5 `gpui-pre-wgpu/`（WGPU／WebGL 后端）

入口 `src/gpui_wgpu.rs`，`root-prefix.before` 为空（上游入口无根属性）。9 个补丁：WGSL／WebGL 的圆角视口字段与距离（3）、Rust 步幅与 Naga 测试导入（2）、四处 `cfg(test)` 字体 `include` 改指 `CARGO_MANIFEST_DIR/test-fixtures/…`（4）。`test-fixtures/assets/fonts/{lilex,ibm-plex-sans}/` 补齐官方归档缺失的三份字体与两份 OFL，来源同一 Zed revision（`test-fixtures-provenance.json`）。`tests/rounded_clip.rs` 用 Naga Validator 校验两种 shader 结构偏移与 Rust ABI 一致。当前Linux产品经`gpui-pre-linux`进入WGPU渲染路径；WebGL属于上游后端，不代表本项目新增Web接口。补丁仍须保持跨后端ABI一致，各平台真实渲染分别取证。

### 5.6 `accesskit_macos/`（独立于 gpui-patch-support）

此包早于共享生成器建立，机制略有不同：

- `src/lib.rs` 直接以 `#[path = "../upstream/src/…"]` 引用未修改的 `context.rs`、`filters.rs`、`util.rs`、`adapter.rs`、`event.rs`、`patch.rs`、`subclass.rs`；只有 `node.rs` 经 `include!(OUT_DIR/node.rs)` 引入生成版本。
- `build.rs` 对完整 `upstream/src/node.rs` 校验固定 FNV-1a 64 指纹，应用 `patches/01…07` 七处唯一替换，再校验补丁结果指纹；输出 `mod node { … }` 完整模块以保留上游 inner attribute。FNV 只做维护漂移检测，来源真实性由 `crate-content.sha256`／`source-content.sha256` 的 SHA-256 清单承担。
- `patches/heading.patch` 是与七个模板等价的可审阅 unified diff。
- 官方归档不含许可证，`licenses/LICENSE-{MIT,APACHE}`、`LICENSE.chromium` 从 `provenance.json` 指定的精确提交原样补入；因此 `source-content.sha256` 为 18 项（15 归档文件 + 3 许可），`crate-content.sha256` 为 15 项。
- 补丁语义：Heading 映射 `AXHeading`；正层级声明并返回 `AXHeadingLevel`；无显式值时按 WebKit 约定返回整数 `AXValue`。Label setter、Braille 属性及其它角色不变。
- 测试：`generation`（漂移与唯一替换失败关闭）、`heading`（`harness = false`，主线程直接调用真实 AX selector，不创建 NSApplication／窗口）、`readonly_probe`。

## 6. 校验与测试命令

全部在项目根执行。来源完整性（不写任何文件）：

```sh
node vendor/gpui-patch-support/verify-source.mjs vendor/gpui-pre vendor/gpui-pre-apple vendor/gpui-pre-macos vendor/gpui-pre-windows vendor/gpui-pre-wgpu vendor/tray-icon
node vendor/accesskit_macos/verify-source.mjs
```

历史记录（2026-10-11）：当时macos/windows/wgpu三包与accesskit通过；`vendor/gpui-pre`、`vendor/gpui-pre-apple`以`generated output drift: src/element.rs`失败，原因是当时Node验证器无法读取宏登记。该限制现已由`patch-inputs.mjs`处理，不能据历史失败继续宣称验证器失效；Rust构建期生成器仍独立校验。本轮文档整理未重跑这些来源检查，不新增通过结论。

生成器自测：

```sh
rustc --edition=2024 --test vendor/gpui-patch-support/generator.rs -o /tmp/loki-gpui-generator-tests
/tmp/loki-gpui-generator-tests --test-threads=1
```

包内测试因在工作区外，必须显式绑定同一组路径补丁（示例取自 `gpui-pre-apple/README.md`，含迁入的可变字体回归与 Metal 实物测试）：

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

2026-10-15 升级 0.3.9 后实际运行（同一组路径补丁）：`gpui-pre` lib（`test-support,parent-rounded-clip,frame-demand-regression`）372 通过／2 上游忽略；`gpui-pre --test rounded_clip_window` 2 通过；`gpui-pre-apple` lib 55 通过（含可变字体 19 项）；`gpui-pre-macos` lib 15 通过；`gpui-pre-wgpu` lib 40 通过、`--test rounded_clip` 1 通过；生成器自测 5 通过。0.3.9 发布清单不再携带 `util_macros`，`style-tests-perf-attribute` 把上游 style 测试的 `#[perf]` 指向内建 `#[test]`；上游与本地 CoreVideo 测试所需的 `CVBase`／`CVReturn` 由 gpui-pre／gpui-pre-apple 的 macOS dev-dependency 显式开启。

`accesskit_macos` 的 `cargo check`／`cargo test --test generation|heading` 用独立 `--target-dir` 与 `--offline`，见其 README。

这些检查证明的是"来源未漂移、模板仍唯一、生成结果等于审阅结果、包内单元回归通过"，**不是**产品验收。真实 Window／GPU 像素／Computer Use 证据按 `AGENTS.md` 只写忽略的 `release/`。

## 7. 维护流程

### 7.1 新增或修改一个补丁

1. 在目标包的 `patches/` 新建 `<名称>.before`／`.after`；`before` 必须在**当前生成树**（而非原始上游，若同文件已有前序补丁）中唯一出现。
2. 在对应登记处追加 `Patch`（gpui-pre／gpui-pre-apple 用 `src/*_patches.rs` 的 `patch!`；其它直接写 `build.rs`；两包共用的 scene 改动写 `gpui-patch-support/scene_patches.rs`）。注意组内与组间顺序。
3. 构建一次，让生成器报出 `reviewed generated output drift: <文件> (actual <sha>)`，审阅 `OUT_DIR/gpui-patched/<文件>` 的实际内容后把新摘要写回 `generated_hashes`。若改动文件此前未在列表中，必须新增条目；生成器会拒绝"改动集合 ≠ 摘要集合"。
4. 若补丁新增了公开 API，补充对应包内测试；若改了 `PolychromeSprite` 等跨后端结构，Metal／HLSL／WGSL／WebGL 四处必须同步。
5. 更新该包 `README.md`（及 `licenses/NOTICE.md`，如有）的行为描述。

### 7.2 升级上游版本

1. 下载官方 `.crate`，核对归档 SHA-256，解包替换整个 `upstream/`。
2. 重新生成 `source-content.sha256`（排序、两个 ASCII 空格、相对 `upstream/`、LF），更新 `provenance.json` 与 `build.rs` 的 `source_manifest_sha256`。
3. 比对上游 `Cargo.toml` 与本地 `Cargo.toml`，同步依赖与 feature，保留本地 `[lib]`／`build`／`publish = false`／测试 target。
4. 核对 `root-prefix.before` 仍是入口文件的精确前缀。
5. 逐个重新匹配补丁；上游已自带等效修改的补丁应删除而不是硬凑 `before`。
6. 重算 `generated_hashes`，同步`docs/ENGINEERING_RULES.md`第2.3节和`verified_third_party.mjs`的精确根、清单摘要及项数，包含tray-icon登记；不在概览维护第二套数量合同。
7. 五个 `gpui-pre*` 包必须同一 Zed revision 同步升级，否则 scene 共享补丁与跨后端 ABI 失效。

### 7.3 移除条件

各 README 约定：当官方 GPUI／AccessKit 已提供等效能力，并通过本项目相同的几何、资源、真实像素和受影响目标平台 Computer Use 回归后，删除对应 `vendor/` 子目录与根 `[patch.crates-io]` 条目。不得只为去掉补丁而跨版本替换框架、改变其它角色语义或取消失败验证。

## 8. 与仓库其它规则的关系

- `docs/ENGINEERING_RULES.md` 2.3：行数门禁对 `vendor/` 的豁免是封闭定义——仅七份 `source-content.sha256` 登记且逐文件 SHA 匹配的原样文件；缺失或漂移必须失败，不能改成跳过整个目录。补丁、shim、生成器、测试、`README.md` 和本文都按人工维护文本计数并要求中文注释。
- `docs/RUST_CLI_TEMPLATE.md`：`upstream/Cargo.lock` 仅作来源证据跟踪（各包 `.gitignore` 显式 `!upstream/Cargo.lock`），不是工作区解析锁。
- 根 `Cargo.toml` 注释：`gpui_apple` 工作区依赖存在的唯一理由是统一开启 `parent-rounded-clip`，删除会静默关闭父圆角裁剪。
- 性能文档 `docs/performance/gpu-01`、`gpu-03`、`案例-2` 分别解释图集回收、CVDisplayLink 需求位与帧调度补丁的动机与证据。

## 9. 已观察到的不一致与风险

以下区分当前边界与历史问题；本轮只同步说明，不修改第三方包或新增运行验收：

- Node验证器曾因无法读取宏/共享补丁而失败，2026-10-11失败事实保留在第6节；当前已支持这些登记形式。文档同步不替代任一实际验证结果。
- `gpui-pre-macos/Cargo.toml` 保留了上游的 `publish = true`，其它六个包装包为 `publish = false`；本项目仅消费路径补丁，不执行crate发布。
- 部分README仍保留通用圆角说明；包专属机制同时查本概览、各`build.rs`与登记模块。Apple已有字体迁移说明、Windows已有宿主行为说明，不再将两者描述为完全未更新的复制。
- `gpui-pre-wgpu/provenance.json` 同时含两套字段命名（`crate_archive_sha256` 与 `archive_sha256`、`crate_archive_regular_file_count` 与 `upstream_file_count`），值一致但与其它四包的 schema 不同。
- `gpui-pre-apple/fixture-provenance.json` 记录的 `source` 是开发机绝对路径（含历史项目 `loki_metis` 的 `node_modules`），不可在其它机器复现下载。
- Windows（`gpui-pre-windows` HLSL／DirectX）与 WGPU 的真实渲染均无实机验证记录；父圆角裁剪仅 Metal 实现。
- 补丁跨共享scene、模块及内联登记，升级须按各包真实入口逐项重匹配，不以会漂移的总量公式代替源码登记，无自动迁移。
