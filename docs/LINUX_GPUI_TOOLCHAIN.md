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

Linux专用入口位于`.agents/skills/desktop-check-development-environment/scripts/linux-gpui-dependencies.sh`。仅在用户明确要求工具链安装，或实际构建已证实相关缺失时使用，不成为日常开发预检。默认只检查；`--install`才调用既有apt并添加必要的目标架构，正常sudo交互不可绕过。普通用户运行时只有apt步骤提权，Rust目标仍在其自身账户添加并复探。若整个入口经sudo/pkexec成为root，仅完成管理员系统依赖阶段，不执行root账户的Rust工具、不猜测原用户HOME；输出`linux.status=pending-original-user`及`rustup target add <目标>`后以20退出，须回到原用户终端执行并重新`--check-only`，不能把这一待接续状态当作完整工具链通过。

```sh
bash .agents/skills/desktop-check-development-environment/scripts/linux-gpui-dependencies.sh --check-only --target aarch64-unknown-linux-gnu
bash .agents/skills/desktop-check-development-environment/scripts/linux-gpui-dependencies.sh --install --target x86_64-unknown-linux-gnu
```

若终端不在项目根，使用当前项目的绝对脚本路径。`dpkg --add-architecture amd64`接受一个独立架构参数，不能改成`--add-architecture=amd64`；脚本以argv方式传递，避免多行命令粘贴错位。需要系统认证而Agent无终端时，可使用宿主既有pkexec触发正常Polkit认证，不能用密码注入、修改sudo策略或静默提权。

入口安装build-essential、目标GNU C/C++编译器、pkg-config、binutils/readelf、tar、dpkg-dev、原生协议回归所需的dbus-daemon测试夹具，以及目标架构的Fontconfig、FreeType、XKB、Wayland、X11/XCB开发库；不存在GTK、WebKit或AppIndicator安装要求。测试只在显式私有地址启动/回收独立daemon，不使用或改写用户session bus；产品运行的实际桌面可以使用dbus-broker，不要求其系统服务由dbus-daemon实现。交叉时配置目标`CC/CXX/AR`和Cargo linker；`PKG_CONFIG_LIBDIR`只能指向`/usr/lib/<目标GNU前缀>/pkgconfig:/usr/share/pkgconfig`，清除继承的宿主覆盖并核对解析出的libdir。禁止用宿主库或单独`PKG_CONFIG_ALLOW_CROSS=1`掩盖缺失交叉依赖。

## 原生托盘和热键方案

Linux GUI显式继承现有tray-icon的`ksni`特性；KSNI通过会话D-Bus和StatusNotifierItem运行自身工作线程，不要求在GPUI主线程混入GTK循环。恢复入口由独立有界worker核对Watcher的真实宿主及注册列表，并复核条目属于本进程且Id为`main_tray`；主线程只读已发布事实。未知、断连、宿主消失或仅创建KSNI对象均不允许将主窗隐藏到不可恢复状态。

热键支持依赖为ashpd（async-io/global_shortcuts）和x11rb（xkb）。Wayland必须使用GlobalShortcuts Portal及系统授权；纯X11可采用具有checked request和确定错误传播的X11连接。当前global-hotkey 0.8的Linux后端仅X11，工作线程断连时可能返回成功，不能当作已确认注册路径。

依赖准备及工具链扩展不会自动增加热键action、注册系统键位或请求Portal权限。Portal/X11的后台热键服务尚未接线，空action合同保持不变。下述窗口和通知已接入相应宿主路径，真实托盘点击、系统通知、桌宠交互及完整候选E2E仍须分别取证；不得据源码、编译或打包成功宣称完整Linux体验通过。

只读会话探测：

```sh
gdbus call --session --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop --method org.freedesktop.DBus.Properties.Get org.freedesktop.portal.GlobalShortcuts version
gdbus call --session --dest org.kde.StatusNotifierWatcher --object-path /StatusNotifierWatcher --method org.freedesktop.DBus.Properties.Get org.kde.StatusNotifierWatcher IsStatusNotifierHostRegistered
```

2026-10-18已核对项目内tray-icon 0.26.1与global-hotkey 0.8.0源码，以及[托盘官方后端说明](https://github.com/tauri-apps/tray-icon)、[XDG GlobalShortcuts协议](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)。只读可用性查询不代表真实按键或托盘点击已测试。

## 窗口、Dock身份与桌宠宿主

进入事件循环前，从已捕获的图形会话中选择后端：存在非空`DISPLAY`时使用X11/XWayland，保留其余会话值，不改写进程环境。纯Wayland没有XWayland时仍使用Wayland；普通toplevel缺少本产品所需的全局定位和置顶合同，当前产品未接通该环境的桌宠宿主路径。桌宠操作明确失败并保留回滚/恢复入口，不能将激活或普通窗口重排当作已实现这些能力；GPUI已有Wayland输入区域API本身不等于完整桌宠已支持。

X11/XWayland窗口通过真实XID执行map/unmap、root全局客户区位置/尺寸、EWMH `ABOVE`、工具窗/跳过任务栏、装饰和锁定尺寸提示。置顶是窗口管理器协商状态，不能仅凭请求已发送声称覆盖关系通过。工作区来自RandR活动屏幕与当前桌面的`_NET_WORKAREA`交集，协议不足时才回退真实root几何；当前窗口scale只在物理坐标与GPUI DIP边界换算，持久化仍采用全局物理位置。Shape Input区域独立于视觉区域，开启穿透后读回；失败尝试恢复原区域，未知状态继续披露。

窗口管理器确认由16ms异步timer与总2秒预算完成，正常界面不通过sleep等待。偏好只有原生效果确认后才交给文件服务；期间屏蔽中间bounds，关闭设置不撤销已接受意图，退出接管尚未提交的最后意图。首次map可能改写初始EWMH状态，显示后重新请求并确认置顶；临时几何/显隐目标确认后释放，不能用旧目标拦住后续拖动或单独切换穿透。

Dock/任务栏身份统一为`com.loki-metris.app`：窗口`app_id`、Debian桌面入口和`StartupWMClass`必须一致。事件循环前由已批准Logo准备64/128像素ARGB图标，活窗写入`_NET_WM_ICON`，开发二进制无需先安装桌面入口也能提供真实图标。包内桌面入口及图标仍须通过最终包核验；桌面分组、图标呈现、混合DPI和热插拔须真实运行验证。

## Hooks 与共享设置加载

桌宠和Agent设置共用同一文件事务，但各自有独立消费者。首次经桌宠读取或保存加载权威设置后，必须向listener及Hook writer发布启用Agent集合和目录；后续缓存命中不能省略这次发布。同位置的noop仍须识别首次加载或磁盘中的Agent选择/目录变化，普通桌宠移动则不重复触发Hooks修复。

Hooks验证区分HTTP接收与状态机接纳：`202`只证明listener接收到请求，必须进一步观察已启用Agent的实际状态。协议回归运行五种公共Agent最终生成的真实shell命令，经二进制relay解析和真实环回listener投递，覆盖带空格、中文及单引号的可执行路径；临时目录及私有端点不改写用户Agent配置。

## 系统通知宿主

Linux使用固定会话D-Bus服务`org.freedesktop.Notifications`，不依赖全局通知CLI。标准没有每应用授权查询，`GetCapabilities`成功只表示当前服务可用；不可达或超时为不可用，不伪装用户拒绝。启动不发送测试通知或修改权限，实际投递仍遵守已确认应用开关与领域事件规则。

通知worker拥有有限提交邮箱及桌面通知ID映射，提交总8秒预算覆盖连接、认证与方法调用；关闭偏好或权限暂停撤销尚未触达系统的命令。确定拒绝与`NoReply`/超时等未知结果分别映射领域回执，不盲目重复发送。`Notify`返回有效ID只记API Accepted，不能证明横幅显示。服务唯一owner变化后丢弃旧桌面ID，只有属于本owner已发送通知的`default`动作触发恢复，未知通知动作不唤醒应用；正文按纯文本转义，桌面入口及本地图标沿用同一产品身份。服务探测、实际横幅/点击、断连和退出均需在真实候选中验证。

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

单实例测试使用文件型Unix socket，完整路径受sockaddr_un容量限制。测试TMPDIR固定选择项目`target/lt-*`短目录，为tempfile/private/instance.sock预留空间；不要放进深层打包工作目录。项目路径本身过长时明确报错并选择更短的本地checkout，不创建不透明链接或跳过单实例回归。tar写入归档前使用`--`终止选项，并移除继承的TAR_OPTIONS，避免产品目录名被解释为命令选项。

## 维护与验证入口

修改Linux打包路径时运行`node --test .agents/skills/desktop-build-gpui-release/scripts/gpui_linux.test.mjs .agents/skills/desktop-build-gpui-release/scripts/gpui_build.test.mjs`。Linux测试使用真实系统ELF和真实tar/dpkg-deb验证最终包，但这些夹具不是产品候选；GUI皮肤定位回归同时覆盖便携及Debian安装目录。Windows/macOS回归保留原有约束，不为支持Linux放宽签名、资源、发行或候选门禁。
