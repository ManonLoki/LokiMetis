//! 完全隔离的合成 Codex 数据根与索引/扫描辅助函数，供本模块下的测试文件共用。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use tauri::async_runtime::block_on;

use loki_metis_core::{LocalUsageAggregate, ProviderKind};

use crate::backend::local_index::{
    CancellationToken, DiscoveredRoot, DiscoveryInputs, LocalIndex, RegisteredRoot, ScanConfig,
    ScanMode, ScanSummary, discover_quick, scan_discovered_roots,
};

/// 直接重开已存在的索引数据库文件，仅供测试注入无法通过公开 API 表达的
/// 历史 fixture（例如手工把某些行标成旧 parser generation）。绕开
/// `LocalIndex` 的业务方法，测试断言的是“查询代码是否正确按当前 parser
/// 版本过滤”，而不是索引写入路径本身。
pub(super) fn reopen_for_fixture(database_path: &Path) -> DatabaseConnection {
    let mut options = ConnectOptions::new("sqlite://placeholder.sqlite3");
    let database_path = database_path.to_path_buf();
    options.map_sqlx_sqlite_opts(move |sqlite_options| sqlite_options.filename(&database_path));
    block_on(Database::connect(options)).expect("database reopens for fixture manipulation")
}

/// 在测试 fixture 连接上执行一条无参数原始 SQL 语句。
pub(super) fn execute_fixture_sql(connection: &DatabaseConnection, sql: &str) {
    block_on(connection.execute_unprepared(sql)).expect("fixture SQL statement succeeds");
}

// 这份 fixture 库有一个反复出现的设计手法值得先说明：session_line/
// token_line 生成的 JSON 里故意混入了看起来像真实敏感信息的“诱饵”字段
// （`sk-privacy-bait-secret` 这种假 API key 格式、假邮箱、`cwd` 里带
// "privacy-bait" 字样的路径、`answer` 回答正文）。这些字段名和值不是
// 随便选的——它们是解析器*不应该*提取进结构化字段的内容。测试断言只要
// 检查最终 canonical 调用记录 / SQLite 内容里完全不出现这些诱饵字符串，
// 就能验证“解析器只挑白名单字段、正文和敏感信息不会意外落库”这条隐私
// 承诺是真的成立，而不是仅凭代码走查“看起来没读那些字段”。

/// 创建只包含允许会话区域的合成 Codex 数据根。
pub(super) fn create_root(parent: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    fs::create_dir_all(root.join("sessions")).expect("active session area is created");
    fs::create_dir_all(root.join("archived_sessions")).expect("archived session area is created");
    root
}

/// 生成带隐私诱饵的 session 元数据，解析器只能保留散列键。
pub(super) fn session_line(session_id: &str) -> String {
    format!(
        "{{\"timestamp\":\"2026-07-30T10:00:00Z\",\"type\":\"session_meta\",\
         \"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"/private/privacy-bait-project\",\
         \"prompt\":\"sk-privacy-bait-secret\",\"email\":\"privacy-bait@example.com\"}}}}\n"
    )
}

/// 生成一个当前 schema 的单次 Token 事件，并带有不得落库的回答诱饵。
pub(super) fn token_line(timestamp: &str, call_id: &str, input: u64, cached: u64) -> String {
    format!(
        "{{\"timestamp\":\"{timestamp}\",\"type\":\"event_msg\",\"payload\":{{\
         \"type\":\"token_count\",\"call_id\":\"{call_id}\",\"model\":\"gpt-5\",\
         \"reasoning_effort\":\"high\",\"info\":{{\"last_token_usage\":{{\
         \"input_tokens\":{input},\"cached_input_tokens\":{cached},\
         \"cache_write_input_tokens\":2,\"output_tokens\":10,\
         \"reasoning_output_tokens\":3,\"total_tokens\":{}}}}},\
         \"answer\":\"privacy-bait-answer\"}}}}\n",
        input + 10
    )
}

/// 生成同时携带单次与累计 Token 的真实形状事件，供重放/估算增量回归使用。
#[allow(clippy::too_many_arguments)]
pub(super) fn token_snapshot_line(
    timestamp: &str,
    last_input: u64,
    last_cached: u64,
    last_output: u64,
    last_total: u64,
    cumulative_input: u64,
    cumulative_cached: u64,
    cumulative_output: u64,
    cumulative_total: u64,
) -> String {
    format!(
        "{{\"timestamp\":\"{timestamp}\",\"type\":\"event_msg\",\"payload\":{{\
         \"type\":\"token_count\",\"model\":\"gpt-5\",\"info\":{{\
         \"last_token_usage\":{{\"input_tokens\":{last_input},\
         \"cached_input_tokens\":{last_cached},\"cache_write_input_tokens\":0,\
         \"output_tokens\":{last_output},\"reasoning_output_tokens\":0,\
         \"total_tokens\":{last_total}}},\"total_token_usage\":{{\
         \"input_tokens\":{cumulative_input},\"cached_input_tokens\":{cumulative_cached},\
         \"cache_write_input_tokens\":0,\"output_tokens\":{cumulative_output},\
         \"reasoning_output_tokens\":0,\"total_tokens\":{cumulative_total}}}}},\
         \"answer\":\"privacy-bait-answer\"}}}}\n"
    )
}

/// 生成缺少单次事实的累计回退事件，用于验证不会重复累加会话累计值。
pub(super) fn cumulative_line(timestamp: &str, input: u64) -> String {
    format!(
        "{{\"timestamp\":\"{timestamp}\",\"type\":\"event_msg\",\"payload\":{{\
         \"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\
         \"input_tokens\":{input},\"cached_input_tokens\":0,\"output_tokens\":10,\
         \"reasoning_output_tokens\":2,\"total_tokens\":{}}}}}}}}}\n",
        input + 10
    )
}

/// 将完整合成 rollout 写入指定位置。
// 一次性写入完整文件（session_meta 行 + 若干调用行拼接），
// 模拟"文件已经存在、内容不再变化"的稳定状态，用于测试首次索引路径。
pub(super) fn write_rollout(path: &Path, session_id: &str, calls: &[String]) {
    let mut content = session_line(session_id);
    for call in calls {
        content.push_str(call);
    }
    fs::write(path, content).expect("synthetic rollout is written");
}

/// 追加字节到活动 rollout，模拟 Codex 进程持续写入。
// 用 `OpenOptions::new().append(true)` 而不是覆盖写：真实 Codex 进程
// 写 rollout 文件的方式就是不断在文件末尾追加新行，这个辅助函数专门
// 配合"增量扫描应该只解析新追加内容，不重新处理已提交部分"这类测试。
pub(super) fn append_rollout(path: &Path, content: &str) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("synthetic rollout is opened for append");
    file.write_all(content.as_bytes())
        .expect("synthetic append succeeds");
}

/// 用显式登记输入发现测试根，确保测试不读取 HOME 或当前 CODEX_HOME。
// 只填 `registered_roots`，把 `home_dir`/`codex_home` 都留空——这样
// discover_quick 内部完全不会去检查执行测试的这台机器上真实存在的
// 用户主目录或环境变量，测试结果 100% 由测试代码自己控制的路径决定，
// 不会因为在不同开发者机器或 CI 环境上运行而产生不同行为。
pub(super) fn discover_registered(paths: &[PathBuf]) -> Vec<DiscoveredRoot> {
    let inputs = DiscoveryInputs {
        home_dir: None,
        codex_home: None,
        registered_roots: paths
            .iter()
            .enumerate()
            .map(|(index, path)| RegisteredRoot {
                root_id: None,
                path: path.clone(),
                alias: format!("测试根 {index}"),
                enabled: true,
            })
            .collect(),
    };
    discover_quick(&inputs, &CancellationToken::new()).roots
}

/// 在隔离 app-data 内打开索引并执行一次无 UI 副作用扫描。
pub(super) fn scan(
    index: &mut LocalIndex,
    roots: &[DiscoveredRoot],
    cancellation: &CancellationToken,
) -> ScanSummary {
    block_on(scan_discovered_roots(
        index,
        roots,
        ScanConfig {
            mode: ScanMode::Quick,
            started_at_epoch_ms: Some(1_000),
            ..ScanConfig::default()
        },
        cancellation,
        |_| {},
    ))
    .expect("synthetic roots are scanned")
}

/// 显式读取 Codex provider 的完整聚合，避免把大对象挂在轻量扫描结果上。
pub(super) fn codex_aggregate(index: &mut LocalIndex) -> LocalUsageAggregate {
    block_on(index.aggregate_for_provider(ProviderKind::RolloutJsonl))
        .expect("Codex provider aggregate loads")
}
