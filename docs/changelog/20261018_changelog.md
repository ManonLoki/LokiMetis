# 2026-10-18 开发变更

## LINUX-NATIVE-TOOLCHAIN-001 · Linux GNU双架构构建与打包工具链

分类：feature。required_version：`0.3.1`。本发布周期功能锁已开启，本事件不重复提升Minor，不构成Git发布。

目标平台增加Linux，支持GNU x64/ARM64的原生与同系统交叉架构工具链。Linux GUI启用现有tray-icon的KSNI后端，并显式声明后续Portal/X11热键适配所需的ashpd/x11rb依赖；不引入GTK事件循环，不分配默认键位，不将后端依赖接通冒充完整Linux运行验收。

根pnpm打包入口增加deb与tar.gz，系统依赖入口分别安装/复探目标GNU编译器、Rust目标和图形开发库。交叉pkg-config拒绝宿主库，实际C++链接探针检查ELF架构；全量单元测试保留在真实宿主，临时夹具位于项目本地持久卷，不放宽网络/未知卷安全策略。

最终包绑定主程序、皮肤/生成器、双语许可证与第三方许可资源，真实解包核对文件集合、模式和字节；Debian入口、包架构及glibc需求明确记录。项目自身Harness入口、相关Skills和技术规则统一路由到`docs/LINUX_GPUI_TOOLCHAIN.md`，完整Linux原生能力和目标运行证据继续独立完成。

## 桌面可靠性修复

本组均为既有范围的独立bug-fix，版本由受管门禁逐项记录：

| change_id | required_version | 行为变化 |
| --- | --- | --- |
| `LMDESK-HOOKS-CACHE-GATE-001` | `0.3.2` | 桌宠先加载设置时也发布Agent启用集合及Hooks目录，缓存命中不再让listener沿用空门禁。 |
| `LMDESK-LINUX-DOCK-ICON-001` | `0.3.3` | 活X11窗口提供已选Logo像素，app-id、WM_CLASS及Debian桌面身份一致。 |
| `LMDESK-LINUX-TRAY-RESTORE-001` | `0.3.4` | 后台确认真实Watcher及本进程恢复入口；主窗隐藏后入口消失会恢复主窗。 |
| `LMDESK-LINUX-PACKAGE-PERMISSIONS-001` | `0.3.5` | deb/tar发行权限不受调用者umask影响，最终解包核验目录与执行入口权限。 |
| `LMDESK-LINUX-DEPENDENCY-ROOT-001` | `0.3.6` | 提权依赖入口只处理系统阶段，原用户Rust接续保持明确待完成状态，避免误操作root工具链。 |
| `LMDESK-LINUX-PET-HOST-001` | `0.3.7` | X11/XWayland桌宠接通置顶、几何、锁定及输入穿透；异步原生确认后保存，关闭设置和退出保留已接受意图。 |
| `LMDESK-LINUX-NOTIFICATIONS-001` | `0.3.8` | 接通freedesktop通知、有限队列和点击归属；权限重试恢复发送，关闭/退出同步撤销尚未提交的命令。 |
| `LMDESK-STATIC-IMAGE-FIRST-FRAME-001` | `0.3.9` | 静态图片在prepaint就绪后主动请求首帧，不再等待鼠标、图库刷新或动画计时器。 |
| `LMDESK-LINUX-TRAY-CLOSE-BORROW-001` | `0.3.10` | 主窗关闭使用已捕获的X11能力，避免重复借用窗口导致有托盘时仍退出。 |
| `LMDESK-LINUX-HICOLOR-ICON-001` | `0.3.11` | Debian图标实际派生512像素并安装到标准hicolor索引目录，修复GNOME主题找不到1024目录中的图标。 |

相关开发回归覆盖五种Agent真实生成命令、私有D-Bus协议与服务换代，以及真实X11窗口属性；这些证据不替代最终候选E2E。纯Wayland桌宠、跨架构运行及系统横幅等限制继续在Linux知识库与技术债中披露。

Harness本地维护补齐Linux打包能力判定、受限umask与提权接续回归，并对已封存Archify生成物执行精确路径、双摘要及回执校验。独立GPUI来源验证器按真实组装顺序展开共享及模块补丁，拒绝未知表达式，并核对全部五个包的最终生成摘要。未取得独立上游源，不创建来源锁或声称上游同步完成。
