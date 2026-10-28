# 本地 GPUI 圆角视口兼容层

本目录以 `provenance.json` 记录的官方 0.3.10 `.crate`（Zed revision 08cbb88）为完整不可变来源。
`upstream/` 保留原文件、许可证与目录关系；`source-content.sha256` 覆盖全部原文件，路径相对 upstream 根。

人工维护入口为 Cargo.toml、src/lib.rs、build.rs、patches/ 和本说明。
共享标准库生成器同时验证固定清单 SHA、每个原文件 SHA、完整文件集、唯一替换片段和生成结果 SHA。
补丁仅写入 Cargo OUT_DIR/gpui-patched，不编辑 upstream，也不把整个 vendor 目录排除于工程门禁。

`PolychromeSprite` 末尾新增 16 字节圆角视口 bounds；原图片矩形、祖先矩形 mask、UV、透明度和图集键保持独立。
原 paint_image 与 emoji 沿用原几何，新入口 ImageRoundedClip/paint_image_with_rounded_clip 显式保存原始 viewport。
Metal、HLSL、WGSL/WebGL 必须同时使用这一 ABI；macOS 验证不能替代 Windows 实际渲染验证。

维护验证：

```sh
node vendor/gpui-patch-support/verify-source.mjs vendor/gpui-pre vendor/gpui-pre-apple vendor/gpui-pre-windows vendor/gpui-pre-wgpu
rustc --edition=2024 --test vendor/gpui-patch-support/generator.rs -o /tmp/loki-gpui-generator-tests
/tmp/loki-gpui-generator-tests --test-threads=1
```

生成验证不是产品候选验收；真实 Window 与 GPU 像素测试须使用相应已启用 test-support 的目标。

Linux 创建窗口时，公共层只在 `WindowOptions.show=true` 才调用平台的首次 `map_window`，
避免 X11 后端忽略隐藏参数后直接显示主窗。其它平台保留既有映射路径，不改变图元 ABI。
test-support 记录平台实际收到的映射请求数，覆盖隐藏创建零请求及普通创建一次请求；
后续显式显示仍由产品原生窗口桥负责，真实登录会话和窗口管理器行为需另行实测。
本补丁仅匹配原样 GPUI 0.3.10（2026-10-28 复核：X11 `map_window` 仍不读取 `show`，公共层仍无条件映射，故保留），升级时若上游已正确处理隐藏创建则移除并重跑相关回归。

`Label` 的 `aria_label` 在未指定 `aria_value`、且节点尚无 value 时同时填入 value，
供 Windows AccessKit 的静态文本名称读取。显式值（包括空字符串）、已有节点文本、
其他角色以及原 label 保持其优先级；真实工作台 AX 树回归覆盖动态计数与最近事件。

焦点只读入口 `Window::focus_generation()` 暴露上游已有代数；真实focus／blur增长，同一目标重复focus保持原值，不创建额外焦点状态。用于识别两帧之间离开后返回的输入，避免仅采样最终bool漏掉新的滚入请求。上游与布局／Scene ABI保持不变。


## Deferred AX 回归观测

TestWindow 现在保留生产注册的真实 A11yCallbacks；test-support 的
TestAppContext::simulate_accessibility_activation/deactivation 只调用这些回调，
不直接改 Window 标记、不创建原生窗口。已有 debug_a11y_tree_json 的 aria.hidden
来自实际 TreeUpdate 节点，测试必须沿真实 children 查祖先，不能用控件 snapshot 代替。
此增量只提供真实 RED 观测，不改变 defer_draw 行为；固定模板和生成 SHA 仍由 build.rs 校验。


## opt-in defer_draw_in_parent

`Window::defer_draw_in_parent`保持deferred真正prepaint/Hitbox/paint的优先级，
同时以无节点的Node/Reservation子序列保存原AX父链和自然顺序，并应用原mask到prepaint。
连续同父兄弟、相反priority、nested及空预约均由独立槽展平，不用单一插入下标。
legacy `defer_draw` 不创建预约、仍使用原浮层AX根与prepaint mask行为。
AX祖先在deferred执行时读取本帧完成后的节点，包含稍后设置的Hidden；最终树无预约节点。

只对含/曾含opt-in deferred的cached view设置私有生命周期标记；AX active时该range重建，
inactive仍沿原layout/paint/dispatch范围replay，并清除旧帧AX token。range扫描含nested/replayed条目；复用准入再沿上一帧已完成deferred ranges作有界可达查询，
覆盖普通deferred稍后才创建opt-in后代的情况。
当前产品Shell/Monitor/Surface及Kit相关输入、Button、Dialog路径无`.cached(...)`调用；
这不替代对公开API的真实cache/activation回归。其它legacy cache不因本补丁全局禁用。
Root已冻结父圆角迁入包不修改；未来组合必须同时保留getter、本组AX/mask字段与parent clip链。


## opt-in with_hitbox_exclusions

`Window::with_hitbox_exclusions(&[bounds], closure)`只排除prepaint期间的真实覆盖矩形。
嵌套scope追加并恢复原列表；Frame把可见输入区域划分为互斥矩形，共享原HitboxId。
owner返回的Hitbox保持完整bounds/content_mask，Frame分片只收窄自身输入content_mask；
当前该字段仅被Frame.hit_test读取，Window.content_mask、Scene/AX bounds及图片visibility不变。
右/下半开边界避免重复ID；空/全遮区域不登记分片；默认无排除列表沿原O(1)路径。
View cache比较精确排除列表；new-parent deferred保存并replay几何；legacy popup/Dialog不继承。
因此可保留保存栏下12px、初始页头上方及横向露出缝隙的实际输入，半透明栏底图仍完整绘制。
仅test-support增加hit_test_ids_for_testing只读实际Frame ID列表，测试不得用bool替代重复ID oracle。
此前rectscope没有产品消费者且无法表达覆盖补集，本增量明确移除它全部15pairs/30fragments，
不把无用public API留入正式提交。原AX与rectscope冻结证据不改写，现单独记录替代源/回执。
