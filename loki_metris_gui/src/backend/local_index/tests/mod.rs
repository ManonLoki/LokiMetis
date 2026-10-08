//! 使用完全隔离的合成数据根验证本机索引闭环，禁止访问真实用户目录；
//! 按场景拆分为多个子模块，[`support`] 收纳所有测试共用的 fixture 与辅助函数。
//! 这是本 crate 里体量最大的集成测试组：不像单元测试那样只测一个纯函数，
//! 这里每个用例都要在 `tempfile::tempdir()` 建的临时目录里真实写文件、
//! 真实打开 SQLite、跑一遍“发现 -> 扫描 -> 索引 -> 查询”的完整闭环，
//! 验证的是模块之间协作是否正确，而不是单个函数的输入输出。

mod clear_index_cache_tests; // 审计清空索引对派生 SQLite 表与 last_coverage_state 的真实效果
mod codex_inflation_tests; // 验证 last 复制 total / same-last 虚高夹具的流式与索引路径
mod codex_ownership_tests; // 验证 fork 复制前缀不会入库且断点续扫保持自身归属
mod codex_remaining_inflation_tests; // 验证 checkpoint restore 后再遇 last-copies/same-last 不重计
mod incremental_scan_tests; // 验证追加/替换/重建三种增量场景的判定与结果
mod parser_upgrade_tests; // 验证 parser generation 升级后旧数据的重扫与迁移
mod root_lifecycle_tests; // 验证数据根登记、启停、重命名、失效的完整生命周期
mod scan_state_tests; // 验证扫描四态（未扫描/需重扫/无调用/就绪）判定
mod signature_budget_tests; // 验证签名探测预算耗尽时的保守回退行为
mod support; // 全部子模块共用的合成数据根构造、写文件、断言辅助函数
