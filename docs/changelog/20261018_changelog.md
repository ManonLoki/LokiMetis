# 2026-10-18 开发变更

## LINUX-NATIVE-TOOLCHAIN-001 · Linux GNU双架构构建与打包工具链

分类：feature。required_version：`0.3.1`。本发布周期功能锁已开启，本事件不重复提升Minor，不构成Git发布。

目标平台增加Linux，支持GNU x64/ARM64的原生与同系统交叉架构工具链。Linux GUI启用现有tray-icon的KSNI后端，并显式声明后续Portal/X11热键适配所需的ashpd/x11rb依赖；不引入GTK事件循环，不分配默认键位，不将后端依赖接通冒充完整Linux运行验收。

根pnpm打包入口增加deb与tar.gz，系统依赖入口分别安装/复探目标GNU编译器、Rust目标和图形开发库。交叉pkg-config拒绝宿主库，实际C++链接探针检查ELF架构；全量单元测试保留在真实宿主，临时夹具位于项目本地持久卷，不放宽网络/未知卷安全策略。

最终包绑定主程序、皮肤/生成器、双语许可证与第三方许可资源，真实解包核对文件集合、模式和字节；Debian入口、包架构及glibc需求明确记录。项目自身Harness入口、相关Skills和技术规则统一路由到`docs/LINUX_GPUI_TOOLCHAIN.md`，完整Linux原生能力和目标运行证据继续独立完成。
