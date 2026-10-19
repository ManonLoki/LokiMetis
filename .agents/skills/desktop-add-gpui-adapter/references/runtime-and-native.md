# GPUI 运行时与原生能力

只在初始化或修改对应机制时读取。当前产品事实优先于中性 renderer；此处保留装配合同，不触发重生成。

- 根 `[workspace.metadata.agent-first-harness]` 固定记录 `gui-framework = "gpui"`，`interfaces` 含 `gui`，`target-platforms` 来自实际选择。GUI 代码放 `<id>_gui/src/`；有子文件的模块使用 `<module>/mod.rs`。壳层 Entity 只拥有页面、语言、主题与设备侧栏偏好；业务规则、业务状态、值域、权限、稳定错误和跨调用编排仍属于 core，并由端侧直接调用 core 用例；适配器不执行 CLI 或解析 CLI 输出。事件绑定实际 Button 自身。主入口唯一日志订阅器经 1024 行有界后台队列同时写 stderr 和平台标准本地日志目录，按日滚动最多保留 7 份；INFO 以上仅接收自身 GUI/core。只记录结构化生命周期事件和稳定错误类别，禁止用户输入、路径、凭据或业务载荷。`lifecycle` 在 GPUI `on_app_quit` 原生不可取消退出回调中同步完成偏好 worker 刷新 → `application_stopped` → `WorkerGuard` 刷新；同一状态提供 `run` 返回回退且只执行一次。macOS `NSApplication terminate` 不保证 `run` 返回，禁止把唯一收尾放在 `run` 之后，也不让短期异步退出超时丢掉最终日志。初始化失败返回失败退出码。

- 原生窗口第一次或无效状态使用 1440×900、最小 960×640、居中且可调整。后续只恢复有效、与当前显示器相交的尺寸/位置/最大化；不恢复全屏/最小化/隐藏。未选托盘或实际创建失败时不拦截关闭，最后窗口关闭必须实际退出进程；实际创建托盘才关闭隐藏，双语菜单显示/退出。原生服务由 GPUI Global/Task 拥有，退出移除并回收 owned worker；第三方内部线程限制按引用如实报告。GPUI 事件循环留在同步主线程；模板不需要 Tokio，不得为壳层另建或嵌套 runtime。未来业务需要后台异步执行时，复用该适配器拥有的 runtime，不能把整个 GPUI loop 放进 `#[tokio::main]` 或 `block_on` 根任务。

- 系统通知启用才生成默认关闭的应用偏好开关与 Kit 提交入口；不在启动时请求权限或发测试消息，不把 Submitted 当授权/送达。自启启用才生成真实 OS 状态开关、unknown/error/重试；后台串行读写，初始化只读不注册，Windows 限当前用户，无法安全编码的路径失败关闭。单实例启用才用私有目录锁与有界本地 IPC，在第二进程只恢复窗口后退出；不转发业务载荷。全局快捷键启用才声明真实 global-hotkey 能力，独立空 action contract 零默认键位/零注册，后续 typed action 按明确需求调用 core，部分失败保留未释放 owned 资源，退出只注销 owned。固定设置页包含当前语言名称/实际包版本、中文/英文/跟随系统与浅色/深色/跟随系统三态主题。默认语言来自 `sys_locale::get_locale()`，未知语言回退英文；系统主题观察真实 `WindowAppearance`，显式选择覆盖。标题从同一双语名称、Cargo 包版本和受管品牌联系人生成，仅一个小写 `v`。主题使用 Kit 的完整语义颜色，视图不散落判断深浅。设备偏好用有限白名单文本，在事件循环前有界读取；渲染和 handler 零磁盘 I/O。后台有界合并队列写同目录临时文件并替换，原生不可取消退出钩子等待最后快照完成。

- GPUI 当前固定宿主基线与 Tauri 分开报告：真实系统语言、三态主题、受限窗口恢复、最后窗口关闭退出已经实现；updater 与通用原生 dialog 当前 `unavailable`，没有 Tauri plugin、ACL 或伪造实现。新增这些真实宿主能力必须单独建立可复用实现、测试和当前平台的可观察证据，再调整 capability facts；不能用 stub 提升为 supported。
