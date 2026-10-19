# Windows GPUI 打包

支持原生 Windows x64 / x86_64-pc-windows-msvc 的 NSIS exe。Windows ARM、xwin、Docker 跨系统路线未接入；不把示例实验配置称为受管能力。

cargo-packager 为全局 CLI，精确 0.11.8；显式 setup 的安装与复读合同同 [macOS 工具说明](macos-packaging.md)。其 NSIS 支持资源走 OS 用户缓存，不安装生成的应用。

~~~sh
pnpm run gpui:package build --root . --mode local --target x86_64-pc-windows-msvc --format nsis
~~~

local 保持 unsigned，最终安装与运行未验证。candidate 需 windows.extractor 显式选择全局 PATH 上已批准的 7z/7zr（可带 .exe），不接受旧项目内路径；现有配置迁移需明确选定全局工具，不移动或删除旧工具。helper 不下载解包器，不用安装应用代替解包。

NSIS 使用带摘要的原样模板与双许可证，通过固定生成层完成专属快捷方式、真实运行进程阻断和仅 owned 自启清理。保留用户文件，不能添加强杀或任意自定义 hook。最终 exe 解到隔离区，核对实际安装根、主程序、日志、双语许可证及全部映射资源。

签名仅在明确 enabled 且已批准公开证书指纹/HTTPS 时间戳配置时执行 signtool sign，再 verify /pa /all；二进制与安装器分别按实际阶段处理。不读取或输出私钥/密码；机器已有凭据不代表本次选择启用。
