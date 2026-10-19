# macOS GPUI 打包

支持 macOS 宿主的 aarch64-apple-darwin / x86_64-apple-darwin；非当前 Apple 架构须有目标工具链。app 格式最终输出 app.zip，另支持 DMG；不承诺 universal 或跨操作系统构建。

cargo-packager 固定兼容版本 0.11.8，从全局 PATH 解析并在实际绝对路径上核对 --version。仅明确需要 setup 才安装：cargo install cargo-packager --version 0.11.8 --locked，并显式 --root 到已核验的 Cargo 用户全局目录，不自动覆盖其他版本；更换不同全局版本须有相应授权。安装后复读 PATH 与版本，未在 PATH 则报告未完成。Node/pnpm 同样是全局工具，Rust 库仍由项目声明。

cargo-packager 自行使用 OS 用户缓存 .cargo-packager 下载 create-dmg/NSIS 支持资源；它没有 tools-dir 选项，不改 HOME，不声称缓存项目隔离。setup 不安装应用。

~~~sh
pnpm run gpui:package setup --root .
pnpm run gpui:package build --root . --mode local --format dmg
~~~

local 对完整 bundle 做无证书 ad-hoc 并验证 plist/identifier/资源封印。App ZIP 归档前签 bundle、解包后复核；DMG 的固定 hook 在封盘前签，记录签名前后 Mach-O 摘要，最终只读挂载再验证。该身份不等于 Developer ID、公证、系统通知授权或渠道信任。

candidate 的正式 Developer ID 签名/公证尚未实现；启用请求或渠道要求在测试前阻断，不能降为 unsigned。profile 启用系统通知又要求 enabled signing 的候选因此不可形成；具体产品是否命中以其 profile 为准，不能关闭已批准能力绕过。

DMG Finder 布局限时十分钟，检查 .DS_Store、背景、唯一 .app、/Applications 目标与包内资源；没有可用 Finder 布局就失败。app.zip 使用系统 ditto 并实际解包核对。签名或最终资源变化都会形成新的候选字节。

安装前解析 CARGO_INSTALL_ROOT、CARGO_HOME 或标准用户 .cargo，拒绝相对路径、项目内路径及指回项目的链接；显式 --root 防止用户 install.root 配置重定向。安装后仍从全局 PATH 复读实际路径与版本，不改 HOME。优先级依据 [Cargo 官方安装说明](https://doc.rust-lang.org/cargo/commands/cargo-install.html)。
