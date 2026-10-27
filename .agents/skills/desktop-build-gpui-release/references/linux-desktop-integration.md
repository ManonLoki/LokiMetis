# Linux 应用目录与桌面安装集成

本页记录上游中性工程与宿主取证知识。本项目已在 `gpui_linux_appstream.mjs` 接入 DEP-11 catalog，实际实现和验证边界以 [Linux 打包合同](linux-packaging.md) 及 [产品知识库](../../../../docs/LINUX_GPUI_TOOLCHAIN.md) 为准；下文上游 helper 缺口不撤销该实现。

只在 Linux deb 的应用发现、桌面身份、软件中心图标、升级或卸载问题中读取。本文提炼已有下游的包集成经验，不新增安装、软件源或宿主修改步骤。

**当前工具边界：** Harness 的 GPUI 打包 helper 已生成 Debian 控制文件、`.desktop`、启动转发入口及 hicolor 图标，尚未生成 AppStream catalog，也不接受根 `packaging/gpui.json` 的 `linux` 文案对象。下文是按需接入的方案与诊断知识，不能作为现有生成能力或已验收结论；配置仍以 [配置合同](configuration.md) 与实际解析器为准。

## 分清入口、目录与包管理器

| 层次 | 拥有的事实 | 不能据此推断 |
| --- | --- | --- |
| Debian control/dpkg | 包名、版本、架构、依赖与安装文件归属 | 桌面菜单、软件中心条目或图标已经出现 |
| `.desktop` 与 hicolor | 可执行入口、可见名称、桌面 ID 和本地图标 | 任意软件中心都会把包作为应用发现 |
| AppStream MetaInfo | 单个组件的上游描述与元数据 | 已进入当前软件中心实际消费的目录或缓存 |
| AppStream catalog | 供消费者索引的组件、包名、启动入口与图标关联 | 包管理服务可用、授权已获得或安装/卸载成功 |
| 软件中心与包管理后端 | 目录消费、包状态解析、用户操作与真实事务 | 其它软件中心、发行版或版本具有相同行为 |

[AppStream Catalog 规范](https://www.freedesktop.org/software/appstream/docs/chap-CatalogData.html) 同时定义 XML 与 DEP-11 YAML。不能把某个客户端只枚举 YAML 子目录写成“Linux 不支持 XML”，也不能只补 `.desktop` 就宣称解决软件中心发现。

## 包与桌面身份关联

已有下游通过 deb 安装 `/usr/share/swcatalog/yaml/<identifier>.yml`，为目标消费者提供 [DEP-11 catalog](https://www.freedesktop.org/software/appstream/docs/sect-AppStream-YAML.html)。它是针对已确认消费者的包集成方案，不是所有 deb 都必须携带 catalog 的通用规则，也不替代发行版软件源的元数据生成流程。

接入时核对同一组已确认身份：catalog 的组件 `ID` 稳定，`Package` 与 Debian control 的包名一致，`Launchable.desktop-id` 指向实际安装的 `<identifier>.desktop`，`Icon.stock` 与主题图标名一致。启动文件的 `Exec` 指向真实转发入口；窗口 `app_id`/`StartupWMClass` 与桌面身份的关系另按实际后端核对。不要为刷新应用中心条目而改包名、冒充新应用或创建另一安装副本。

产品名和默认/中文摘要、描述来自已确认文案。生成 YAML 时安全编码标量；描述中的纯文本还须按 AppStream 标记规则转义，不能让 `&`、尖括号、引号或换行改变结构。身份和包版本沿用既有配置及 Cargo/control 事实，不在另一份元数据中维护独立版本。内容类别、描述和许可按真实产品选择，不把下游的业务文案、分类或固定长度预算当模板默认。

新增目录应进入最终包的精确文件集合、摘要与权限检查；沿用目录 `0755`、普通元数据 `0644` 的规则，并覆盖受限 umask。便携 tar.gz 只是解包运行，不据此登记系统安装或获得包管理器卸载入口。目录方案本身不配置 APT 源、下载远程图标、写 Snap 私有目录或生成维护脚本。

## 按软件中心的实际消费路径诊断

2026-10-20 核对的 Ubuntu App Center 源码快照为 `d7d3fbe8`。其 [AppStream 服务](https://github.com/ubuntu/app-center/blob/d7d3fbe8/packages/app_center/lib/appstream/appstream_service.dart) 用 catalog 中的包名建立查询关系，再由 [DebModel](https://github.com/ubuntu/app-center/blob/d7d3fbe8/packages/app_center/lib/deb/deb_model.dart) 通过 PackageKit 解析包状态及发起移除。补足目录仅提供关联输入；服务、授权、事务结果和用户数据保留仍需分别观察。

该快照的 [已安装列表](https://github.com/ubuntu/app-center/blob/d7d3fbe8/packages/app_center/lib/manage/local_deb_providers.dart#L93-L121) 按已安装包名查目录，没有匹配组件的条目会被过滤。其锁定的 `appstream 0.2.11` 的 [pool 实现](https://github.com/canonical/appstream.dart/blob/33b05768a81b389da831f526086299911322a2fa/lib/src/pool.dart#L40-L71) 枚举 catalog 根层 XML 和 `yaml` 子目录，未枚举标准 `xml` 子目录，也不扫描 MetaInfo；这是特定依赖版本的消费边界，不是 XML 或 MetaInfo 格式本身无效。

同一快照的 [本地 deb 页面](https://github.com/ubuntu/app-center/blob/d7d3fbe8/packages/app_center/lib/deb/local_deb_page.dart#L132-L153) 在已安装时禁用按钮，没有比较待安装版本；[图标辅助函数](https://github.com/ubuntu/app-center/blob/d7d3fbe8/packages/app_center/lib/appstream/appstream_utils.dart#L82-L101) 只消费 stock 主题图标或 remote URL。因此 catalog 补齐不能单独证明双击升级按钮、详情页 Logo 或已安装页图标已修复。这些是该快照的实现观察，不是全部 Ubuntu 或软件中心的永久限制。

stock 图标在宿主文件系统存在，还要确认软件中心进程能通过实际主题搜索路径读取。Snap 沙箱、主题内容接口及缓存可能改变可见范围；隔离解包路径也未必对桌面进程可见。先核对实际应用/依赖版本、目录枚举、缓存和进程可见性，不通过更换包名、写私有目录或添加网络源掩盖问题。目标版本或打包渠道改变时重新核对，不能沿用旧快照断言。

应用中心还可能持有独立的进程内目录索引。上述 AppStream 服务有自己的加载/重载路径，不能把原生 `appstreamcli` 的缓存等同于该索引；刷新原生缓存不证明已运行的应用中心已重新加载新增目录。诊断时分别确认包内文件、消费者枚举及实际重载结果。

## 验证分层

| 实际检查 | 能报告的结论 | 仍待证明 |
| --- | --- | --- |
| 最终 deb 解包并核对 control、catalog、desktop、图标、权限和摘要 | 当前最终包携带一致元数据 | 宿主安装、索引与桌面呈现 |
| 原生 `appstreamcli convert` 消费生成目录并核对转换结果 | 指定工具版本可解析，关联及转义内容被保留 | 完整规范符合性、软件中心实际索引 |
| 针对所用格式执行明确的规范校验 | 工具实际覆盖的规范规则通过，问题按真实严重程度披露 | 其它消费者兼容、包管理事务与界面效果 |
| 软件中心目录加载和 PackageKit 查询 | 指定宿主能找到组件及其包状态 | 图标呈现、升级或卸载成功 |
| 最终候选在目标桌面实际安装、发现、显示图标、升级、卸载 | 分别完成的真实场景 | 其它架构、桌面、发行版或新版候选 |

[appstreamcli 手册](https://www.freedesktop.org/software/appstream/docs/re01.html) 区分 `convert` 和 `validate`；不可把格式转换、严格 JSON 配置校验或按字符串断言等同于完整 AppStream 规范校验。原生解析回归因工具缺失或非 Linux 宿主跳过时据实记录，不能报告通过，也不因阅读本知识库自动安装工具或扩大环境门禁。

真实安装/卸载按本次授权和最终候选场景执行，先保留已有包与用户数据的必要恢复事实，只操作本应用。启动、应用发现、图标、升级和卸载分别记录源码/包摘要、架构、桌面、软件中心及后端版本；某一步成功不填补其它步骤。原始验收记录留在下游候选证据中，旧候选通过不覆盖新加入的目录机制。
