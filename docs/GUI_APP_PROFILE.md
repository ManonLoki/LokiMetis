# GUI 应用资料

- 中文名称：诡秘神谕
- English name: LokiMetris
- GUI framework: gpui
- owner: ManonLoki
- Logo: candidate-1 金色神谕之眼；用户在本次 Codex 表单明确选择
- unavailable fixed capabilities: updater / general native dialog / deep_link

```gui-initialization-config
system_tray: enabled
system_notification: enabled
autostart: enabled
about_page: disabled
sponsor_page: enabled
single_instance: enabled
deep_link: disabled
global_shortcut: enabled
sidebar_mode: compact
```

```gui-global-shortcut-contract
{"schemaVersion":1,"actions":[]}
```

## Logo 与图标证据

三个生成请求在发起前绑定稳定顺序 candidate-1 → candidate-2 → candidate-3，三个原始图分别按同序展示。用户明确选择 candidate-1 金色神谕之眼；未检查或标准化其余两个候选。

仅选中项后置验证为 1254×1254、8-bit RGB PNG，可安全解码、有可见像素；保持构图/颜色后缩放为 1024×1024、8-bit RGBA 非交错无损 PNG。原始字节永久保存在 packaging/identity/candidate-1-original.png，仅最终母版进入应用，ICNS/ICO/托盘从同一母版确定性衍生。

| 文件 | 字节数 | SHA-256 |
|---|---|---|
| packaging/identity/candidate-1-original.png | 1722011 | 591717236d2f609cc3104278479f8625936c61dbb5891b450d4d40a4e3821f2f |
| loki_metris_gui/assets/logo.png | 1206313 | 073e1aee636c19b43e1da2e2661514e8834b94dc72b59fb1391723fbfcb25cb8 |
| loki_metris_gui/assets/tray.rgba | 4096 | 404660dd4f5b112a4a224168fbab614d3ca8bfbefcfe2dc2dbd96f83de1eaeae |
| packaging/icons/app.icns | 1635688 | 937c808d0d0dc4c9196f9d740038f805102fd1d58ebb55b70434318cce3e9719 |
| packaging/icons/app.ico | 116871 | ab2dd3433903435f883d5d4c6848f2a622412576a3a4ce2b61a4ff0fcf881286 |
| packaging/macos/background.png | 2303 | 8e98ead127864a4af8422dde938ca6eaa091d25245808bba37ce8db110b1217e |

## UI 设计与宿主基线

产品复刻差异以 [ADR-001](adr/20261009_ADR.md) 批准：保留当前名称、Logo 和独立应用身份；业务导航为用量看板 / AI监控 / 应用换肤，底部设置；参考产品的二级导航与卡片取代中性 88px 页面标题。compact 仍为 80px。参考主题/密度逐阶段通过 Computer Use 校准。应用信息已迁入公共设置，移除旧中性关于入口；赞助区按参考设置页内联，不生成独立导航。设置章节用原生Heading与2／3层级，正文用Text保留可访问名称；本地AccessKit兼容补丁只维护标题映射，来源及许可按固定SHA-256清单核验。通知开关已按本次1:1目标启用，启动不申请系统权限。

中性基线命中标准 gpui-gui-common-v1；当前业务页采用上段已批准的参考产品像素覆盖。侧栏 compact，固定 80px、Logo 36px、图标 22px、居中图标/名称上下排列，不折叠。公共设置已迁入应用信息与统一 Agent 五项开关，语言为中英两项（系统探测仅初值），主题保留浅深/跟随系统三态；有实际保存回执及失败保留旧值。更新日志为受控原生弹窗，普通构建保持真实空状态。公共设置包含通知与自启两列卡，以及原样迁入的历史项目微信/支付宝两张本地收款图；用户本次明确授权为达成1:1目标引入所需资源，图片仅展示，不解码重建、不授权支付。GitHub动作是已知历史源码入口，提示中明确LokiMetis，未宣称为新项目仓库。

标题公式为 {applicationName} v{Cargo.version} {contactChannel}:{contactValue}，名称随 locale，联系人取受管品牌 contacts.windowTitle。应用 identifier=com.loki-metris.app，publisher=ManonLoki；不代表已取得商店/签名资格。系统语言由 sys_locale 读取，未知回退英文；窗口只恢复有效尺寸/位置/最大化，首次/损坏居中 1440×900、最小 960×640。

系统托盘、开机自启、单实例、全局快捷键 enabled；通知与页内赞助 enabled；独立关于页和深链接 disabled。热键 actions=0、初始非空 bindings=0、实际系统注册=0，退出无外部键位需要注销。自启仅有能力，初始化不注册，真实宿主检查后恢复原状态。

## macOS 打包资产与未决事项

中性背景 packaging/macos/background.png 为 660×400 PNG，未来原生 DMG 窗口 660×400，应用/Applications 落点取 GPUI 构建 Skill 的受管布局。已生成背景与平台图标不是已构建 DMG 或布局验收。首次真实产品 GUI 身份流程需确认描述、反馈/分发渠道、DMG 视觉及任何商店元数据；当前均未确定。

## AI监控当前实现

AI监控已接通工作台、监控管理、图片管理与Hooks设置。工作台三秒读取本应用环回监听状态、已接收/失败计数与最近事件；可见失焦窗口继续刷新，隐藏后暂停页面读取，监听与状态机独立运行。管理页使用统一启用集合，保留各 Agent 的位置、四行为图片及多行草稿，原子合并保存不覆盖其它 Agent；图库支持原生文件选择、批量导入、格式筛选与受引用约束的删除。静态图、GIF 与 APNG 使用实际像素，动画支持有限次数、失焦继续及隐藏恢复；纹理边缘保护避免小图放大混入图集邻块。图片只在实际可见位置解码，按App登记图库entry、播放器与桌宠整页需求；一个位置离屏、离页或窗口隐藏不驱逐其它窗口的同ID资源，最后owner离开才释放缓存与所有存活窗口的全部GPU帧。窗口关闭与Entity释放独立收尾，迟到旧entry不影响新entry；保留各位置时间线，显式刷新只重试已失败资源并通知实际owner，健康及pending资源继续共享。解码最多并发2项，CPU保留像素总预算512MiB，不等同进程峰值。Hooks设置按统一Agent集合提供目录草稿、保存、恢复默认及真实写入回执；可复制文件路径只读，未保存草稿禁止写入，异步回执不覆盖后续输入。原子写入只维护本应用marker，保留参考应用及用户配置；Windows WSL宿主路径尚缺真实Windows证据。桌宠已接通独立透明原生窗、六布局／十二位置分页、Hook图文投影、动画和独立深色设置；布局、尺寸、置顶、锁定与位置在应用级串行保存，正常退出排空待提交意图。设置可关闭重开和返回主界面；主窗隐藏不隐藏桌宠，托盘可切换浮窗显示。macOS辅助尺寸赋值与退出重启已实测；无边框窗保留FullSizeContentView，按历史宿主激活并可由原生Window菜单访问。桌宠主体双击／右键已用真实指针观察，方向键消费与悬停保留已补回归，第七次预览已实测方向键、滚轮及原生分页按钮的边界回绕；第八次预览普通解锁拖动仍未改变位置，原生鼠标事件分支正在定位；Windows宿主尚未实测。图库与选择器圆角使用独立原视口绘制接口，图片和棋盘共享圆心，Metal像素与资源回归通过，第11次预览已观察触发区、选项和图库上沿裁切；文字阴影、材质和其它控件仍待收敛。macOS 字体附加笔画膨胀仅在进程内关闭，系统持久偏好不变。

## 应用换肤当前实现

GPUI皮肤页使用统一启用Agent集合筛选Codex／WorkBuddy宿主，提供响应式卡片、搜索、用户资源多选、创建纯主题与生成提示词、原生ZIP选择／预检／信任／导入、导出及转换／删除确认。资源事务属于应用级后台owner；页面离开后预检暂存取消，已接受提交仍完成并清理。宿主操作保留实例身份、代码／外观／Windows整宿主恢复授权，排队操作失去页面授权后不能启动新副作用；已进入事务的补偿不被GUI授权检查阻断。

六套内置皮肤和生成器随包进入macOS `Contents/Resources/skins` 或Windows安装根 `resources/skins`；release只读取包内资源，缺失明确报错。注入runtime、兼容层与事务标记使用LokiMetris独立命名空间，发现原版owner后拒绝接管；自有旧回调停止并保留共享状态清理账本。创建／预检／纯主题提交／导出已有macOS开发预览观察；WorkBuddy已完成当次授权的应用／停用及正常实例恢复，Codex与Windows宿主、最终视觉仍未验收；本地App包内资源实际运行已观察。预览保留参考Card.Section负边距：内部176px高、左右扩16px并上移16px，外层实际可见160px；居中Cover不改原图。安装参考版v0.3.12的实际Brotli嵌入代码／CSS与历史dist完整匹配，六套预览资源三方字节相同。图片圆角沿外卡片1px边框内缘使用19px，选择框同步分区位置；真实窗口几何／图元4项与消费者3项通过。选择框圆角外的绘制／命中仍有真实差异，实物构图待新版对照。

主窗无初始焦点时，Tab／Shift+Tab进入原生tab stop；只在成功获焦后消费首键，已有Input／模态焦点和其它窗口继续原生分发，订阅随主壳释放。数据源弹窗使用独立命名Dialog、Heading、描述及关闭／输入名称，模态打开时隐藏后台辅助树；子按钮拥有自己的Enter动作。键盘8项和弹窗5项／既有1项回归通过，完整新版Computer Use仍待核对。

监控行为卡标题、图片／内容／可选标签、light徽标和22px清除动作已按参考尺寸设置，更换图片保留原trigger动作并增加独立装饰图标。管理／Hooks／工作台显示名称消费core公开目录，Grok协议身份保持原样。桌宠首图定位仅在成功保存后完成；失败延迟重试，等待当前Agent／桌宠／图库版本的新鲜快照，用户手动翻页终止自动定位。上述原生展示与生命周期回归已通过，混字号字形基线、徽标字距和实际新版窗口仍待比对。

macOS 主字体使用历史 @fontsource-variable 5.3.0 的原样 Geist／Geist Mono Latin WOFF2；两字体各自 OFL 与既有静态 OFL 随包保留。Windows 暂沿用原十份静态 TTF。gpui-pre-macos 的完整固定上游17文件仅在 OUT_DIR 应用字重／原生字体身份补丁，不安装系统字体；19项原生与身份回归通过；第14次开发预览已观察实际文字，但完整字形像素仍未验收。未知字体上下文只临时 CFEqual 比较，缓存不持有CFType；首次未命中扫描已有字体，额外变化轴及Windows可变字体仍未验收。

监控补充内容使用真实Kit Textarea的局部外观适配：历史CSS推导两行56.4px、14px／400／1.55、1px边框／12px圆角，暗色#2e2e2e／#424242／占位#696969，浅色白／#ced4da／占位#adb5bd；焦点仅单层品牌描边，100ms标准CSS ease，原生编辑／AX路径继续保留。Hooks目录动作采用历史30px xs内容层，恢复默认为subtle，后台保存仅映射提交操作的loading。真实编辑器／Metal投影和相关Monitor46项通过；第14次预览已观察焦点细边框、多行中文粘贴及撤销恢复。直接中文typeText未插入，真实系统IME组合输入未验收；右键菜单缺少原版系统文字工具，Tab移出／返回已观察但下一焦点可见性与完整顺序仍待修正。


历史sm／xs按钮已补背景和原生内容之间的15%浅／深Gaussian加载遮罩，12px标准差采用有界纹理和数值积分近似；按钮8项含真实Metal层叠与TestAtlas生命周期通过，WebKit精确像素及全按钮按压缩放仍未通过。Management标签使用原生700，与Hooks共同验证十个实际字体消费者；Textarea焦点首次进入按实际ScrollHandle几何最小滚入，既有focus generation识别无绘帧焦点往返；导航／异步有效Agent／真实滚轮代数撤销旧请求。尺寸或缩放变化在原租约有效时等待新布局，仅接受原生Y夹紧，手动滚动优先，内嵌图库不阻挡仍挂载字段。最终Monitor67项（焦点滚动20项）及Agent设置2项通过，同一二进制／347项源码和实际生成Window身份已核对，按钮8项及生产编译通过；下一开发预览仍需实机比对，其它控件的Tab可见性和系统文字服务继续保留差异。
