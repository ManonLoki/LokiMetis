# Linux 原生宿主方案

只在修改Linux窗口、托盘、通知或热键机制时读取。这里收录已有产品验证过的工程做法；中性renderer仍拒绝生成Linux托盘/热键，产品实现不能据此改判为模板已支持。当前产品方案、精确版本和验证状态统一取[Linux工程知识库](../../../../docs/LINUX_GPUI_TOOLCHAIN.md#已验证能力与证据索引)。

## 图形会话与窗口

启动前捕获会话能力；产品需要全局定位/置顶且存在`DISPLAY`时可以选择X11/XWayland，不改写进程环境。纯Wayland普通toplevel不能承诺同样的全局定位和置顶；未实现协议路径明确失败并保留恢复入口，不能用激活代替置顶。

X11使用真实XID及checked request。几何以root全局物理坐标保存，仅在原生边界转换DIP；工作区取RandR活动显示器与当前桌面`_NET_WORKAREA`交集，缺协议时才回退root几何。混合DPI及热插拔需单独实测。

EWMH置顶是窗口管理器协商状态。首次map后重新请求并异步读回，以实际覆盖关系验证；轮询有总预算且不阻塞主循环。确认后释放临时几何/显隐目标，避免旧目标拦住拖动或单独切换穿透。Shape Input与视觉区域分别拥有；写后读回，失败尝试恢复，恢复失败保留未知状态。只有原生效果确认才提交偏好；退出接管已接受但尚未提交的最后意图。

## 托盘与关闭恢复

KSNI/StatusNotifierItem使用会话D-Bus，不在GPUI主线程混入GTK循环。创建对象不证明能恢复：后台有界worker联合核对Watcher存在真实host、注册列表包含本进程拥有的服务，以及精确托盘Id。owner变化、注销、断连或未知结果撤销旧的可用性；主线程只读已发布事实。

关闭回调已经借用当前GPUI窗口，不可在其中再次update同窗以取得XID。在安装机制时捕获所需原生能力，关闭及延迟隐藏阶段仍重新核对恢复入口；缺少可靠入口时允许正常关闭，不能隐藏到不可恢复状态。最终验收操作真实托盘恢复/退出，D-Bus属性读回不代替点击。

## Dock与桌面身份

窗口`app_id`、桌面文件标识、`StartupWMClass`及图标名应来自同一产品身份。已批准Logo派生窗口ARGB图标及真实尺寸hicolor PNG；`_NET_WM_ICON`读回只证明窗口属性，Dock还需能解析有效`.desktop`和`Exec`。隔离解包运行时，宿主未必能访问包内路径；临时宿主映射必须精确登记并恢复。最终包解包检查和真实Dock呈现分别取证。

## freedesktop通知

固定会话服务`org.freedesktop.Notifications`的`GetCapabilities`只证明服务可用；该协议没有每应用授权查询。不可达和超时不伪装为用户拒绝。启动不投递测试消息，实际发送遵守应用偏好及core事件规则。

worker拥有有界提交邮箱与通知ID映射，连接、认证及调用共享总预算。关闭、权限暂停和退出撤销未提交命令；确定拒绝和未知回执分别处理，超时不盲目重投。服务唯一owner变更后丢弃旧ID，只处理本owner已发送通知的`default`动作；未知动作不恢复窗口。正文按纯文本转义，通知桌面入口及图标沿用产品身份。API Accepted、真实横幅与点击恢复分别验证。

## 热键与回归边界

Wayland全局快捷键需要GlobalShortcuts Portal及真实系统授权；X11注册需要checked request和确定错误传播。`global-hotkey 0.8`的Linux断连回执不能当可靠注册成功。依赖声明及空action合同不构成后台服务或按键验证，也不授权注册键位。

协议回归使用owned私有D-Bus daemon，不改变用户session bus；真实X11测试的ignore状态不等于已执行。按改动选择`native/tray_linux_tests.rs`、`native/notifications/linux*_tests.rs`、`pet/host/linux_tests.rs`及`native/linux.rs`中的相关回归；完整候选再覆盖真实桌面交互。产品源路径位于`loki_metris_gui/src/`，其他下游使用各自对应模块，不复制产品状态或业务规则。
