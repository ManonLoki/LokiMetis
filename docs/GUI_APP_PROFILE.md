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

AI监控已接通工作台、监控管理、图片管理与Hooks设置。工作台三秒读取本应用环回监听状态、已接收/失败计数与最近事件；可见失焦窗口继续刷新，隐藏后暂停页面读取，监听与状态机独立运行。管理页使用统一启用集合，保留各 Agent 的位置、四行为图片及多行草稿，原子合并保存不覆盖其它 Agent；图库支持原生文件选择、批量导入、格式筛选与受引用约束的删除。静态图、GIF 与 APNG 使用实际像素，动画支持有限次数、失焦继续及隐藏恢复；纹理边缘保护避免小图放大混入图集邻块。图片只在实际可见位置解码，最后一个位置离屏、离页或窗口隐藏后释放缓存与全部GPU帧；保留时间线，显式刷新可重试失败资源。解码最多并发2项，CPU保留像素总预算512MiB，不等同进程峰值。Hooks设置按统一Agent集合提供目录草稿、保存、恢复默认及真实写入回执；可复制文件路径只读，未保存草稿禁止写入，异步回执不覆盖后续输入。原子写入只维护本应用marker，保留参考应用及用户配置；Windows WSL宿主路径尚缺真实Windows证据。桌宠已接通独立透明原生窗、六布局／十二位置分页、Hook图文投影、动画和独立深色设置；布局、尺寸、置顶、锁定与位置在应用级串行保存，正常退出排空待提交意图。设置可关闭重开和返回主界面；主窗隐藏不隐藏桌宠，托盘可切换浮窗显示。macOS辅助尺寸赋值与退出重启已实测；无边框窗保留FullSizeContentView，按历史宿主激活并可由原生Window菜单访问。桌宠主体双击／右键已用真实指针观察，方向键消费与悬停保留已补回归，第七次预览已实测方向键、滚轮及原生分页按钮的边界回绕；第八次预览普通解锁拖动仍未改变位置，原生鼠标事件分支正在定位；Windows宿主尚未实测。图库与选择器圆角使用独立原视口绘制接口，图片和棋盘共享圆心，Metal像素与资源回归通过，第11次预览已观察触发区、选项和图库上沿裁切；文字阴影、材质和其它控件仍待收敛。macOS 字体附加笔画膨胀仅在进程内关闭，系统持久偏好不变。

## 应用换肤当前实现

GPUI皮肤页使用统一启用Agent集合筛选Codex／WorkBuddy宿主，提供响应式卡片、搜索、用户资源多选、创建纯主题与生成提示词、原生ZIP选择／预检／信任／导入、导出及转换／删除确认。资源事务属于应用级后台owner；页面离开后预检暂存取消，已接受提交仍完成并清理。宿主操作保留实例身份、代码／外观／Windows整宿主恢复授权，排队操作失去页面授权后不能启动新副作用；已进入事务的补偿不被GUI授权检查阻断。

六套内置皮肤和生成器随包进入macOS `Contents/Resources/skins` 或Windows安装根 `resources/skins`；release只读取包内资源，缺失明确报错。注入runtime、兼容层与事务标记使用LokiMetris独立命名空间，发现原版owner后拒绝接管；自有旧回调停止并保留共享状态清理账本。创建／预检／纯主题提交／导出已有macOS开发预览观察；WorkBuddy已完成当次授权的应用／停用及正常实例恢复，Codex与Windows宿主、最终视觉仍未验收；本地App包内资源实际运行已观察。预览视口为176px、居中Cover：安装参考版v0.3.12的实际Brotli嵌入代码与历史dist完整匹配，已纠正此前160px的判断。六套预览资源在三方字节相同；四列／两列及1x／2x真实窗口边界回归通过，实物构图仍待新版对照。

主窗无初始焦点时，Tab／Shift+Tab进入原生tab stop；只在成功获焦后消费首键，已有Input／模态焦点和其它窗口继续原生分发，订阅随主壳释放。数据源弹窗使用独立命名Dialog、Heading、描述及关闭／输入名称，模态打开时隐藏后台辅助树；子按钮拥有自己的Enter动作。键盘8项和弹窗5项／既有1项回归通过，完整新版Computer Use仍待核对。

监控行为卡标题、图片／内容／可选标签、light徽标和22px清除动作已按参考尺寸设置，更换图片保留原trigger动作并增加独立装饰图标。管理／Hooks／工作台显示名称消费core公开目录，Grok协议身份保持原样。桌宠首图定位仅在成功保存后完成；失败延迟重试，等待当前Agent／桌宠／图库版本的新鲜快照，用户手动翻页终止自动定位。上述原生展示与生命周期回归已通过，混字号字形基线、徽标字距和实际新版窗口仍待比对。
