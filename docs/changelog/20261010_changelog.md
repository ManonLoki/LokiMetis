# 2026-10-10 开发变更

## LMDESK-100M-001 · 今日每100M Token系统通知

分类：feature。required_version：`0.2.0`。同周期首个功能提升已由既有事件应用，本次通过受管版本门禁登记事件，不再次提升Minor。

今日摘要复用启用Agent/根目录的All canonical去重与WorkBuddy合并，捕获同一自然日/时区；仅完整、精确且90秒内的事实推进每100,000,000档位。新增core压缩队列、高水位、首次/来源变化基线、32日保留、跨日过期、有界退避、崩溃Unknown及显式重试。独立私有账本按入队、预约、系统调用、回执、结果保存顺序处理；同值不落盘。

应用级唯一原生owner接入macOS UserNotifications和Windows Toast，设置/托盘/App菜单共享已确认本地开关并分别显示系统权限、积压、基线和Unknown。启动不申请权限；macOS裸binary失败关闭；Windows仅主动授权时登记并读回本产品AUMID。已接受API请求不代表用户已经看到横幅。摘要单worker与页面查询取消隔离，后台文件事务不进入渲染或40ms事件轮询；同值观测更新新鲜度但不增加UI revision。

相关开发回归30项通过：自然日/DST与资格2项、里程碑状态11项、GUI通知/私有文件/真实SQLite与WorkBuddy时区13项、页面查询4项。macOS `aarch64-apple-darwin` 实际GUI目标编译检查通过；真实Windows适配器独立 `x86_64-pc-windows-msvc` 类型及Send/Sync检查通过。完整Windows GUI交叉编译在当前macOS宿主因缺少Windows C头文件（ring找不到assert.h）失败，不能作为Windows完整集成通过。

未运行Computer Use、真实系统权限/横幅/点击验收或应用打包安装；未重启preview17，未合并默认分支、推送、创建tag或发布。正式系统验收和其它桌面体验修复仍由独立Task承担。开发日志与交付证据位于忽略的 `release/task5-token-notification-20261010/`，不作为发布候选。
