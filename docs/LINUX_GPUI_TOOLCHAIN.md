# Linux GPUI 工程知识库

本文件是本项目Linux兼容、双架构构建和打包工具链的技术事实源。目标来自根Cargo metadata；声明支持不等于运行验收。Linux当前承接已批准的x64与ARM64工具链扩展，Windows/macOS路径继续独立维护。

## 支持矩阵与宿主边界

| Rust目标 | Debian架构 | GNU工具前缀 | 包格式 |
| --- | --- | --- | --- |
| `aarch64-unknown-linux-gnu` | `arm64` | `aarch64-linux-gnu` | `.deb`、`.tar.gz` |
| `x86_64-unknown-linux-gnu` | `amd64` | `x86_64-linux-gnu` | `.deb`、`.tar.gz` |

两个Linux宿主架构均可构建自身或另一GNU架构。自动依赖安装入口只支持已存在apt/dpkg的Debian/Ubuntu；当前工具链以Ubuntu 26.04 GNU布局为基线。musl、ARM32、RPM、AppImage、跨操作系统构建和目标模拟执行未接入，不将它们推断为可用。系统glibc是ABI约束，不能把新发行版构建宣称为兼容所有旧发行版。

## 依赖、安装和复探

通用工程运行时继续使用环境Skill的Git/Rust/Node/npm/pnpm门禁。Node与pnpm安装在标准用户目录；新登录shell读取持久PATH，已运行Agent进程可能仍保留旧PATH，应开启新登录shell或使用门禁返回的路径，不能据旧进程的`command not found`重复安装。

Snap版curl使用私有`/tmp`，曾导致写下载文件失败（curl 23）；优先使用已安装的原生`/usr/bin/curl`。尚无原生curl时，可将本次标准`TMPDIR`设为用户现有、可访问的Downloads目录后运行原门禁；不关闭TLS/SHA-256校验，不覆盖HOME，不伪造测试模式。

Linux专用入口位于`.agents/skills/desktop-check-development-environment/scripts/linux-gpui-dependencies.sh`。仅在用户明确要求工具链安装，或实际构建已证实相关缺失时使用，不成为日常开发预检。默认只检查；`--install`才调用既有apt并添加必要的目标架构，正常sudo交互不可绕过。管理员安装结束后在原用户下添加并复探Rust目标，不在root账户下替用户安装Rust。

```sh
bash .agents/skills/desktop-check-development-environment/scripts/linux-gpui-dependencies.sh --check-only --target aarch64-unknown-linux-gnu
bash .agents/skills/desktop-check-development-environment/scripts/linux-gpui-dependencies.sh --install --target x86_64-unknown-linux-gnu
```

若终端不在项目根，使用当前项目的绝对脚本路径。`dpkg --add-architecture amd64`接受一个独立架构参数，不能改成`--add-architecture=amd64`；脚本以argv方式传递，避免多行命令粘贴错位。需要系统认证而Agent无终端时，可使用宿主既有pkexec触发正常Polkit认证，不能用密码注入、修改sudo策略或静默提权。

入口安装build-essential、目标GNU C/C++编译器、pkg-config、binutils/readelf、tar、dpkg-dev，以及目标架构的Fontconfig、FreeType、XKB、Wayland、X11/XCB开发库；不存在GTK、WebKit或AppIndicator安装要求。交叉时配置目标`CC/CXX/AR`和Cargo linker；`PKG_CONFIG_LIBDIR`只能指向`/usr/lib/<目标GNU前缀>/pkgconfig:/usr/share/pkgconfig`，清除继承的宿主覆盖并核对解析出的libdir。禁止用宿主库或单独`PKG_CONFIG_ALLOW_CROSS=1`掩盖缺失交叉依赖。

## 原生托盘和热键方案

Linux GUI显式继承现有tray-icon的`ksni`特性；KSNI通过会话D-Bus和StatusNotifierItem运行自身工作线程，不要求在GPUI主线程混入GTK循环。实际托盘宿主是否存在必须单独判断；仅创建对象或开启feature不能证明桌面有恢复入口，不能据此让关闭窗口隐藏到不可恢复状态。

热键支持依赖为ashpd（async-io/global_shortcuts）和x11rb（xkb）。Wayland必须使用GlobalShortcuts Portal及系统授权；纯X11可采用具有checked request和确定错误传播的X11连接。当前global-hotkey 0.8的Linux后端仅X11，工作线程断连时可能返回成功，不能当作已确认注册路径。

本次依赖准备及工具链扩展不会自动增加热键action、注册系统键位或请求Portal权限。Portal/X11的新服务尚未接线；Linux桌宠原生操作、通知权限/点击及各窗口恢复能力仍需分别适配和实际运行验证。不得据编译/打包成功宣称完整Linux体验通过。

只读会话探测：

```sh
gdbus call --session --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop --method org.freedesktop.DBus.Properties.Get org.freedesktop.portal.GlobalShortcuts version
gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher IsStatusNotifierHostRegistered
```

2026-10-18已核对项目内tray-icon 0.26.1与global-hotkey 0.8.0源码，以及[托盘官方后端说明](https://github.com/tauri-apps/tray-icon)、[XDG GlobalShortcuts协议](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)。只读可用性查询不代表真实按键或托盘点击已测试。

## 统一构建与打包入口

继续使用根`pnpm run gpui:package`。Linux不安装cargo-packager CLI，复用GNU tar、dpkg-deb和readelf；`setup`只复探已安装工具，实际系统安装使用上一节显式入口。

```sh
pnpm run gpui:package preview --root . --mode local --target aarch64-unknown-linux-gnu --format deb
pnpm run gpui:package setup --root . --mode local --target x86_64-unknown-linux-gnu --format deb
pnpm run gpui:package build --root . --mode local --target aarch64-unknown-linux-gnu --format deb
pnpm run gpui:package build --root . --mode local --target x86_64-unknown-linux-gnu --format tar.gz
```

每次真实打包先在真实宿主运行全workspace/all-targets/all-features非空测试，再显式target构建release二进制。交叉目标不在宿主强行执行；manifest分别记录`hostTarget`、目标、`native/cross-architecture`和宿主测试范围，目标运行状态保持Unverified。默认单个Cargo job控制内存；测试关闭调试符号，减少大GPUI链接的内存占用，不能把资源耗尽误归为源码错误。测试临时目录放在本次项目忽略的工作目录；索引夹具要求项目位于可确认本地持久卷，本机tmpfs `/tmp`会被既有卷安全策略拒绝，不能为迁就测试放宽网络/未知卷边界或改写HOME。

便携归档使用`LokiMetris/主程序`与相邻`resources/skins`，附`launch`入口。Debian包使用`/opt/<应用identifier>/主程序`与相邻资源，`/usr/bin/<项目slug>`只作转发；同时安装`.desktop`与hicolor图标。版本只取Cargo，架构只取Rust目标。Debian依赖披露从真实ELF读取的glibc最低版本及所需运行库；不自带另一发行版的glibc，不安装或运行生成的应用。

资源配置必须逐项显式声明`linux`目标，不默认复用macOS/Windows映射。两份根许可证、皮肤、生成器和第三方许可证均进入最终包；正式候选还需相同字节的更新日志。构建前后验证源码与资源快照；最终包真实解包后检查文件集合、权限、ELF64机器类型、二进制与资源摘要，以及Debian控制文件和桌面入口。

local仅写忽略的`target/gpui-packaging/local-linux-*`，生成包、相邻SHA-256和manifest。candidate继续要求clean已发布默认主分支/tag、冻结上下文、版本门禁及当次E2E选择；未实现Linux签名，任何启用请求都明确阻断。构建结果只保存在忽略目录和最终回复，不写入本知识库或伪造验收记录。

## 维护与验证入口

修改Linux打包路径时运行`node --test .agents/skills/desktop-build-gpui-release/scripts/gpui_linux.test.mjs .agents/skills/desktop-build-gpui-release/scripts/gpui_build.test.mjs`。Linux测试使用真实系统ELF和真实tar/dpkg-deb验证最终包，但这些夹具不是产品候选；GUI皮肤定位回归同时覆盖便携及Debian安装目录。Windows/macOS回归保留原有约束，不为支持Linux放宽签名、资源、发行或候选门禁。
