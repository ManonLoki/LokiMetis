# 性能优化参考

先确认运行产物和场景，再用调用栈、事件频率及行为回归定位重复工作。本项目已核实的经验是：无变化写入会放大后台刷新，防抖需要区分最后意图与在途任务，退出需要保护最终位置，原生图库不必携带 Base64 展示副本。这些经验支持后续排查，不构成长期内存、跨框架性能或完整产品体验的验收结论。

整理日期：2026-10-10。本文依据 Task 1 已核对的保存证据与已提交源码编写；文档编写阶段没有重新编译、运行产品测试、启动应用或采样。后续代码变化时，应重新核对相关不变量和证据，不能把本文数值当作当前版本的性能承诺。

## 1. 事实基线与证据入口

本文源码基线 `C` 为 `1fc34da10935802c58a6a29072a57bac458a5de0`，其父提交 `P` 为 `aa5ec10e6bc165231b863dd37ffa3bdb46bb8796`。相对源码链接用于导航，符号含义以 `C` 中的对象为准；工作树日后改变时，可用 `git show 1fc34da10935802c58a6a29072a57bac458a5de0:仓库相对路径` 定位历史实现。

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
5. **后续桌面体验七项问题**：独立诊断或后续方案不等于优化已实现。本参考不合并这些结果；涉及主题卡顿、通知、鼠标穿透、Token 总量、多屏坐标等后续改动，须按各自任务补实现与行为证据后再更新案例。

后续更新本文时，把新案例绑定到精确源码、非空行为结果和运行条件；原始截图、大日志及收据继续保留在忽略的 `release/` 集合。实现事实、性能观察、待验证假设分别陈述，历史失败和未执行范围均保留。
