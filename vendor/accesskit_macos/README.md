# AccessKit macOS 标题兼容补丁

此目录替代 GPUI 0.3.8 使用的 `accesskit_macos 0.26.3`，保持原有公开接口与依赖。
补丁只将 Heading 映射为 `AXHeading`，为正数层级声明、返回 `AXHeadingLevel`，
并按 WebKit 的原生约定在没有显式值时返回整数 `AXValue`。
两种 Braille 属性、父类属性列表及其它角色保持原有语义。Label setter 未修改；
静态文字显示为 settable 的来源尚未证明，不在此补丁范围内。

## 来源与许可

`upstream/` 原样保存 crates.io 官方 0.26.3 归档的全部 15 个普通文件，包括原始
Cargo 清单、锁文件、VCS 元信息和所有源码；禁止手工编辑。
归档 SHA-256 为
`ce02dc63b43f0c9296af9ac946312a2dc8814427d7a64d2d600971dac55b6076`。

官方归档未携带许可证。`licenses/` 的 `LICENSE-MIT`、`LICENSE-APACHE` 和
`LICENSE.chromium` 分别从原始 `.cargo_vcs_info.json` 指定的官方提交
`c88605b96d04431f9c3c792464a0f2f253480e94` 原样补充。
`licenses/NOTICE.md` 是本项目维护的修改说明，不属于原样上游内容。
精确来源 URL、SHA-256 清单定义见 `provenance.json`。

## 装配与维护

`src/lib.rs` 按路径使用未修改的上游模块。`build.rs` 仅使用 Rust 标准库，
先对完整原始 `node.rs` 字节检查固定 FNV-1a 指纹，再按 `patches/*.before` 和
`*.after` 执行七处严格、唯一的原文替换。结果指纹也必须与已审阅补丁相符。
FNV-1a 只检测维护漂移，不用于安全认证；SHA-256 来源清单另行核验。

`patches/heading.patch` 是可审阅的显式差异，必须与替换模板保持一致。
构建不调用系统 patch 命令、不访问网络，也不写入 `upstream/`。
生成的 `OUT_DIR/node.rs` 包含完整 `mod node { ... }`，保留上游模块级
inner attribute、版权注释和模块关系。此生成物禁止手工编辑。
补丁、模板、生成器、薄壳及测试属于人工维护文件，不能按原样第三方豁免规模规则。

当 GPUI 接入的官方 AccessKit 已同时修正 Heading 角色与层级导出，并通过这里的
原生 AX 回归及真实应用 Computer Use 核对后，可移除此目录和根 Cargo path patch。
不得只为取消本地补丁而跨版本升级整套 AccessKit 或改变其它角色语义。

## 验证

在项目根执行；AX 测试仅适用于真实 macOS 主线程：

```sh
node vendor/accesskit_macos/verify-source.mjs
ACCESSKIT_TARGET_DIR="$(mktemp -d /tmp/loki-metris-accesskit-target.XXXXXX)"
cargo check --manifest-path vendor/accesskit_macos/Cargo.toml --target-dir "$ACCESSKIT_TARGET_DIR" --offline
cargo test --manifest-path vendor/accesskit_macos/Cargo.toml --target-dir "$ACCESSKIT_TARGET_DIR" --offline --test generation
cargo test --manifest-path vendor/accesskit_macos/Cargo.toml --target-dir "$ACCESSKIT_TARGET_DIR" --offline --test heading
```

离线解析需要已有依赖缓存；本次验证复用当前项目解析版本，未新增下载或依赖。
`generation` 检查原文漂移与唯一替换的失败关闭边界。
`heading` 使用 `harness = false`，直接编译产品同一生成节点并调用真实 AX selectors；
不创建 NSApplication、窗口或视图，不注册 Context 节点缓存，也不发送系统通知。
它验证正层级与整数 Value、无效层级、非标题、显式值优先级、Braille 共存和父类属性保留。
AXValue 映射依据：[WebKit AXCoreObject::value](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/accessibility/AXCoreObject.cpp#L849-L860)
及 [macOS NSNumber 转换](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/accessibility/mac/WebAccessibilityObjectWrapperMac.mm#L1320-L1334)，核对日期2026-10-09。
这些回归不替代完整应用的原生 AX Computer Use 验收。
