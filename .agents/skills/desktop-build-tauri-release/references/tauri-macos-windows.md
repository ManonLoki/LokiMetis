# Tauri macOS / Windows 候选路线

## 共同要求

- 只使用项目本地 Tauri 构建工具和锁文件；每次命令显式合并 `src-tauri/tauri.release.conf.json`。
- 该配置只映射根 `release-notes.json`，不创建远程版本服务、feed 或额外制品。
- source commit 必须是 clean 40 位 HEAD；候选、manifest、性能探针和验证证据都绑定同一提交。
- 安装包签名、公证和渠道发布是独立门禁；秘密只从批准的安全运行时来源读取，不写入仓库、日志或 bundle。

## Windows 原生 x64 NSIS

- 宿主必须是 Windows x64，使用 MSVC Rust target 与项目本地 Tauri 命令生成 NSIS。
- 原生路线不传 xwin runner。输出必须从真实构建元数据定位，不能猜测文件名。
- 渠道要求 Authenticode 时验证签名链、timestamp 与最终字节；否则 manifest 明确标记 unsigned 及作用域。

## macOS 原生 DMG

- 使用项目内 660×400 DMG 背景，app 与 Applications 落点分别为 `(180,220)` 和 `(480,220)`。
- 直接分发启用 Developer ID 时，签名、公证、ticket stapling 和最终验证必须作为一个完整阶段；不可保留“只签名未公证”的候选。
- `verify-dmg-layout.sh` 只读挂载最终 DMG，验证唯一 app、Applications 链接、背景、非空布局文件与包内本地 release notes 字节一致。

## macOS xwin → Windows x64 NSIS

- 只在批准目标明确且真实构建失败暴露工具缺失时调用 xwin 环境门禁。
- 使用兼容 cargo-xwin、LLVM/LLD、NSIS 与 `x86_64-pc-windows-msvc` target；不生成 MSI。
- 交叉编译成功不证明 Windows 安装或运行，manifest 固定 `buildMode: cross-compiled-xwin`、`runtimeVerification: Unverified`。

参考：Tauri 2 官方 build/bundle、macOS signing/notarization、Windows installer 与 cargo-xwin 文档。
