# GPUI 构建补丁支持

此目录是人工维护的标准库生成器与独立 Node 验证器，不属于原样第三方豁免。
生成器拒绝链接、路径越界、额外源文件、清单漂移、原文件漂移、非唯一模板及输出 SHA 漂移。
所有源码写入只发生于 OUT_DIR/gpui-patched；crate 根 shim 保留根属性，模块和相对资源从完整生成树解析。

Node 验证器使用 Node 自带 crypto/fs/path；Rust 生成器使用标准库 SHA-256，不依赖外部命令、全局包或新增 Cargo 依赖。
来源清单的相对路径以各包 upstream 目录为根，工程原样内容登记须使用同一根及固定清单摘要。

各兼容包的维护入口、模板和测试照常接受行数与中文注释规则；只有经过完整 SHA 核验的 upstream 原文件属于原样内容。

`scene_patches.rs` 与 `scene_patches/` 登记 gpui-pre 与 gpui-pre-apple 共用的 scene.rs 补丁（上游 scene.rs 相同，仅包内路径不同），只维护一份。gpui-pre / gpui-pre-apple 的专属补丁组放在包内 `src/*_patches.rs`，用 `patch!` 宏按 `名称.before|after` 约定登记；其余兼容包直接在 `build.rs` 登记。同一文件上的补丁组顺序不可调换，匹配基于前序补丁已应用的当前文本，生成摘要不符会失败关闭。

独立验证器按 build.rs 的真实拼装顺序读取共享 `all(path)`、模块 `PATCHES`、`patch!` 与内联 `Patch`，模板路径相对声明文件解析；未知表达式失败关闭，不执行 Rust 构建代码。维护回归运行 `node --test vendor/gpui-patch-support/verify-source.test.mjs`，包含全部五个 GPUI 包的来源与生成摘要核验。
