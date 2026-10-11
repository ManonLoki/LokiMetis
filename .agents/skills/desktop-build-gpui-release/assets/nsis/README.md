# 固定 NSIS 模板来源

`cargo-packager-0.11.8.nsi` 逐字节保留 crates.io `cargo-packager` 0.11.8 的
`src/package/nsis/installer.nsi`，SHA-256 为
`3ff5fde09ae24dfc031ab91f939b290f394e2c6d3549f2c43c37414716a47b7a`。
同目录保留该包的完整 `LICENSE_APACHE-2.0` 与 `LICENSE_MIT`。

维护入口为 `scripts/gpui_nsis.mjs`：摘要和唯一锚点检查后，仅在本次构建暂存区
生成模板。运行中的同名进程导致安装/卸载停止，不强杀；卸载仅删除精确指向
当前安装主程序的 HKCU Run 命令，不删除其他安装或 OS 管理的 StartupApproved。
快捷方式统一使用正式英文名，避免覆盖中文同名参考 App；开始菜单目录按安装上下文
保存并供卸载恢复，静默/被动安装的 `/NS` 同时禁止桌面及开始菜单链接。
模板单元测试和编译均不能替代实际安装/卸载验收。
