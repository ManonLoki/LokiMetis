//! 未改变文件的解析复用：结果与重新解析逐项相同，改变的文件必须重新读取。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;
use tempfile::tempdir;

use super::cache::parses_on_this_thread;
use super::read_project_usage;

/// 生成一条合法 usage 事件；`message` 决定逻辑调用身份。
fn event(session: &str, message: &str, input: i64) -> String {
    json!({
        "timestamp": 1_760_000_000_000_i64,
        "sessionId": session,
        "cwd": "/work/demo",
        "providerData": {
            "messageId": message,
            "model": "model-a",
            "usage": {
                "inputTokens": input,
                "inputTokensDetails": [{ "cached_tokens": 1 }],
                "outputTokens": 2,
                "totalTokens": input + 2,
                "requests": 1
            }
        }
    })
    .to_string()
}

/// 在夹具根下创建 `projects/<project>/<session>.jsonl`。
fn transcript(home: &Path, lines: &[String]) -> PathBuf {
    let directory = home.join("projects/demo");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.jsonl");
    fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

/// 缓存键含根 ID，各测试使用独立根，互不影响全局缓存。
fn root(home: &Path) -> String {
    format!("cache-test-{}", home.display())
}

/// 未改变的文件第二轮不再解析，且记录与警告完全相同。
#[test]
fn unchanged_file_is_not_parsed_again_and_results_match() {
    let temp = tempdir().unwrap();
    let home = temp.path();
    transcript(home, &[event("s", "m1", 10), event("s", "m2", 20), "not json".to_owned()]);
    let root = root(home);

    let before = parses_on_this_thread();
    let first = read_project_usage(home, &root).unwrap();
    let second = read_project_usage(home, &root).unwrap();

    assert_eq!(parses_on_this_thread() - before, 1, "第二轮必须命中缓存");
    assert!(first.records == second.records);
    assert_eq!(first.records.len(), 2);
    assert_eq!(first.coverage.warning_count, 1);
    assert_eq!(second.coverage.warning_count, 1, "格式警告随缓存一并保留");
}

/// 文件追加后长度与修改时间都变化，必须重新解析并包含新记录。
#[test]
fn appended_file_is_parsed_again() {
    let temp = tempdir().unwrap();
    let home = temp.path();
    let path = transcript(home, &[event("s", "m1", 10)]);
    let root = root(home);
    assert_eq!(read_project_usage(home, &root).unwrap().records.len(), 1);

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file, "{}", event("s", "m2", 20)).unwrap();
    drop(file);

    let before = parses_on_this_thread();
    let updated = read_project_usage(home, &root).unwrap();
    assert_eq!(parses_on_this_thread() - before, 1);
    assert_eq!(updated.records.len(), 2);
}

/// 长度相同但内容被替换时，修改时间/状态变更时间不同，不能返回旧记录。
#[test]
fn same_length_rewrite_is_not_served_from_cache() {
    let temp = tempdir().unwrap();
    let home = temp.path();
    let path = transcript(home, &[event("s", "m1", 10)]);
    let root = root(home);
    let first = read_project_usage(home, &root).unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&path, event("s", "m1", 11) + "\n").unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().len() as usize,
        (event("s", "m1", 10) + "\n").len(),
        "夹具必须保持同样长度"
    );

    let second = read_project_usage(home, &root).unwrap();
    assert_eq!(second.records.len(), 1);
    assert_ne!(first.records[0].input_tokens, second.records[0].input_tokens);
    assert_eq!(second.records[0].input_tokens, 11);
}

/// Windows 同长度日志原地改写并恢复修改时间，真实 ChangeTime 必须使旧缓存失效。
#[cfg(windows)]
#[test]
fn preserved_mtime_rewrite_is_not_served_from_cache() {
    let temp = tempdir().unwrap();
    let path = transcript(temp.path(), &[event("s", "m1", 10)]);
    let root = root(temp.path());
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    read_project_usage(temp.path(), &root).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&path, event("s", "m1", 11) + "\n").unwrap();
    fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
    let before = parses_on_this_thread();
    let updated = read_project_usage(temp.path(), &root).unwrap();
    assert_eq!(updated.records[0].input_tokens, 11);
    assert_eq!(parses_on_this_thread() - before, 1);
}

/// Windows 同名日志替换且长度/修改时间一致，打开句柄的真实文件 ID 必须区分新文件。
#[cfg(windows)]
#[test]
fn preserved_mtime_replacement_is_not_served_from_cache() {
    let temp = tempdir().unwrap();
    let path = transcript(temp.path(), &[event("s", "m1", 10)]);
    let root = root(temp.path());
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    read_project_usage(temp.path(), &root).unwrap();
    let replacement = temp.path().join("replacement");
    fs::write(&replacement, event("s", "m1", 11) + "\n").unwrap();
    fs::File::options().write(true).open(&replacement).unwrap().set_modified(modified).unwrap();
    fs::rename(&path, temp.path().join("old")).unwrap();
    fs::rename(replacement, &path).unwrap();
    let before = parses_on_this_thread();
    let updated = read_project_usage(temp.path(), &root).unwrap();
    assert_eq!(updated.records[0].input_tokens, 11);
    assert_eq!(parses_on_this_thread() - before, 1);
}

/// 已缓存的文件被删除后，下一轮既不再返回其记录，也会被清出缓存。
#[test]
fn removed_file_stops_contributing_records() {
    let temp = tempdir().unwrap();
    let home = temp.path();
    let path = transcript(home, &[event("s", "m1", 10)]);
    let root = root(home);
    assert_eq!(read_project_usage(home, &root).unwrap().records.len(), 1);

    fs::remove_file(&path).unwrap();
    assert!(read_project_usage(home, &root).unwrap().records.is_empty());

    // 同名文件重新出现且内容相同，也必须重新解析而不是复活旧缓存。
    transcript(home, &[event("s", "m1", 10)]);
    let before = parses_on_this_thread();
    assert_eq!(read_project_usage(home, &root).unwrap().records.len(), 1);
    assert_eq!(parses_on_this_thread() - before, 1);
}
