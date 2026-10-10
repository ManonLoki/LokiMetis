# 2026-10-10 开发变更

## LMDESK-SOURCE-QUALITY-001 · 增量检查点持续保留质量与读取资格

分类：bug-fix。required_version：`0.2.1`。通过受管版本门禁提升 Patch，未发布。

Codex 已消费完整坏行的内容无关警告随来源 generation 保存；重复扫描、重启、合法追加和窗口外保留均继续报告 Partial。fork 待自身起点的状态仍动态计算，找到合法自身起点后可解除。Codex parser 升为 v9，私有状态 schema 升为 v2；缺失质量或旧 parser 检查点必须重新解析，同根新版来源不能掩盖尚未重建的旧来源。

取消增量读取后，即使文件大小和修改时间未变化，也必须继续消费未读后缀。只读快照在同一 SQLite 事务内确认最新扫描、全部启用根与检查点完整性；扫描中、失败、取消或有警告时，旧 Complete 缓存不能赋予今日通知资格。普通扫描和单根重建在实际写入前失效宿主覆盖缓存，成功收尾后才恢复。

相关开发回归通过：GUI 索引 203 项、来源服务 11 项、查询 4 项、通知筛选 25 项，core 索引 38 项、联合视图 10 项；测试组有重叠，不累加为独立用例总数。新增真实 SQLite 连续回归覆盖取消后重启续读、90M→110M 批次写入期间的通知抑制、最新失败/取消/警告、启用根 Partial 及后续成功恢复。开发证据位于忽略的 `release/source-quality-regression-20261010/`。

新源码的 macOS 开发预览已完成 Computer Use 比对：90M 完整基线追加至 110M 并包含完整坏行后，连续五次未变扫描及普通重启均保留一条警告和 Partial；系统允许且本地通知开启时，通知账本保持不变。仅修复同根坏行后恢复完整覆盖，通知 API 接受一次，下一轮未重复。原有宿主文件和系统权限已恢复，未保存的桌宠草稿已通过界面恢复。此项为开发回归，API 接受不代表横幅可见；系统横幅/点击、Windows 宿主与正式候选验收仍待完成。

## LMDESK-100M-001 · 今日每100M Token系统通知

分类：feature。required_version：`0.2.0`。同周期首个功能提升已由既有事件应用，本次通过受管版本门禁登记事件，不再次提升Minor。

今日摘要复用启用Agent/根目录的All canonical去重与WorkBuddy合并，捕获同一自然日/时区；仅完整、精确且90秒内的事实推进每100,000,000档位。新增core压缩队列、高水位、首次/来源变化基线、32日保留、跨日过期、有界退避、崩溃Unknown及显式重试。独立私有账本按入队、预约、系统调用、回执、结果保存顺序处理；同值不落盘。

应用级唯一原生owner接入macOS UserNotifications和Windows Toast，设置/托盘/App菜单共享已确认本地开关并分别显示系统权限、积压、基线和Unknown。启动不申请权限；macOS裸binary失败关闭；Windows仅主动授权时登记并读回本产品AUMID。已接受API请求不代表用户已经看到横幅。摘要单worker与页面查询取消隔离，后台文件事务不进入渲染或40ms事件轮询；同值观测更新新鲜度但不增加UI revision。

相关开发回归30项通过：自然日/DST与资格2项、里程碑状态11项、GUI通知/私有文件/真实SQLite与WorkBuddy时区13项、页面查询4项。macOS `aarch64-apple-darwin` 实际GUI目标编译检查通过；真实Windows适配器独立 `x86_64-pc-windows-msvc` 类型及Send/Sync检查通过。完整Windows GUI交叉编译在当前macOS宿主因缺少Windows C头文件（ring找不到assert.h）失败，不能作为Windows完整集成通过。

未运行Computer Use、真实系统权限/横幅/点击验收或应用打包安装；未重启preview17，未合并默认分支、推送、创建tag或发布。正式系统验收和其它桌面体验修复仍由独立Task承担。开发日志与交付证据位于忽略的 `release/task5-token-notification-20261010/`，不作为发布候选。
