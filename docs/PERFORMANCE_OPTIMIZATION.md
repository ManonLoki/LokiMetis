# 性能优化参考

先确认运行产物和场景，再用调用栈、事件频率及行为回归定位重复工作。本项目已核实的经验是：无变化写入会放大后台刷新，防抖需要区分最后意图与在途任务，退出需要保护最终位置，原生图库不必携带 Base64 展示副本，材质动画可复用精确的固定几何。这些经验支持后续排查，不构成长期内存、跨框架性能或完整产品体验的验收结论。

整理日期：2026-10-10，最近补充：2026-10-11。初稿依据 Task 1 已核对的保存证据与已提交源码编写，初稿整理阶段没有重新编译、启动应用或采样。后续案例分别依据其实现和开发检查更新；各轮产物与运行条件单独保留，不能把历史数值当作当前版本的性能承诺。

## 1. 事实基线与证据入口

Task 1 历史案例的源码基线 `C` 为 `1fc34da10935802c58a6a29072a57bac458a5de0`，其父提交 `P` 为 `aa5ec10e6bc165231b863dd37ffa3bdb46bb8796`。相对源码链接用于导航；工作树日后改变时，可用 `git show 1fc34da10935802c58a6a29072a57bac458a5de0:仓库相对路径` 定位历史实现。第5.5节起为后续独立案例，不属于 `C` 的实现或测试结果。

| 资料 | 项目相对位置或稳定标识 | 用途 |
|---|---|---|
| Task 1 事实清单、核对数据 | Task key `loki-performance-evidence-20261010`；该 Task Worktree 中 `release/performance-evidence-20261010/report.md`、`evidence-audit.json` | 实现事实 F1–F8、运行观察 O1–O4、日志结果与指纹差异 |
| 原始测试与测量集合，下文简称 `E` | `release/gpui-parity/stage11-performance-validation/` | `index.json`、各轮 `*-results.json`、原始日志、top/footprint/sample |
| Preview16、Preview17 构建收据 | `release/gpui-parity/stage-11-preview16.json`、`stage-11-preview17.json` | 源码字段、dirty 状态、平台、可执行摘要、预览性质 |
| 运行及场景收据 | `E/preview17-runtime-receipt.json`、`preview17-gallery-rendering-receipt.json` | PID、bundle SHA、页面、采样前后状态、独立调用栈时间 |
| 旧组结果及草稿保全 | `release/gpui-parity/performance-preview16-before-repeat.json`、`stage-11-preview17-draft-preservation.json` | 旧组条件；测量未误保存用户草稿的边界 |

这里的 `E/` 是上述目录的简写。证据位于忽略的 `release/`，并不随源码提交分发；Task 1 产物保存在其独立 Worktree，原始集合保存在保存项目。新 checkout 缺少它们时，应按 Task key 和相对目录寻找保留集合；找不到就标记历史运行证据不可用，不补造记录。本文不依赖某个用户名绝对路径或临时目录才能解释实现。

Task 1 核对了 98 个证据文件并复算四组采样；其中收据引用核对为 index 56 项、runtime 18 项、gallery 3 项。这些集合并非可直接相加的独立文件计数。其报告 SHA-256 为 `e83279edb32ed5aa633be99da437f8edf74b17930cc418d8b4ef3bf8ee077ae2`。核对只证明保存资料内部一致，没有重新执行或独立认证历史实验。

阅读时区分三种强度：

- **实现事实**：在 `C` 中可定位的机制，以及保存日志中实际执行的非空行为回归。
- **有限观察**：指定二进制、页面、负载和时间窗口里的 CPU、footprint、调用栈。
- **待验证方向**：从观察提出的归因或方案，尚无受控实验或真实行为证据。

## 2. 按症状选择排查路径

| 症状 | 首先区分 | 优先检查 | 能支持结论的证据 |
|---|---|---|---|
| 静置页面持续占 CPU | 真正静置、动画播放、后台扫描或修复 | 唤醒频率、相同状态反复写入、revision 到刷新路径 | 同场景 CPU 数列、调用栈、写入/刷新计数 |
| 拖动或 bounds 通知引起 IO/抖动 | 逻辑坐标变化与物理像素实际变化 | DPI 量化、防抖截止时间、重复目标、在途回执 | 相同物理像素及 A→B→A 回归、真实存储结果 |
| 停止拖动后位置未保存 | 截止时间被不断延期、失败、队列丢意图 | desired/in-flight/成功回执的状态转换 | 到期、失败、旧快照及首次保存回归 |
| 图库播放 CPU 高 | 元数据构建、编码、解码、布局、绘制分别发生在哪一段 | 数据表示、可见素材和动画负载、layout/paint 栈 | 固定素材的同步采样、分阶段计数、单变量对照 |
| 内存升高或离页后回落慢 | 当前 footprint、RSS、历史峰值、allocator 活分配 | 页面资源 owner、缓存、原始字节、解码帧、GPU 资源 | 重复进出页面序列及资源/活对象生命周期 |
| 退出后重启位置回退 | 尚未派发与已接受 blocking 工作 | 最终意图屏障、锁内读取时序、重复 finish | 超过关闭等待预算的晚到写入回归 |

一次截图只能提出症状。页面名称、进程身份或素材状态没有被确认时，先补条件；不要直接按最高内存值或某个热栈名称决定修复。

## 3. 让测量可重复

### 3.1 固定输入与产物身份

每轮先保存一条完整记录，字段至少包含：

| 类别 | 必须记录的条件 |
|---|---|
| 源码 | 完整 commit、dirty 状态；有未提交输入时的逐文件摘要/清单与范围 |
| 构建 | target、debug/release、优化级别、debug assertions、工具链和实际构建参数 |
| 运行 | bundle 内可执行 SHA-256、实际 PID、启动时间、相关 helper 的可证明归属 |
| 设备 | 系统、硬件、显示器、缩放、窗口大小与位置、电源条件 |
| 产品状态 | 页面、语言、主题、工具开关、通知/自启、扫描状态、Pet 状态 |
| 素材 | 数量、原始字节、尺寸、动画帧和播放状态；已存资料与未存草稿分别保全 |
| 采样 | 预热时长、起止时间、间隔、原始样本、排除规则、并发构建/扫描负载 |

commit 不能代表 dirty 构建的全部输入；Cargo 产物摘要也不能代替 bundle 中实际运行文件的摘要。采样前后确认同一 PID 和可执行，记录页面与播放状态。前后均无 Cargo 只能证明两个时点，不能自动推导整个区间没有编译或状态切换。

对照实验统一预热、窗口时长、采样周期、素材及后台负载，并重复同一操作序列。要归因某项优化，应只改变该项；同时修改写入、队列、图库和清理时，只能描述组合结果。跨实现比较还要匹配所有 helper 的归属与负载，不能只比主进程。

### 3.2 记录原始序列与独立时间

历史 30 秒组使用 `top -l 16 -s 2 -pid <PID> -stats pid,cpu,mem` 与 `footprint --pid <PID> --sample 5 --sample-duration 30 -j <输出.json>`；50 秒组对应 `top -l 26` 和 `--sample-duration 50`。这是已有命令形状，尖括号项须由后续获授权测量的真实 PID、输出位置替换，本文没有执行它们。

这些 top 记录删除第一个 0.0 样本后，对其余 PID 行 CPU 值取算术均值；保留旧组的 0.3 和图库的尖峰，不挑选有利样本。新实验需预先说明首样本规则，不能按结果临时剔除。保留样本数、范围和原始时间；采样间隔有偏差时，不把简单均值写成精确的时间加权结果。

physical footprint 从 footprint JSON 的 process `footprint` 原始字节除以 `1048576` 得到 MiB。记录其每个采样时点，不能用另一时点的 vmmap 最低值替换时间序列。sample、vmmap 与 top/footprint 若不在同一窗口，只能作为独立证据关联；同时采样本身也有观测开销。

### 3.3 指标的含义与限制

| 指标 | 用于回答 | 不能直接推出 |
|---|---|---|
| CPU 百分比 | 该工具口径下的处理器消耗及尖峰 | 主线程延迟、根因或单项优化收益；不按“单进程必在 0–100%”截断原值 |
| physical footprint 当前值 | 系统对进程当前内存负担的记账观察 | Rust 活对象总量、泄漏已消除、全部 GPU/共享资源的精确归属 |
| footprint peak | 进程生命周期内记录的高水位 | 当前仍占同样内存；当前值回落而 peak 不降并不矛盾 |
| RSS / resident | 工具口径下驻留内存页面的观察 | 与 footprint、top MEM 完全相同，或与 allocator 活分配相等 |
| 虚拟 SIZE | 地址空间映射规模 | 实际物理占用或活对象字节 |
| allocator allocated / fragmentation | 分配器区域内活分配和碎片/保留空间的线索，须保留原字段口径 | 进程全部资源、GPU 占用或泄漏因果 |
| 原始图片字节预算 | 应用持有的压缩/原始资源字节上界 | 解码帧、纹理、布局对象与 allocator 驻留的总上界 |

释放活对象后系统记账可能不会立即等幅下降；短时回落也可能发生在仍有泄漏的进程。诊断泄漏要固定负载重复进入/离开，观察恢复基线是否持续漂移，再用 owner、引用或活分配证据定位未释放资源。活动监视器的一张截图不能承担这个结论。

## 4. 从调用栈走到可检验的机制

一次操作的成本可从“每次工作成本”和“触发频率”两端排查。先沿实际路径画出谁触发谁，再判断是否存在不必要的重复；低成本操作被高频触发，也可能持续消耗 CPU 和 IO。

本项目重复刷新路径为：

```text
bounds 通知 → 位置保存 → pet_revision 变化
             → Pet refresh → 设置/档案/图库读取 → 原生目录构建
```

对应入口：[位置队列](../loki_metris_gui/src/pet/position/mod.rs)、[设置写入](../loki_metris_gui/src/monitor/settings.rs)、[保存及 revision](../loki_metris_gui/src/usage_service/agents.rs)、[Pet refresh](../loki_metris_gui/src/pet/mod.rs)、[后台目录读取](../loki_metris_gui/src/usage_service/monitor_catalog.rs)。

建议按下列顺序形成可检验结论：

1. 在已确认的页面和操作窗口采集调用栈，定位 IO、编码、布局或绘制所在路径。
2. 核对输入状态、触发来源和频率；同值通知是否改变 revision，刷新是否真的需要重新读取/编码。
3. 提出可被反例推翻的假设，例如“相同物理位置仍增加 revision”，并用真实 Controller/后台 store 路径构造回归。
4. 先证明原行为失败，再修正机制，验证非空筛选通过；同时加入错误实现应失败的负向回归。
5. 用相同场景重新测量成本与触发次数，分别报告正确性与性能；源码机制成立不等于收益已完成定量归因。

历史 Preview16 的独立 5 秒 sample 可见 `monitor_catalog_work`、`save_pet_blocking`、`encode_base64`，与上述机制一致。Preview17 Settings 的独立 sample 未观察到这三个符号，仍有 Hook repair 与 JSON 合并/序列化；有限样本未出现不能证明路径永久不执行，文本出现次数也不是 CPU 占比。图库另一次 5 秒 sample 可见 Taffy layout、元素状态/ID 和绘制；它与图库 30 秒测量不同步，只能提供待验证方向。

## 5. 已核实的实现案例与正确性约束

### 5.1 合并无变化写入，避免 revision 反馈

`update_pet_settings_if_changed` 在同一设置写锁内读取最新磁盘值，再计算 Pet 偏好和位置。两者均未改变时返回当前设置及 `changed=false`，跳过文件替换；这仍包含读取和解析，不是整个应用零 IO。

`save_pet_blocking` 在实际变更和成功 no-op 后都更新完整内存设置缓存，仅实际变更增加 `pet_revision`。因此优化切断的是“无变化保存 → revision → 重复刷新”，不能通过跳过缓存更新来实现。回归覆盖已知 Agent、目录、Pet 字段取最新磁盘值；未知 JSON 字段逐字保留只在 no-op 分支得到覆盖，不能外推到实际改写。

参考：[store 回归](../loki_metris_gui/src/usage_service/pet.rs) 中 `repeated_physical_position_is_a_noop_but_return_after_b_is_committed`、`position_commit_and_noop_refresh_cache_from_latest_disk_fields`。

应保留的约束：

- 重复物理位置不增加 revision；实际位置/偏好变化仍保存并通知。
- 不覆盖最新磁盘上的其他已知字段，不重新启用被关闭的 Agent。
- 实际变更和 no-op 都刷新正式缓存读取结果。
- “从不更新缓存”和“只在 changed 时更新缓存”两个错误变体都应失败。历史各有 1 项真实失败；仅检查内部计数不足以证明缓存正确。

### 5.2 防抖合并最后意图，等待正式回执

位置先按 DPI 换算并 `round` 到物理像素。不同位置静止满 200ms 才提交，相同物理目标不重置截止时间，防止连续同值 bounds 通知使保存永不到期。

队列分别保存 desired、in-flight 和最近成功写入回执；单个 driver 等正式后台回执完成后再提交下一位置。发起成功不等于磁盘保存成功，展示快照也不能替代写入回执。该队列状态服务于 GUI 写入协调，持久化权威仍是设置。

| 时序 | 必须保留的行为 |
|---|---|
| 已保存 A；B 尚未被接受；又回 A | 可取消尚未发生的 B 写入 |
| 已保存 A；B 已在途；又回 A | 保留恢复 A 的意图，不能因当前快照仍是 A 而取消 |
| B 已成功，旧展示快照 A 晚到；随后回 A | 不能吞掉 B→A 的真实恢复写入 |
| 首次 saved=None | 首次有效位置仍需要保存 |
| 写入失败且出现更新位置 | 旧失败不丢失较新意图 |
| 同一目标失败 | 保留最终意图，等待后续事件或退出重试，避免后台忙循环 |

参考：[队列回归](../loki_metris_gui/src/pet/position/tests.rs)、[Controller feedback 回归](../loki_metris_gui/src/pet/feedback_tests.rs)。测试需观察到期时间、正式回执和最终存储值，不能只证明某个任务字段被清空。

### 5.3 退出最终位置防止晚到写入覆盖

正常退出时，首次实际取走 runtime 的 finish 用 `OnceLock` 接管非空最终位置。位置任务获得原 mutation 锁之后才读屏障；若晚到任务原想写 B，而退出最终意图为 A，它应改用 A。锁前捕获旧屏障会留下竞态。

参考：[屏障与保存](../loki_metris_gui/src/usage_service/agents.rs)、[finish_with_pet](../loki_metris_gui/src/usage_service/mod.rs)、[退出回归](../loki_metris_gui/src/usage_service/agents_shutdown_tests.rs)、[GUI 最终收据回归](../loki_metris_gui/src/pet/shutdown_receipt_tests.rs)。

约束包括：晚到的已接受写入不能覆盖最终 A；重复 finish 不替换首次退出命令；首次 None 不设位置屏障；晚到的非位置偏好操作保留最终位置及最新字段。历史 `late_accepted_position_cannot_overwrite_final_position_after_shutdown_budget` 实际覆盖了 owner/store 路径，不是只比较两个内存值。

这不是取消已接受的 blocking 闭包。原后台 owner 的 5 秒共同预算及 runtime 的 1 秒等待仍保留，同步最终保存可能继续等锁/IO，其他关闭流程还有独立等待。不能把这些数相加后承诺“整个退出最多 6 秒”，也不能声称所有 blocking 工作必定排空或任何错误下都不丢数据。预算来源：[background owner](../loki_metris_gui/src/usage_service/background/mod.rs)。

### 5.4 原生图库分离元数据与资源字节

原生目录只包含 id、filename、format 和计数；原始资源另用 `Arc<[u8]>` 持有，不为原生快照生成 data URL。这移除了该路径的编码工作与编码副本，仍读取原图，也没有证明所有原图、解码帧或 GPU 资源已惰性加载/回收。

参考：[core 图库](../loki_metris_core/src/monitor_gallery.rs)、[GUI 图片快照](../loki_metris_gui/src/monitor/images.rs)、[core 编码合同回归](../loki_metris_core/src/monitor_gallery_native_tests.rs)、[GUI 原生快照回归](../loki_metris_gui/src/monitor/images_tests.rs)。

不能为了降低成本把完整 preview 接口的 image 清空。`preview_from_bytes` 仍真正调用 Base64，保留 data URL 和空/非法字节错误；原生元数据测试用线程局部编码计数验证零编码。单图 32MiB、快照 128MiB 是原始字节预算，不包含解码帧、纹理和分配器驻留，后续内存分析必须分别核算。

### 5.5 增量捷径必须保留质量和已读边界

只比较大小与修改时间不足以证明无需读取：取消后的检查点可能已经观察全文件大小，却只消费了前缀。跳过前还须满足 `parsed_offset + trailing_bytes == observed_size`；这里的 trailing 只能是已读取但尚未闭合的末行。使用检查加法，避免溢出把无效检查点误判为完成。

坏行警告属于当前来源 generation，不能因本轮没有重新读取而清零。只持久化有界、内容无关的计数；重复扫描直接恢复计数，合法追加仅合并新消费完整行的警告，真正替换或完整重建才重新计算。可解除的 fork 归属边界由当前状态动态计算，不永久累计。这样保留 unchanged 与窗口外 Retain 的 IO 优势，也保留覆盖结论。

读取新批次调用时不能套用旧 Complete 缓存。快照在同一 SQLite 事务中读取最新扫描与启用来源事实，宿主在取得实际 writer 后先失效覆盖资格，完整收尾才恢复。索引四态仍服务展示，精确读取资格单独表达；避免为了通知而清空可见历史统计。

参考：[来源质量持久化](../loki_metris_gui/src/backend/local_index/jsonl/mod.rs)、[续读边界](../loki_metris_gui/src/backend/local_index/rollout_ingest.rs)、[一致快照](../loki_metris_core/src/local_index/snapshot.rs)、[连续回归](../loki_metris_gui/src/backend/local_index/tests/quality_checkpoint_tests.rs)、[写入期间今日摘要](../loki_metris_gui/src/usage_service/source_tests.rs)。这些回归证明正确性边界；没有测量吞吐、CPU 或内存收益，不能据此宣称性能提升。

### 5.6 原生窗口反馈只驱动变更，动画终点与防抖分别处理

主窗的 NSWindow 通知只更新 revision、单调时间和按值复制的 zoom 目标，不进入 GPUI 或借用壳层。复用已有原生调度：无新 revision 不进入窗口更新栈；无待处理 zoom 目标不读取原生 frame；只在最终读回后合并一份完整几何偏好。不能为了保存位置在每个渲染帧枚举屏幕或写文件。

静默 250ms 只负责合并，不能独自证明动画完成。实机曾把普通宽度 961 保存成缩放中间值 1383；修正后以系统 delegate 提供的目标矩形与实际外框比较，容差为半个当前 backing 像素，未知目标保持保护。关闭/退出可跳过合并等待，但不能跳过缩放、全屏或 live-resize 的保护。最大化时系统迁屏只映射最后可信普通锚点，不能拿最大化外框代替普通尺寸。

原生变更可能同步回调 GPUI，因此恢复 task 先检查活窗及 generation，再释放 App/Window 借用后调用 AppKit。等待期间携带入口捕获的指针屏，不重新追随鼠标；拓扑变化只刷新可用屏幕。观察者最后引用释放时注销，任务隐藏/退出时失效，避免把多窗口恢复变成长期轮询或悬挂 owner。

参考：[纯放置规则](../loki_metris_core/src/window_placement/mod.rs)、[原生信号与终点](../loki_metris_gui/src/native/windowing/macos_host.rs)、[几何回归](../loki_metris_gui/src/native/windowing/geometry_settle.rs)、[恢复事务](../loki_metris_gui/src/app/window_state.rs)。本轮证明了几何正确性与调用边界，没有测量 CPU、内存或跨框架收益；不能据此认定用户报告的性能问题已解决。

### 5.7 辅助窗按自身样式定位，控件同步按确认边界处理

固定无边框配置窗在本机 AppKit 也会返回 `isZoomed=true`。直接复用主窗“必须非 zoom 才设置普通外框”的保护，会让窗口显示却没有完成定位。保留全屏/过渡保护，辅助窗独立应用外框，再读取真实显示器、位置和内容尺寸，不能凭可见窗口或 API 返回判定成功。

本轮内置屏曾读回0.5 DIP偏移，等待八个事件回合仍未消除。最终在唯一的原生全局 AppKit 点边界对齐原点，并把对齐后的目标反算回屏内 DIP，继续使用半个 backing 像素的严格容差。这个对齐是项目依据实测选择的策略，不宣称所有 AppKit 窗口都要求整数点。读回等待有界，仅在未匹配时执行；时间届满仍不匹配就记录失败，不扩大容差或改写成功条件。每次等待及显示前复核精确窗口和 generation，原生对象不可用时撤销隐藏窗，保留入口重试。

控件也存在重复工作与时序问题：指针先更新 slider，而偏好观察者尚未派发时，即时绘制会用旧偏好把输入覆盖。滑块现在只在 owner 的处理完成边界同步，失败回执同样恢复已确认值；渲染不每帧回写控件。键盘与辅助赋值在入口夹紧，同值不发控件通知，避免越界数值滞留和无变化偏好请求。

参考：[配置窗事务](../loki_metris_gui/src/pet/settings_window/macos.rs)、[单一原生坐标转换](../loki_metris_gui/src/native/windowing/macos_host.rs)、[控件确认边界](../loki_metris_gui/src/pet/settings.rs)、[真实指针与回滚回归](../loki_metris_gui/src/pet/settings_window/tests.rs)。开发预览和失败轮次保留于忽略的 `release/pet-settings-window-development-20261010/`。这些检查支持坐标、生命周期及输入正确性；没有 CPU、内存或跨框架收益测量。

### 5.8 动画只重算变化参数，固定几何精确保留

阴影每帧随 CSS 参数改变，但圆角挖空及九切片只依赖物理尺寸、缩放和半径。原实现每次重绘都对整张紧凑纹理重新计算 `rounded_coverage`。现在每个 Shadow owner 仅保存一个几何计划，按行把相邻、`f64::to_bits()` 完全相同的覆盖率压成连段；颜色和动画样本变化只重算 Gaussian 轴并重着色。宽高、缩放或半径改变才替换计划，隐藏、离屏、无阴影材质和 owner 释放沿原 `clear/retire` 路径清空；不建立全局历史缓存。

不能把“缓存”直接做成完整常驻 `f64` 遮罩。长卡片 2× 下，该密集表约需 5MiB，而本实现实际 Vec 容量合计为 73,920 字节，包含行偏移、连段和切片预留。每个内像素仍写原直通 BGRA，包括 alpha 为零的像素；外围保护圈保持全零，alpha 乘法仍按原左结合顺序计算。以近似值合并、量化成 f32、跳过透明 RGB 或改变乘加顺序，都可能改变最终字节。

正确性依据是冻结的旧栅格算法、旧切片及旧圆角函数；CDF 本轮未改，双方复用。回归比较完整 BGRA、切片四字段的 f64 位值，另逐点展开连段核对覆盖率位值，避免最终 alpha 舍入恰好一致掩盖量化。真实测试窗口还验证静止图片 ID 复用、80/150/299ms 主题样本沿用同一几何、原点平移不重建，尺寸/半径/比例变化失效，筛选条替换和隐藏立即释放计划。`on_next_frame` 回收须显式投递后帧，单纯再次调用测试 `render_frame` 不能代替事件投递。

同一属性组的缓动函数只采样一次，再用于各通道；各属性组仍保留自己的时钟、反向因子和 160/300ms 时长。缓动的 f32 结果可能在 299ms 已精确到终值，300ms 仅把 running 变为 false，此时复用端点纹理属于正确行为，不能强制每个时间点都换图片 ID。

局部标准库基准从本轮源码提取未改动的 CPU 公式，以 `rustc -O` 编译，预热 CDF 和几何，在相同 24 个动态样本上交替执行旧算法与缓存重着色，各 8 轮，计时外先比较像素，输出用 `black_box` 保留并及时释放。长卡片 2× 的每帧中位数为 3.044ms → 0.885ms；另三组为 2.526ms → 0.737ms、0.738ms → 0.214ms、0.758ms → 0.229ms。首次几何准备另计，四组平均为 0.484–1.941ms，不能省略成零成本。

这些数字只证明该 CPU 栅格路径在本轮输入下减少重复工作，未测 GPU 上传、整体帧率或公平的 release/Tauri 对照。源码、提取变换、原始各轮数据与本轮开发检查分开保存在忽略的 `release/theme-geometry-development-20261010/`；此处属于内部优化 `LMDESK-THEME-GEOMETRY-001`，`required_version: 0.2.1`，不提升版本，不把用户报告的主题卡顿、946MB 内存或整机 CPU 差异标为已解决。

参考：[几何计划](../loki_metris_gui/src/app/surface/raster/shadow_geometry.rs)、[旧算法像素回归](../loki_metris_gui/src/app/surface/raster/shadow_geometry_tests.rs)、[真实 owner 回归](../loki_metris_gui/src/app/surface/shadow_owner_tests.rs)、[属性时间线](../loki_metris_gui/src/app/surface/motion.rs)。

### 5.9 描边几何保存采样程序，不能把累加改成乘法

带粗顶边的 accent 卡片原来每次颜色变化都重复执行 8×8 圆角、椭圆 padding-box 和内高光分类。现在每个 Rim owner 只保留当前几何计划：每个 texel 的非空贡献按原 `sy→sx` 顺序保存为类别和重复次数，连续同类贡献可以压缩，重着色仍逐次执行原 `f64 +=`。类别独立于颜色与 alpha；完全透明首绘不能删掉以后可能可见的类别，零顶边也仍可能有内高光。

这不是可以随意代数化的计数表。有效 f32 颜色的固定反例中，同一 texel 的贡献为 `Top×31, Main×1, Top×6, Main×1, Top×6, Main×2`，原始加法得到蓝通道 127；即便保持连段顺序，仅把每段改成 `sample * repeats` 也会得到 128。回归同时保留错误乘法的 mutant 断言，防止未来优化再次引入这个舍入差异。累加 alpha 大于零、最终 8bit alpha 舍入为零时，原直通 RGB 仍须写入；纯绿内高光案例的精确结果为 `[0,255,0,0]`。

几何键含实际物理宽高及 scale/radius/top 的原始 f32 位值，切片目标轴也属于当前计划。颜色变化和平移复用，物理尺寸或任一原始位值变化重建，包括正负零；回归用物理宽高和 cap 保持不变的相邻比例值，避免尺寸变化恰好掩盖漏键。半径仍先按实际卡片短边收敛，再算 cap 和切片，不能改用压缩后的短边。上/下角不能镜像，因为粗顶边和向上偏移的内高光不对称。

测试冻结原 renderer、颜色预乘、内外圆角函数和切片，完整比较 BGRA 与四个切片字段的位值；另逐 texel 展开连段核对原序，覆盖 64 次整格贡献。真实材质 owner 验证明暗主题关联动画、几何失效、accent 移除、隐藏和卸载。零逻辑尺寸曾在材质入口直接返回，旧图可能继续驻留；像素对齐后才变为零的区域也须统一清空当前和退休图像。本轮在两处空区域入口执行同一 `clear`，用真实图集验证，无须等待后帧。该同步释放成立于 owner 本帧尚未绘制材质的空区域，不能推广到本帧已提交有效材质后随意删除图集。

复用计划增加少量常驻内存：长卡片 2× 的偏移、连段及切片 Vec 实际容量合计 63,504 字节，首次准备平均 2.696ms；隐藏、空区域、accent 移除和 owner 释放都丢弃唯一计划。没有每 texel 独立 Vec、完整 64 样本数组或主题历史字典。相同 cap 下继续拉长卡片不增加计划容量，连续重着色不增长预留。

同样的标准库 CPU 基准用 `rustc -O` 编译，24 个动态颜色样本、8 轮交替运行，计时外先核对全部像素。四组旧算法/缓存重着色每帧中位数分别为 2.655ms/0.046ms、2.657ms/0.046ms、0.416ms/0.010ms、0.710ms/0.012ms；首次准备另计为 0.428–2.696ms。测量开始时 GPUI 和参考应用都已停止，但其它宿主负载没有完全控制。这里的约 40–59 倍仅指复用几何后的 CPU 描边重着色路径，不含 GPU 上传、初次布局或整个界面帧循环，不能乘到应用 CPU 或帧率上。

原始数据、源码提取变换和开发检查保存在忽略的 `release/rim-geometry-development-20261010/`。此处为内部优化 `LMDESK-RIM-GEOMETRY-001`，`required_version: 0.2.1`，版本未提升；尚未关闭整体卡顿、跨框架 CPU 差异或长期内存问题。

参考：[描边计划与保序重着色](../loki_metris_gui/src/app/surface/raster/rim.rs)、[独立旧算法与舍入反例](../loki_metris_gui/src/app/surface/raster/rim_geometry_tests.rs)、[真实 owner 及空区域释放](../loki_metris_gui/src/app/surface/rim_owner_tests.rs)、[材质绘制入口](../loki_metris_gui/src/app/surface/state.rs)。

### 5.10 缓存纯颜色端点，继续执行每帧动画合同

固定几何复用后，`MaterialMotion` 每帧仍会按同样输入重新生成五组端点，包括五次 RGB→Hsla→Rgba 往返、预乘和阴影参数。当前实现只缓存当前 `dark/hovered/top_accent` 对应的固定大小端点值，无额外堆分配或历史主题集合；输入变化重算，连续动画采样和静止重绘复用。首次生成成本仍然存在，缓存也增加每个 owner 的固定内存，不能将其写成无成本优化。

缓存范围仅限纯端点生成。每帧仍依原顺序执行 border、top、surface、highlight、shadow 的 setter 与 sample，`transition` 继续作为本帧参数，不能因输入键没变而跳过。即时 `jump_to` 会刷新时间线；`Hsla::PartialEq` 使用 total_cmp，能区分带符号零且相同 NaN payload 自等，但目标数组使用普通浮点比较，NaN 每帧仍可能重定向。甚至所有输入分量有限的极大 hue，也可能在转换中溢出产生 NaN；仅检查输入有限不能证明可以省 setter。

回归冻结原 owner 和颜色 helper，逐项比较四组 Hsla 的字段位值、阴影参数/BGRA 及 running。覆盖明暗、hover、快速反向、过渡开关、hover 期间更换 accent、各字段带符号零、异常颜色和恢复正常颜色。实际生成计数另证明同输入帧不重算；同输入在动画中切换即时模式仍正确落到端点。相关 surface 回归最终 61 项通过。这个结果证明计算复用和行为保持，没有整机 CPU 或帧时间收益测量，不给出速度倍数。变化 `LMDESK-MATERIAL-ENDPOINTS-001` 为 maintenance，版本仍 0.2.1。

### 5.11 共享解码不等于共享窗口纹理寿命

同一素材由图库和桌宠共享时，全局仍有使用者不代表每个窗口都应保留纹理。原登记器仅在全局最后一个位置退出时清理，导致隐藏主窗的动态图集随桌宠一起常驻。现在分别判断“本窗最后位置退出”和“全局最后位置退出”：前者只删除该窗中这张图的全部帧并刷新场景；后者保留原 CPU Asset 与所有窗口纹理清理路径。同窗尚有其他位置时不删除，其它窗的纹理及共享解码结果保持可用。

局部清理延后到当前 App 更新的收尾，避免在本轮已提交有效图元时立即破坏图集；这只是 GPUI 的更新队列顺序，不是 GPU fence。任务只携带弱解码引用、精确 WindowId 和实际 ImageId，不延长 CPU 像素寿命。收尾时复核同窗是否重新取得同一实际图片；同逻辑 ID 的新一代不能阻止旧图清理。窗口已关闭时不改清理范围，绝不能用全局删除兜底。删除纹理后显式刷新，防止恢复时继续重放旧 AtlasTile；这是实现安全约束，本轮未用删除该调用的变体独立验证旧 Scene 缓存保护。

回归分别检查实际 Asset 结果、强弱引用和 Metal 图集。多帧检查逐帧查询真实 tile，缺项探针不分配新纹理；Metal 的默认 `contains` 不能充当驻留 oracle。覆盖双窗共享、同窗双位置、同回合卸载再挂载、局部清理后全局退出、窗口关闭及实体释放。恢复使用普通 draw，另推进实际时间并核对绘制像素；恢复回归不额外强制刷新，也不只检查逻辑登记来代替正常恢复。新一代身份测试是直接构造的收尾交错，不宣称覆盖完整运行时重解码竞态。

本轮还用标准库生成合法 512×512、120 帧 APNG，隔离双方持久数据，通过生产 SessionStart 让桌宠真正取得图片，再比较主窗可见、隐藏、恢复和再次隐藏。空桌宠不能算共享场景；原始文件只有约 264KB，解码像素和逐帧纹理却大得多，文件预算不能约束这两类成本。图形内存按窗口生命周期回落支持本次修复，但可见状态仍保留多帧纹理，后续需单独设计有界工作集。

38 项图片回归及 12 项图库生命周期回归分别通过，筛选有重叠，不能相加当作唯一测试数。开发预览、失败轮次、原始 footprint、历史 0.3.12 比对和数据还原记录保存在忽略的 `release/image-window-residency-development-20261010/`。两代预览均为 dirty 开发输入，几何、进程寿命和后台负载没有完全统一；不能用跨进程绝对值计算公平收益，也没有 CPU、长期泄漏或 Windows 结论。变化 `LMDESK-IMAGE-WINDOW-RESIDENCY-001` 按 bug-fix 登记为 0.2.2。

参考：[窗口与全局退役](../loki_metris_gui/src/monitor/image_residency.rs)、[生命周期交错回归](../loki_metris_gui/src/monitor/image_window_residency_tests.rs)、[真实 Metal 多帧回归](../loki_metris_gui/src/monitor/image_window_metal_tests.rs)。

### 5.12 图集区域必须等 GPU 完成后才能复用

删除逻辑 key 和回收物理区域是两件事。Metal command buffer 保留 texture 对象，仍不能阻止 CPU 的 `replace_region` 向过早归还的 allocator 区域上传新图片。GPUI 的 defer、下一次界面更新或固定等待若干帧，都不是 GPU 完成证明。

本次修复在首次纹理查询前登记编码 epoch。逻辑删除立即生效，但完整 AtlasTile、allocator 区域、物理引用计数及 texture slot 保留到当时已有的编码全部完成或取消。活动 epoch 使用有序集合；不能用最大的已完成编号跳过较早的未提交编码。只保存活动编码和退休资源，不累积已完成历史；后来开始的编码不会延长旧资源的等待。

私有 EncodedFrame 包装提交入口，只转发实际需要的 Metal 操作。正常提交把图集 guard 转交完成回调；编码错误或未提交返回值被丢弃时自动取消。完成回调也处理 GPU 错误终态，只操作图集锁，不进入 Window/App。图集 guard 的 Arc 允许窗口退出后独立收尾；图集不持 command buffer，避免引用环。不能持锁提交或等待 GPU。

真实回归用 SharedEvent 阻塞提交队列，以可观察的物理纹理槽证明旧实现提前回收，再核对完成后的释放。另用同页 keeper、700×700 红色区域和同逻辑 key 的蓝色重插，检查尚未提交时也不能复用旧区域，实际像素仍为红色且新分配仍存在。还覆盖丢弃未提交编码与先关闭 renderer 的生命周期。状态测试检查乱序完成、不同退休批次、后续编码和一万次无历史积累。最终 Apple 渲染 27 项、图片 38 项开发回归通过；10 项退休相关回归包含在前者内，不能相加。

此 fence 只保护已经开始编码的读取，不能让未来 Scene 安全重放已逻辑删除的旧 tile；应用仍需保留实际绘制帧的需求并在卸载清理时刷新缓存场景。GPU backlog 也可能保留多个退休帧，本次没有建立严格显存硬上限。可见动画的逐帧工作集、CPU 收益、长期泄漏和 Windows 时序尚需各自验证。变化 LMDESK-METAL-ATLAS-RETIREMENT-001 按 bug-fix 登记为 0.2.3，失败日志及开发预览指纹保存于忽略的 `release/metal-atlas-retirement-development-20261010/`。

参考：[退休状态](../vendor/gpui-pre-apple/src/atlas_retirement.rs)、[真实 Metal 回归](../vendor/gpui-pre-apple/src/atlas_retirement_metal_tests.rs)、[提交包装补丁](../vendor/gpui-pre-apple/patches/atlas-retirement-submit.after)、[图集回收补丁](../vendor/gpui-pre-apple/patches/atlas-retirement-physical.after)。

### 5.13 动画纹理按真实绘制位置保留工作集

同一动画的 CPU 解码结果仍可共享，但每窗无需固定所有已播放帧。macOS 现在为每个 Playback owner 记录最后成功 paint 的实际 ImageId/frame_index，同窗取并集；下一次成功绘制替换旧 claim，失败绘制不提前撤销旧帧，缓存 Scene 没有回调时继续保留其原帧。时间线、预绘制选择和最新 CPU Weak 都不能替代真实 paint 的身份。

收尾队列每窗合并一次，执行前重新检查并集，处理 A→B→A、多个位置同帧和错相播放。队列及 claim 只保存身份值，不强持像素或 Entity。正常换帧只清旧 key，不强制刷新无关缓存；位置卸载和孤立 paint 必须刷新场景，即使其它位置仍持同帧而无需删 key。CPU 登记器在 Catalog/Overlay 仍存活时也撤销退出 Playback 的 GPU claim；旧整图退役检查实际 claim，补充5.11的最新 Weak 判断，不能误删仍在绘制的另一物理图像。

有界的是活跃逻辑帧集合，而非瞬时显存硬上限。物理 AtlasTile、分配器区域和纹理槽继续受5.12的 GPU 完成规则保护；慢 GPU 或在途编码可能暂留多个已退役区域。逐帧重新上传的成本也需要单独测量，不能由内存下降推断 CPU 改善。

回归先以正式12帧 APNG、真实 Metal 证明旧策略暖机累积5帧，随后验证单位置30个边界始终1帧、错相双位置20个边界保持2帧，以及图库持CPU时卸载播放器释放全部GPU帧。有限动画自然结束后，把左侧播放器放入 GPUI cached view，右侧同源动画继续换帧；左侧真实 prepaint/paint 计数不增长、末帧 tile 不变。两个位置最终同帧时撤销右侧，仍须刷新缓存场景并保留左侧纹理，最后读回白色末帧像素。普通 draw 与强制 refresh 的 render_frame 必须区分，否则无法验证缓存重放。

测试查询也要验证后端合同：Metal 未实现的 contains 默认返回false，新增单帧 query 使用 get_or_insert_with(key, || Ok(None))，真实 Metal 测试同时检查存在帧和缺失帧。旧 App 隔离测试要求动画全部帧常驻，已改为当前真实帧检查，并停止该隔离测试自身推进动画，以免合法 tile 重建污染跨 App 身份断言。最终48项相关图片回归通过；编译失败、旧 oracle 失败和各轮 dirty 预览分别保留于忽略的 `release/image-frame-working-set-development-20261010/`，不是正式候选验收。

同一合法512×512、120帧、100ms APNG在图库与真实SessionStart驱动的桌宠双窗播放，配置与素材逐文件SHA一致，两代图库截图均2984×1858像素。对照0.2.3的图形dirty为1,031,159,808B，修复0.2.4两次可见采样均29,343,744B；隐藏主窗后修复版22,675,456B，恢复24,035,328B，再隐藏18,448,384B，桌宠截图与恢复图库均显示持续换色。空图库及未成功取得桌宠图片的早期样本已排除。该结果支持工作集减少纹理驻留；dirty开发输入、进程寿命、GPU完成点及后台用量索引负载仍不完全匹配，不计算公平加速比例，也不据此宣称CPU改善或长期无泄漏。原始支持目录46项、29文件在实验后按字节/权限/链接还原。

变化 LMDESK-IMAGE-FRAME-WORKING-SET-001 按 bug-fix 登记为0.2.4。此策略仅在macOS播放器启用，Windows、优化发布构建、长期泄漏与公平Tauri比较仍需独立验证。

参考：[帧工作集](../loki_metris_gui/src/monitor/image_frame_residency.rs)、[真实 Metal 与缓存重放回归](../loki_metris_gui/src/monitor/image_window_metal_tests.rs)、[单帧清理与查询补丁](../vendor/gpui-pre/patches/image-frame-drop.after)。

### 5.14 稳定标题避免重复请求原生平台

Shell 原先每次 render 都格式化版本和标题并调用 Window.set_window_title；GPUI core、macOS 与 Windows 的 setter 均没有相等值守卫。macOS 路径会构造 NSString、更新窗口菜单并请求 NSWindow.setTitle，Windows 路径会构造 HSTRING 并请求 SetWindowTextW。不能仅凭调用次数推断系统实际重绘次数。

当前完整标题只有有效语言会在进程内变化，版本来自编译期，支持信息为固定文案。每个 Shell 保存最后请求的有效语言，首次仍设置完整标题；成功保存语言后下一次绘制更新，稳定语言的重复绘制跳过格式化和原生请求。缓存不跨窗口，不通过宿主 getter 做逐帧比较。GPUI setter 没有成功回执，字段表示已请求；去重也取消了旧实现逐帧隐式重试失败 setter 的行为。未来标题增加动态字段时必须扩大缓存键。

回归通过真实 TestWindow 写入哨兵标题再强制根视图绘制：旧实现覆盖哨兵而失败，去重后保留；新建 Shell 仍设置完整标题。另验证语言 pending 时保持旧标题、真实内存 Writer 成功回执后更新中文完整标题、稳定中文继续去重，以及实际 Writer 拒绝提交时保持原语言和标题。3项标题与3项既有偏好回归通过。哨兵证明未再次请求覆盖，不是精确宿主成功次数统计。

这是 LMDESK-WINDOW-TITLE-REQUEST-001 内部维护，版本保持0.2.4。初步5秒 sample 没有捕获标题 setter 热栈，不能把它认定为用户报告的高CPU根因；没有公平Tauri CPU对照或量化收益。开发输入与实机记录保留于忽略的 `release/window-title-request-development-20261010/`。

参考：[标题请求及真实平台回归](../loki_metris_gui/src/app/window_title.rs)。

### 5.15 优化后重新核对回归的观察口径

资源生命周期优化会改变旧测试的观察对象。动画改为当前帧工作集后，要求所有帧同时常驻的断言已不符合合同；只断言“没有全部常驻”也不能证明释放成功。正向检查应逐帧读取真实图集，确认已绘制帧仍有驻留；当前 macOS 单位置播放器还应确认只有一个帧。隐藏与离开页面后，逐帧检查必须全部不存在，同时保留解码对象弱引用和实际文件服务的生命周期断言。

平台边界也必须与夹具能力相符。GPUI TestWindow 不提供可捕获的 AppKit 原生对象，虚拟 resize 不能证明 macOS V2 坐标保存。它可验证宿主不可用时保留最后可信几何：让预置状态、初始尺寸和 resize 尺寸互不相同，安装生产退出钩子，提交真实异步偏好，实际退出后重新读取文件，确认最新偏好排空且可信几何没有被虚拟反馈覆盖。真实 macOS 尺寸保存仍须通过原生窗口缩放、正常退出和重开补证；非 macOS 的旧几何保存回归继续按该平台运行。

发现旧断言失败时，先定位正式合同、实际观察与失败顺序，再修正测试。不能只放宽数量、增加等待或跳过失败来让打包通过。测试边界与产品故障分别记录，优化包的构建和运行证据保留于忽略的 `release/`。

参考：[逐帧图库回归](../loki_metris_gui/src/app/monitor/catalog_tests.rs)、[真实退出与宿主边界回归](../loki_metris_gui/src/app/mod_test.rs)。

### 5.16 用完整进程集合测 CPU，区分后台、遮挡与静态帧源

跨框架比较必须包含宿主。macOS WebKit helper 可能由 launchd 持有，按 PPID 递归会漏计；使用应用 coalition 获取实际集合，再分别报告主进程、主进程加 WebKit、完整集合。两端都核对 PID、启动时间、UUID 和可执行路径，成员变化、退出或读取失败使该轮无效；不能将失败读数算零。CPU 用每个进程实际单调时间内 user+system 增量计算，集合求和且不除核心数。resident 与 footprint 另报，进程账本相加不等于去重物理内存，也不包含 WindowServer 的间接开销。

本次 macOS 本地优化试包与历史0.3.12设置页都为中文、System、通知关闭，原生填充同尺寸。三轮各预热5秒、测20秒，采样期间无 Computer Use 或本项目编译；前台端点始终是Chrome，两版在后台。GPUI完整集合 CPU 中位数约0.045841%，范围0.044977%～0.123659%；参考完整集合含WebKit为0.002001%，范围0.001883%～0.002031%。每轮 interrupt wakeups 分别为5291/4741/4776与72/66/64。数值很小但唤醒差异值得继续定位；不能据此声称前景体验、公平框架倍率或用户高CPU问题已经解决。遮挡时间线、Pet、索引历史和运行时长尚未完全控制，端点前台相同也不能排除区间内变化。

源码核实的 macOS 帧源按窗口 occlusion visible 订阅 CVDisplayLink；静态失焦但仍有可见像素时继续接收显示器节拍，完全遮挡时停止。每显示器共享 link，每窗口有独立 main-queue source；core 的 dirty guard 跳过绘制，仍会进入帧回调。应用的40/100/200/500ms循环来自重复安排一次性 timer，名义频率不能直接换算系统唤醒。5秒 sample 捕获了帧回调和业务 timer，但不足以分摊此次CPU或唤醒差异。若要停静态帧源，必须先补齐 dirty、next-frame动画、resize、重新可见及可访问性激活的重新唤醒路径，不能只调用 stop。

原始构建、精确二进制、三轮进程身份和采样方法保留在忽略的 `release/cpu-idle-render-development-20261010/`。本轮为开发诊断，仍需前景、动画和长期内存的独立证据。

参考：[帧源订阅](../vendor/gpui-pre-macos/upstream/src/display_link.rs)、[窗口可见性及帧回调](../vendor/gpui-pre-macos/upstream/src/window.rs)、[core 帧调度](../vendor/gpui-pre/upstream/src/window.rs)。

### 5.17 定时读取继续，只有展示状态变化才通知

监控工作台原先每次三秒读取都调用 `cx.notify`。现在每次实际读取仍先更新 `last_read`，完整 `HookRelayStatus` 或读取错误变化时才通知。失败保留最后成功指标；恢复时即使指标相同，也清除读取错误并通知一次。不能只比较桌宠 revision：HTTP拒绝会改变失败计数和错误，监听开关也可能不推进该 revision。完整相等比较包含额外桌宠字段，属于不会漏更新的保守去重。

回归使用隔离目录中的真实宿主、真实TCP拒绝和关闭、真实GPUI Entity观察器。每次读取独立 update 后提交事件，防止同一更新的通知合并掩盖问题；重复成功/失败均检查通知数不增长且 `last_read` 已更新，断开服务保留完整指标，重新挂载相同快照仍清错。恢复无条件通知的错误变体按预期失败，修正后的监控模块103项相关回归通过。开发预览实机检查确认工作台显示和跨页返回正常；历史版本轮后台截图正文为空，只能记录控件树，不能给出像素一致结论。

这是 `LMDESK-MONITOR-PROJECTION-NOTIFY-001` 内部维护，`required_version: 0.2.4`，版本不变。减少的是无变化通知，没有整机CPU收益测量；三秒读取及其它定时器仍存在。原始失败、错误变体、回归和新二进制检查保留于忽略的 `release/monitor-projection-development-20261011/`。

参考：[生产读取与通知](../loki_metris_gui/src/app/monitor/mod.rs)、[真实宿主和通知回归](../loki_metris_gui/src/app/monitor/refresh_tests.rs)。

## 6. 历史验证结果及构建指纹限制

### 6.1 保留失败、修正与不同二进制

以下是 Task 1 核对过的历史日志，不是本文新运行结果。各轮具体二进制、筛选及退出码以 `E/index.json` 和相应收据为准。

| 收据/日志主题 | 实际结果 | 可支持的结论 |
|---|---|---|
| `base-red-results.json`，文件均带 `loki-metris-performance-` 前缀 | 2+1+2 项失败，exit=101；重复 revision、同像素防抖、原生目录编码等行为断言命中 | 5 项真实行为 RED；fresh-cache 先败于旧 revision，不能独立证明 cache mutant |
| `base-green-results.json` | 同筛选 2、1、2 项通过，队列另 6 项通过 | 对应修正和队列回归；不是全仓测试 |
| `shutdown-red-results.json` | 晚到 B 覆盖最终 A 的 1 项失败，约 6.07 秒；另 2 项通过 | 不能称“三项关闭 RED” |
| 两份 `cache-*-red.json` | no-update、changed-only 各 1 项失败，命中正式缓存断言 | 负向变体有效；后续 validation 修正匹配文本/行号，没有新跑测试 |
| `final-green-results.json` | Agent/退出 10、GUI 图库 24、管理页 101 项通过；Pet 42 通过、1 失败 | 该轮不是全绿，列出 870 个测试不等于全部执行通过 |
| `fixture-green-results.json` | 将默认 `Locked(false)` no-op 夹具改为真实 `Locked(true)` 变化后，Pet 43 项通过 | 修正后使用新二进制，不能与上轮拼成同一二进制全绿 |
| `core-green-results.json` | core 图库 6 项通过 | metadata 零编码、完整 preview 真编码及错误合同 |
| `production-check.jsonl` 与 stability 收据 | 末条 `build-finished.success=true`；1313 个输入无 drift/added，HEAD 未变 | 当时整合源码的 check 记录；不等于纯净 C checkout 或跨平台验收 |

上表短文件名都补 `loki-metris-performance-` 前缀。失败 Pet 测试为 `first_image_partial_snapshot_failure_retries_without_new_catalog_revision`，夹具修正在 [first_populated_tests.rs](../loki_metris_gui/src/pet/first_populated_tests.rs)。共享 Cargo 产物初次列测试曾遇 SIGKILL，日志匹配器也曾误判；后续固定二进制到新 inode 并记录 SHA。编译/工具/夹具失败必须与行为 RED 分开，parse、hash 守卫和未编译准备包也不能替代行为测试。

关键历史 GUI 测试二进制 SHA-256：

| 轮次 | SHA-256 |
|---|---|
| base-green | `258121c432d5c2a634f0d101d1384c441fc67573c0979b50de169efa979dba3a` |
| final-green（含 Pet 失败） | `c9ba969d7107e71d231b62d62431d740c918f23d5e1fd9aeb8c8990366eb773c` |
| fixture-green（Pet 43 通过） | `5ff7d790c6a0020780fddc028256696d614e7b46f4a6e437c37fb26da6a5a33e` |

### 6.2 源码提交与构建输入并不完全相同

历史编译前记录的 HEAD 为 `P`，但文件已经包含优化。Task 1 对 `C` 改动的 16 个 Rust 文件逐个比较，11 个与编译前摘要一致，5 个与保存的清理快照一致而不同于 `C`：`monitor/images.rs`、`monitor/settings.rs`、`pet/mod.rs`、`pet/position/mod.rs`、`usage_service/mod.rs`，均位于 `loki_metris_gui/src/`。

差异涉及仅测试入口/导入清理，以及 UsageData 字段、RootRecord 导入和 cancel/finish 入口；最终位置屏障流程仍在。具体逐文件摘要和差异由 Task 1 的 `evidence-audit.json` 保存。故历史构建/测试记录只能按其记录的输入引用，不能只写一个提交哈希就宣称“纯净 C 已通过全部检查”。本文也不验收其他会话的未提交清理。

## 7. 历史运行观察：数值只能附条件引用

以下均为 macOS `aarch64-apple-darwin` 开发预览，`development-only`、`releaseCandidate=false`，运行收据标为 debug；已有 check profile 为 `opt_level=0`、debuginfo=2、debug assertions 开启。没有优化发布构建的长期数据。

| 产物 | sourceCommit / sourceDirty | bundle 可执行 SHA-256 |
|---|---|---|
| Preview16 | `P` / false | `441b4bf903ea03cae06e6c994b0c2f75ba96a73c9c3ed2a1751c0088d3d2d8bf` |
| Preview17 | `C` / true | `2fbd3f71cd662d9d0fb3485a2496c13210510397fac96fb2600aaf47c6e6f51b` |

Preview17 收据的 `sourceSnapshotSha256` 为 `ac3cafe589165df1e524020c50e1c3c68efae4e25e491237084773b2c70e68b2`；它是原字段值，不能自行解释为已证明覆盖全部源码的独立 manifest。下表 CPU 均为排除首样本后的算术均值，footprint 是独立采样点，时间为 Asia/Shanghai。

| 观察组/开始时间 | 页面、时长/有效 CPU 样本 | CPU 均值；范围 | physical footprint | 负载及可比性限制 |
|---|---|---|---|---|
| O1 Preview16；2026-10-09 23:41:59 | Settings；约30秒/15 | 15.4333%；0.3–19.2% | 6 点均约858.35MiB；生命周期 peak 889.24MiB | 页面经前后 CU 确认；5 工具、中文/System、通知/自启关；同期另一会话 Cargo 编译，Pet 未严格匹配 |
| O2 Preview17；2026-10-10 00:02:45 | Settings；约50秒/25 | 4.5240%；3.7–5.2% | 10 点约514.50→451.14→427.28MiB；peak 669.19MiB | 页面经前后 CU 确认；仅采样后 Cargo 列表为空，没有全窗口无编译的连续证明 |
| O3 Preview17；2026-10-10 00:11:29 | Gallery；30秒/15 | 58.6133%；29.3–171.8% | 6 点约221.74→232.96MiB | 实际 GIF 播放；top 后段 MEM 曾显示334M，与 footprint 不是同一时点/口径；采样前后无 Cargo |
| O4 同一 Preview17；2026-10-10 00:12:27 | 回 Settings；50秒/25 | 4.0400%；3.5–5.0% | 11 点约318.52→253.19MiB | 页面前后 AX 确认；同 PID/二进制，采样前后无 Cargo；短时回落不排除长期泄漏 |

O1 对应 `E/loki-metris-performance-preview16-settings-before-repeat-{top.txt,footprint.json}`。O2–O4 分别对应 `E/loki-metris-performance-preview17-{settings-after,gallery-playback,after-gallery-settings}-{results.json,top.txt,footprint.json}`；O3/O4 各有 `-measurement.json` 保存命令与前后 PID/SHA。这些大括号表示文件族，不是一个实际文件名。

O1–O4 的窗口、预热、进程寿命、素材/Pet 状态和并发负载不同；只可引用各自窗口的观察值，不能算公平加速比例或单项收益贡献。原生图库 4 素材、参考图库 6 素材，未匹配动画负载，参考 WebKit helper 的完整归属也未验证，不能据此宣称优于 Tauri。

同一 Preview17 的更晚 vmmap/stack 曾显示约353.8M，不能用这个较低值替换 O2 末点427.28MiB。Preview15 早期标为 static-settings 的区间后来确认在 Dashboard，切换时刻/原因未知，已排除出同页比较；其历史峰值、sample 与更晚 vmmap 只保留为独立诊断。相关旧记录为 `release/gpui-parity/performance-preview15-initial.json`、`performance-preview15-dashboard-vmmap02.json`。

## 8. 常见误判与后续验证方向

| 容易误写的结论 | 应采用的判断 |
|---|---|
| 内存下降，所以泄漏修好了 | 仅报告指定窗口回落；需要长期重复负载与资源生命周期证据 |
| sample 没出现某符号，所以该路径消失 | 只说明该次有限采样未观察到；结合事件计数与回归 |
| 加缓存后测试绿了，所以缓存正确 | 用正式读取证明变更/no-op 都取到新值，再用错误变体验证区分力 |
| 防抖就是每次通知重置计时器 | 同物理像素不得延期；必须处理在途 B 与恢复 A |
| 退出取消了任务，所以旧写入不会发生 | 已接受 blocking 可能继续；由终止意图和锁内顺序保证最终状态 |
| 元数据零编码，所以图库全部内存有界 | 原始字节、解码帧、纹理与分配器空间分开核算 |
| 所有测试一轮全绿、commit 就是构建输入 | 保留失败与夹具修正，记录每轮二进制及 dirty 输入 |
| debug 热点代表发布收益或跨框架优势 | 固定发布构建、相同场景与完整进程集合后再比较 |

后续路线仅列补证条件，没有在本 Task 实施、运行或承诺收益：

1. **Hook repair cache**：`C` 不含该优化，现有准备包没有可引用的真实 RED/GREEN 或运行结果。先定义缓存失效与外部配置变化条件，证明错误缓存可被回归捕获，再做同负载对照。
2. **动态图布局/解码/绘制**：现有 Taffy/绘制栈不能区分各段贡献。固定素材、尺寸、帧数、可见数量与构建 profile，补同步调用栈及分阶段频率/耗时，再选择单变量方案。
3. **长期内存及资源回收**：以相同进出页面操作重复多轮，保留预热后恢复基线、当前值和生命周期峰值，定位未释放 owner/引用、活分配或 GPU 资源；不能只延长一次截图观察。
4. **公平性能比较**：统一 debug/release、系统与素材负载，确定所有 helper 归属，重复轮次并报告波动。Windows 与最终候选体验当前为 Unverified，不借用 macOS 预览结果。
5. **后续桌面体验七项问题**：独立诊断或方案不等于优化已实现。本文只补充已实施且分别取证的案例；主题卡顿、通知、鼠标穿透、Token 总量、多屏坐标等其余改动，须按各自任务补实现与行为证据后再更新，不能据局部案例认定七项均已完成。

后续更新本文时，把新案例绑定到精确源码、非空行为结果和运行条件；原始截图、大日志及收据继续保留在忽略的 `release/` 集合。实现事实、性能观察、待验证假设分别陈述，历史失败和未执行范围均保留。
