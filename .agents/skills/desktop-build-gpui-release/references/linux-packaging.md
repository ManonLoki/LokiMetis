# Linux GNU 双架构打包

| Rust target | Debian 架构 | GNU 前缀 | 格式 |
| --- | --- | --- | --- |
| aarch64-unknown-linux-gnu | arm64 | aarch64-linux-gnu | deb、tar.gz |
| x86_64-unknown-linux-gnu | amd64 | x86_64-linux-gnu | deb、tar.gz |

Linux 宿主支持同系统双向交叉架构，测试运行宿主，目标只构建/ELF校验；不模拟执行。musl、ARM32、Windows/macOS 跨系统、RPM、AppImage 未接入。新发行版 glibc 约束随实际 ELF 披露，不承诺兼容所有旧系统。

## 工具与 setup

Linux 不安装 cargo-packager。使用目标 GNU C/C++、ar、pkg-config、readelf、GNU tar、dpkg-deb，以及原生协议测试需要的 dbus-daemon。Fontconfig、FreeType、XKB/xkbcommon-x11、Wayland、X11/X11-xcb/XCB 头文件和库必须为目标架构；不强塞 GTK、WebKit 或 AppIndicator。

preview 不执行探针。setup 检查工具/目标库，写忽略 target 下的 C++ 探针并真实编译链接、验证 ELF，不运行探针；它有文件和编译副作用，不是只读检查，不安装系统依赖。明确要求安装或真实相关失败时，才由环境 Skill 使用 linux-gpui-dependencies.sh；仅既有 apt/dpkg 的 Debian/Ubuntu 自动安装，不能静默提权。整个入口提权为 root 时只做系统阶段，以 pending-original-user/退出20交回原用户完成 rustup，不猜测 HOME。

交叉 env 明确 CC/CXX/AR/Cargo linker，PKG_CONFIG_LIBDIR 为 /usr/lib/<GNU前缀>/pkgconfig:/usr/share/pkgconfig；清除继承宿主覆盖，确认实际 libdir。只有 PKG_CONFIG_ALLOW_CROSS=1 不能证明交叉库正确。

~~~sh
pnpm run gpui:package preview --root . --mode local --target aarch64-unknown-linux-gnu --format deb
pnpm run gpui:package setup --root . --mode local --target x86_64-unknown-linux-gnu --format tar.gz
pnpm run gpui:package build --root . --mode local --target aarch64-unknown-linux-gnu --format deb
~~~

## 构建与最终包

真实宿主全部 workspace/all-targets/all-features 非空测试通过后再 --target 构建。默认单 Cargo job、宿主测试关闭调试符号用于降低链接内存；started blocking 与宿主卷安全策略不放宽。单实例测试短 TMPDIR 放 target/lt-*；项目路径过长就要求更短本地 checkout，不以链接/跳过测试绕开 socket 地址限制。D-Bus 测试使用 owned 私有 daemon，不改用户 session bus。

tar.gz 含产品目录、可执行程序、相邻 resources 与 launch；deb 含 /opt/<identifier>、/usr/bin 转发入口、.desktop 和真实派生的 hicolor/512x512 PNG。资源逐项声明 linux 映射；不从其他平台猜测。双语许可证与第三方许可证按配置进入，candidate 另含相同日志字节。tar 清除 TAR_OPTIONS，并用 -- 结束选项。

最终包真实解包核对精确集合、权限、ELF64机器类型、二进制/资源摘要；deb 还核对控制文件、架构、glibc最低需求、桌面身份与启动入口。构建不安装或运行应用。manifest 分开 hostTarget、target、native/cross-architecture、宿主测试与 runtimeVerification:Unverified。

签名未实现，启用请求失败关闭。工具支持不代表模板已接所有 Linux 桌面能力，也不代表已有远程 CI。local 写 target，candidate 沿公共候选合同写 release；目标运行/桌面托盘/通知/混合DPI按实际候选另验收。
