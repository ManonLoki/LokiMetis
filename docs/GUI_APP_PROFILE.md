# GUI 应用资料

- 中文名称：诡秘神谕
- English name: LokiMetris
- GUI framework: gpui
- owner: ManonLoki
- Logo: candidate-1 金色神谕之眼；用户在本次 Codex 表单明确选择
- unavailable fixed capabilities: updater / general native dialog / deep_link

```gui-initialization-config
system_tray: enabled
system_notification: disabled
autostart: enabled
about_page: enabled
sponsor_page: disabled
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

产品复刻差异以 [ADR-001](adr/20261009_ADR.md) 批准：保留当前名称、Logo 和独立应用身份；业务导航为用量看板 / AI监控 / 应用换肤，底部设置；参考产品的二级导航与卡片取代中性 88px 页面标题。compact 仍为 80px。参考主题/密度逐阶段通过 Computer Use 校准。旧中性关于页暂保留至应用信息迁入公共设置后移除；原生能力缺口未实现前保持真实 unavailable，不把计划当已启用。

中性基线命中标准 gpui-gui-common-v1；当前业务页采用上段已批准的参考产品像素覆盖。侧栏 compact，固定 80px、Logo 36px、图标 22px、居中图标/名称上下排列，不折叠。公共设置已迁入应用信息与统一 Agent 五项开关，语言为中英两项（系统探测仅初值），主题保留浅深/跟随系统三态；有实际保存回执及失败保留旧值。关于页暂显示真实 Cargo 版本、负责人、受管品牌联系人、免责声明及中性更新/日志入口，尚无通知开关或赞助页。

标题公式为 {applicationName} v{Cargo.version} {contactChannel}:{contactValue}，名称随 locale，联系人取受管品牌 contacts.windowTitle。应用 identifier=com.loki-metris.app，publisher=ManonLoki；不代表已取得商店/签名资格。系统语言由 sys_locale 读取，未知回退英文；窗口只恢复有效尺寸/位置/最大化，首次/损坏居中 1440×900、最小 960×640。

系统托盘、开机自启、单实例、全局快捷键 enabled；通知、赞助、深链接 disabled。热键 actions=0、初始非空 bindings=0、实际系统注册=0，退出无外部键位需要注销。自启仅有能力，初始化不注册，真实宿主检查后恢复原状态。

## macOS 打包资产与未决事项

中性背景 packaging/macos/background.png 为 660×400 PNG，未来原生 DMG 窗口 660×400，应用/Applications 落点取 GPUI 构建 Skill 的受管布局。已生成背景与平台图标不是已构建 DMG 或布局验收。首次真实产品 GUI 身份流程需确认描述、反馈/分发渠道、DMG 视觉及任何商店元数据；当前均未确定。

## AI监控当前实现

AI监控已接通工作台、监控管理与图片管理。工作台三秒读取本应用环回监听状态、已接收/失败计数与最近事件；可见失焦窗口继续刷新，隐藏后暂停页面读取，监听与状态机独立运行。管理页使用统一启用集合，保留各 Agent 的位置、四行为图片及多行草稿，原子合并保存不覆盖其它 Agent；图库支持原生文件选择、批量导入、格式筛选与受引用约束的删除。静态图、GIF 与 APNG 使用实际像素，动画支持有限次数、失焦继续及隐藏恢复；纹理边缘保护避免小图放大混入图集邻块。Hooks设置和桌宠尚在实施，圆角裁切、材质、控件与资源缓存仍待收敛。macOS 字体附加笔画膨胀仅在进程内关闭，系统持久偏好不变。
