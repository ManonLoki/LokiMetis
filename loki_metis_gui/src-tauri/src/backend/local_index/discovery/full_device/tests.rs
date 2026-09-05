use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tempfile::tempdir;

use super::super::FullDiscoveryOptions;
use super::*;
use crate::backend::local_index::CancellationToken;
#[cfg(target_os = "windows")]
use crate::backend::local_index::is_obviously_network_path;
use loki_metis_core::CoverageState;

/// 验证 Windows canonicalize 产生的扩展盘符路径不会被误判为 UNC 网络路径。
#[cfg(target_os = "windows")]
#[test]
fn windows_verbatim_disk_path_is_local() {
    assert!(!is_obviously_network_path(Path::new(r"\\?\C:\local\codex")));
    assert!(is_obviously_network_path(Path::new(
        r"\\server\share\codex"
    )));
    assert!(is_obviously_network_path(Path::new(
        r"\\?\UNC\server\share\codex"
    )));
}

/// 验证两个隔离起点都能发现未知位置中的有效 rollout 结构签名。
#[test]
fn full_discovery_finds_valid_roots_across_multiple_volume_roots() {
    let first = tempdir().expect("first synthetic volume is available");
    let second = tempdir().expect("second synthetic volume is available");
    write_signature_rollout(
        &first
            .path()
            .join("apps/codex-a/sessions/2026/07/rollout-a.jsonl"),
    );
    write_signature_rollout(
        &second
            .path()
            .join("profiles/codex-b/archived_sessions/rollout-b.jsonl"),
    );

    let mut progress = Vec::new();
    let result = discover_full_device_with_progress(
        &FullDiscoveryOptions {
            search_roots: vec![first.path().to_path_buf(), second.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
        |item| progress.push(item),
    );

    assert_eq!(result.roots.len(), 2);
    assert_eq!(result.coverage.roots_discovered, 2);
    assert_eq!(
        progress.last(),
        Some(&DiscoveryProgress {
            directories_scanned: result.directories_scanned,
            roots_discovered: 2,
        })
    );
}

/// 验证相同文件系统快照在单 worker 与四 worker 下返回完全一致的根与覆盖事实。
#[test]
fn parallel_and_single_worker_discovery_are_identical() {
    let first = tempdir().expect("first synthetic volume is available");
    let second = tempdir().expect("second synthetic volume is available");
    write_signature_rollout(
        &first
            .path()
            .join("profiles/alpha/sessions/rollout-alpha.jsonl"),
    );
    write_signature_rollout(
        &first
            .path()
            .join("profiles/beta/archived_sessions/rollout-beta.jsonl"),
    );
    write_signature_rollout(
        &second
            .path()
            .join("nested/gamma/sessions/rollout-gamma.jsonl"),
    );
    fs::create_dir_all(first.path().join("ordinary/one/two"))
        .expect("ordinary fixture tree is created");
    fs::create_dir_all(second.path().join("ordinary/three/four"))
        .expect("second ordinary fixture tree is created");
    let options = FullDiscoveryOptions {
        search_roots: vec![first.path().to_path_buf(), second.path().to_path_buf()],
        ..FullDiscoveryOptions::default()
    };

    let mut single_progress = Vec::new();
    let single = discover_full_device_with_workers(
        &options,
        &CancellationToken::new(),
        1,
        |progress| single_progress.push(progress),
        &|_| {},
    );
    let mut parallel_progress = Vec::new();
    let parallel = discover_full_device_with_workers(
        &options,
        &CancellationToken::new(),
        4,
        |progress| parallel_progress.push(progress),
        &|path| {
            if path.ends_with("alpha") {
                std::thread::sleep(Duration::from_millis(20));
            }
        },
    );

    assert_eq!(parallel, single);
    assert_eq!(parallel_progress, single_progress);
    assert_eq!(parallel.roots.len(), 3);
}

/// 验证 worker 确实并行执行目录 I/O，且同时活跃数量不会越过固定四线程硬上限。
#[test]
fn parallelism_is_observable_and_hard_capped() {
    let temp = tempdir().expect("parallel fixture root is available");
    for index in 0..16 {
        fs::create_dir_all(temp.path().join(format!("branch-{index:02}/leaf")))
            .expect("parallel fixture branch is created");
    }
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(AtomicUsize::new(0));
    let hook = {
        let active = Arc::clone(&active);
        let peak = Arc::clone(&peak);
        let observed = Arc::clone(&observed);
        move |_path: &Path| {
            observed.fetch_add(1, Ordering::SeqCst);
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(current, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(10));
            active.fetch_sub(1, Ordering::SeqCst);
        }
    };

    let result = discover_full_device_with_workers(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
        8,
        |_| {},
        &hook,
    );

    assert_eq!(result.coverage.state, CoverageState::Complete);
    assert!(observed.load(Ordering::SeqCst) > DISCOVERY_ROUND_SIZE);
    assert!(peak.load(Ordering::SeqCst) >= 2);
    assert!(peak.load(Ordering::SeqCst) <= MAX_DISCOVERY_WORKERS);
}

/// 验证并行 worker 共享一份批次遍历额度，不会把目录上限按线程数放大。
#[test]
fn parallel_workers_share_one_traversal_budget() {
    let temp = tempdir().expect("budget fixture root is available");
    for index in 0..24 {
        fs::create_dir_all(temp.path().join(format!("branch-{index:02}")))
            .expect("budget fixture branch is created");
    }
    let options = FullDiscoveryOptions {
        search_roots: vec![temp.path().to_path_buf()],
        max_directories: 3,
        max_entries: 64,
        max_traversal_batches: 2,
        ..FullDiscoveryOptions::default()
    };

    let single =
        discover_full_device_with_workers(&options, &CancellationToken::new(), 1, |_| {}, &|_| {});
    let parallel =
        discover_full_device_with_workers(&options, &CancellationToken::new(), 4, |_| {}, &|_| {});

    assert_eq!(parallel, single);
    assert_eq!(parallel.coverage.state, CoverageState::Partial);
    assert!(parallel.directories_scanned <= 6);
}

/// 验证目录项临界额度在单 worker 与四 worker 下产生完全一致的部分覆盖。
#[test]
fn parallel_and_single_worker_entry_budget_boundary_are_identical() {
    let temp = tempdir().expect("entry budget fixture root is available");
    for index in 0..8 {
        fs::create_dir_all(temp.path().join(format!("branch-{index:02}")))
            .expect("entry budget fixture branch is created");
    }
    let options = FullDiscoveryOptions {
        search_roots: vec![temp.path().to_path_buf()],
        max_entries: 3,
        max_traversal_batches: 1,
        ..FullDiscoveryOptions::default()
    };

    let single =
        discover_full_device_with_workers(&options, &CancellationToken::new(), 1, |_| {}, &|_| {});
    let parallel =
        discover_full_device_with_workers(&options, &CancellationToken::new(), 4, |_| {}, &|_| {});

    assert_eq!(parallel, single);
    assert_eq!(parallel.coverage.state, CoverageState::Partial);
}

/// 验证目录项额度为零时仍可检查显式搜索根本身的严格签名。
#[test]
fn zero_entry_budget_still_checks_signed_search_root() {
    let temp = tempdir().expect("signed root fixture is available");
    write_signature_rollout(&temp.path().join("sessions/rollout-direct.jsonl"));

    for workers in [1, 4] {
        let result = discover_full_device_with_workers(
            &FullDiscoveryOptions {
                search_roots: vec![temp.path().to_path_buf()],
                max_entries: 0,
                max_traversal_batches: 1,
                ..FullDiscoveryOptions::default()
            },
            &CancellationToken::new(),
            workers,
            |_| {},
            &|_| {},
        );

        assert_eq!(result.roots.len(), 1);
        assert_eq!(result.directories_scanned, 1);
    }
}

/// 验证父卷遍历不会进入另一个显式排队的子卷，子卷仍从自身入口独立发现。
#[test]
fn parallel_scan_prunes_another_explicit_search_root() {
    let parent = tempdir().expect("parent volume fixture is available");
    let mounted = parent.path().join("a-mounted");
    write_signature_rollout(&mounted.join("sessions/rollout-mounted.jsonl"));

    let result = discover_full_device_with_workers(
        &FullDiscoveryOptions {
            search_roots: vec![mounted.clone(), parent.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
        4,
        |_| {},
        &|_| {},
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(result.directories_scanned, 2);
    assert_eq!(result.coverage.skipped_count, 1);
}

/// 验证排除目录只参与覆盖统计，不会提前消费同轮中合法候选的共享目录额度。
#[test]
fn excluded_directory_does_not_consume_directory_budget() {
    let temp = tempdir().expect("exclusion fixture root is available");
    let excluded = temp.path().join("a-excluded");
    let valid = temp.path().join("b-valid");
    fs::create_dir_all(&excluded).expect("excluded fixture is created");
    write_signature_rollout(&valid.join("sessions/rollout-valid.jsonl"));

    let result = discover_full_device_with_workers(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            excluded_roots: vec![excluded],
            max_directories: 2,
            max_traversal_batches: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
        4,
        |_| {},
        &|_| {},
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].path,
        fs::canonicalize(valid).expect("valid root canonicalizes")
    );
    assert_eq!(result.directories_scanned, 2);
}

/// 验证同一目录的子目录在 EOF 后按无损路径键排序，临界签名预算不会受 read_dir 顺序影响。
#[test]
fn tight_signature_budget_uses_stable_child_order() {
    let temp = tempdir().expect("ordered fixture root is available");
    let expected = temp.path().join("a-valid");
    write_signature_rollout(&expected.join("sessions/rollout-valid.jsonl"));
    fs::create_dir_all(temp.path().join("z-consuming/sessions"))
        .expect("signature budget consumer is created");
    let options = FullDiscoveryOptions {
        search_roots: vec![temp.path().to_path_buf()],
        max_signature_directories: 1,
        ..FullDiscoveryOptions::default()
    };

    for workers in [1, 4] {
        let result = discover_full_device_with_workers(
            &options,
            &CancellationToken::new(),
            workers,
            |_| {},
            &|_| {},
        );
        assert_eq!(result.coverage.state, CoverageState::Partial);
        assert_eq!(result.roots.len(), 1);
        assert_eq!(
            result.roots[0].path,
            fs::canonicalize(&expected).expect("expected root canonicalizes")
        );
    }
}

/// 验证单个 worker 意外 panic 会转为保守部分覆盖，不会让协调器永久等待缺失结果。
#[test]
fn worker_panic_returns_partial_without_deadlock() {
    let temp = tempdir().expect("panic fixture root is available");

    let result = discover_full_device_with_workers(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
        4,
        |_| {},
        &|_| panic!("synthetic worker failure"),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert!(result.coverage.skipped_count >= 1);
}

/// 验证取消后协调器不再派发新轮次，且最多等待当前八个有界 ticket 回收。
#[test]
fn parallel_cancellation_stops_new_leases() {
    let temp = tempdir().expect("cancellation fixture root is available");
    for index in 0..32 {
        fs::create_dir_all(temp.path().join(format!("branch-{index:02}/leaf")))
            .expect("cancellation fixture branch is created");
    }
    let cancellation = CancellationToken::new();
    let observed = AtomicUsize::new(0);

    let result = discover_full_device_with_workers(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &cancellation,
        4,
        |_| {},
        &|_| {
            let current = observed.fetch_add(1, Ordering::SeqCst) + 1;
            if current == 3 {
                cancellation.cancel();
            }
        },
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert!(observed.load(Ordering::SeqCst) <= DISCOVERY_ROUND_SIZE + 2);
}

/// 验证未知候选的空目录、错误文件名、普通 JSON 与畸形首记录不会误判。
#[test]
fn full_discovery_rejects_false_positive_session_directories() {
    let temp = tempdir().expect("synthetic volume is available");
    fs::create_dir_all(temp.path().join("empty/sessions")).expect("empty area is created");
    fs::create_dir_all(temp.path().join("wrong-name/sessions"))
        .expect("wrong-name area is created");
    fs::write(
        temp.path().join("wrong-name/sessions/session.jsonl"),
        "{\"type\":\"session_meta\"}\n",
    )
    .expect("wrong-name fixture is written");
    fs::create_dir_all(temp.path().join("malformed/sessions")).expect("malformed area is created");
    fs::write(
        temp.path()
            .join("malformed/sessions/rollout-malformed.jsonl"),
        "{not-json}\n",
    )
    .expect("malformed fixture is written");
    fs::create_dir_all(temp.path().join("ordinary-json/sessions"))
        .expect("ordinary JSON area is created");
    fs::write(
        temp.path()
            .join("ordinary-json/sessions/rollout-ordinary.jsonl"),
        "{\"timestamp\":\"2026-07-31T08:00:00Z\",\"type\":\"ordinary_record\",\"payload\":{\"id\":\"synthetic\"}}\n",
    )
    .expect("ordinary JSON fixture is written");

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert!(result.coverage.skipped_count >= 4);
}

/// 验证发现真实 Codex 根后立即停止下钻，根内 logs 即使伪造会话结构也不会成为第二数据源。
#[test]
fn full_discovery_stops_at_signed_root_and_never_registers_nested_logs() {
    let temp = tempdir().expect("synthetic volume is available");
    let codex_root = temp.path().join("profile/.codex");
    write_signature_rollout(&codex_root.join("sessions/rollout-primary.jsonl"));
    write_signature_rollout(
        &codex_root
            .join("logs")
            .join("sessions")
            .join("rollout-log-copy.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].path,
        fs::canonicalize(codex_root).expect("signed root canonicalizes")
    );
    assert_ne!(result.roots[0].alias, "logs");
}

/// 验证主动发现会剪枝独立 logs 子树，即使其中伪造完整 rollout 签名也不能登记。
#[test]
fn full_discovery_prunes_standalone_logs_root_with_valid_rollout() {
    let temp = tempdir().expect("synthetic volume is available");
    let valid = temp.path().join("profile/.codex");
    let forbidden = temp.path().join("other/logs");
    write_signature_rollout(&valid.join("sessions/rollout-valid.jsonl"));
    write_signature_rollout(&forbidden.join("sessions/rollout-bait.jsonl"));

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].path,
        fs::canonicalize(valid).expect("valid root canonicalizes")
    );
    assert!(result.roots.iter().all(|root| root.path != forbidden));
    assert!(result.coverage.skipped_count >= 1);
}

/// 验证未知候选的会话区域是符号链接时不会穿越到目标 rollout。
#[cfg(unix)]
#[test]
fn full_discovery_rejects_symlink_session_area() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("synthetic volume is available");
    let target = temp.path().join("linked-session-target");
    write_signature_rollout(&target.join("rollout-linked.jsonl"));
    let candidate = temp.path().join("candidate");
    fs::create_dir_all(&candidate).expect("candidate root is created");
    symlink(&target, candidate.join("sessions")).expect("session symlink is created");

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert!(result.symlink_skipped_count >= 1);
}

/// 验证主动发现收到取消后停止遍历并把覆盖标为取消。
#[test]
fn full_discovery_reports_cancellation() {
    let temp = tempdir().expect("temporary root is available");
    fs::create_dir_all(temp.path().join("nested")).expect("fixture tree is created");
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &cancellation,
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert_eq!(result.directories_scanned, 0);
    assert_eq!(result.coverage.roots_scanned, 0);
}

/// 验证进度回调触发取消后不会继续进入未知候选结构探测。
#[test]
fn progress_cancellation_stops_before_signature_probe() {
    let temp = tempdir().expect("temporary root is available");
    write_signature_rollout(&temp.path().join("sessions/rollout-cancelled.jsonl"));
    let cancellation = CancellationToken::new();

    let result = discover_full_device_with_progress(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &cancellation,
        |_| cancellation.cancel(),
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert!(result.roots.is_empty());
    assert_eq!(result.directories_scanned, 1);
}

/// 验证已确认的数据根在随后取消时仍被保留并计入覆盖。
#[test]
fn cancellation_retains_roots_confirmed_before_the_signal() {
    let temp = tempdir().expect("temporary root is available");
    write_signature_rollout(
        &temp
            .path()
            .join("candidate/sessions/rollout-confirmed.jsonl"),
    );
    fs::create_dir_all(temp.path().join("other")).expect("second directory is created");
    let cancellation = CancellationToken::new();

    let result = discover_full_device_with_progress(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &cancellation,
        |progress| {
            if progress.roots_discovered == 1 {
                cancellation.cancel();
            }
        },
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert_eq!(result.roots.len(), 1);
}

/// 验证主动遍历目录预算耗尽属于部分覆盖，而不是冒充用户取消。
#[test]
fn traversal_budget_exhaustion_is_partial() {
    let temp = tempdir().expect("temporary root is available");
    fs::create_dir_all(temp.path().join("nested")).expect("fixture tree is created");

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            max_directories: 1,
            max_traversal_batches: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert_eq!(result.coverage.roots_scanned, 1);
    assert_eq!(result.directories_scanned, 1);
}

/// 验证单批目录额度用尽后会继续下一批，并发现不同深度和名称的全部有效根。
#[test]
fn traversal_continues_across_bounded_batches_to_find_all_signed_roots() {
    let temp = tempdir().expect("batched traversal root is available");
    write_signature_rollout(
        &temp
            .path()
            .join("first/arbitrary/sessions/rollout-first.jsonl"),
    );
    write_signature_rollout(
        &temp
            .path()
            .join("second/deeper/location/archived_sessions/rollout-second.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            max_directories: 2,
            max_entries: 3,
            max_traversal_batches: 8,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Complete);
    assert_eq!(result.roots.len(), 2);
    assert!(result.directories_scanned > 2);
}

/// 验证主动遍历目录项预算恰好用完仍完整，出现第 N+1 项才报告部分覆盖。
#[test]
fn traversal_entry_budget_distinguishes_exact_limit_from_overflow() {
    let exact = tempdir().expect("exact-budget root is available");
    File::create(exact.path().join("only-entry")).expect("single entry is created");
    let exact_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![exact.path().to_path_buf()],
            max_entries: 1,
            max_traversal_batches: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    let overflow = tempdir().expect("overflow root is available");
    File::create(overflow.path().join("first-entry")).expect("first entry is created");
    File::create(overflow.path().join("second-entry")).expect("second entry is created");
    let overflow_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![overflow.path().to_path_buf()],
            max_entries: 1,
            max_traversal_batches: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(exact_result.coverage.state, CoverageState::Complete);
    assert_eq!(exact_result.directories_scanned, 1);
    assert_eq!(overflow_result.coverage.state, CoverageState::Partial);
    assert_eq!(overflow_result.directories_scanned, 1);
}

/// 验证结构签名目录项预算恰好覆盖有效文件，而第 N+1 个候选触发部分覆盖。
#[test]
fn signature_entry_budget_distinguishes_exact_limit_from_overflow() {
    let exact = tempdir().expect("exact signature root is available");
    write_signature_rollout(&exact.path().join("sessions/rollout-valid.jsonl"));
    let exact_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![exact.path().to_path_buf()],
            max_signature_entries: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    let overflow = tempdir().expect("overflow signature root is available");
    fs::create_dir_all(overflow.path().join("sessions")).expect("signature area is created");
    fs::write(
        overflow.path().join("sessions/rollout-first.jsonl"),
        "{\"type\":\"ordinary\"}\n",
    )
    .expect("first invalid candidate is written");
    fs::write(
        overflow.path().join("sessions/rollout-second.jsonl"),
        "{\"type\":\"ordinary\"}\n",
    )
    .expect("second invalid candidate is written");
    let overflow_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![overflow.path().to_path_buf()],
            max_signature_entries: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(exact_result.coverage.state, CoverageState::Complete);
    assert_eq!(exact_result.roots.len(), 1);
    assert_eq!(overflow_result.coverage.state, CoverageState::Partial);
    assert!(overflow_result.roots.is_empty());
}

/// 验证结构探测目录预算由首候选消耗后，第二个有效候选不能获得新预算。
#[test]
fn signature_directory_budget_is_shared_across_candidates() {
    let consuming_root = tempdir().expect("directory budget consumer is available");
    fs::create_dir_all(consuming_root.path().join("sessions"))
        .expect("empty sessions area is created");
    let valid_root = tempdir().expect("valid root is available");
    write_signature_rollout(
        &valid_root
            .path()
            .join("sessions/rollout-valid-after-directory-budget.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![
                valid_root.path().to_path_buf(),
                consuming_root.path().to_path_buf(),
            ],
            max_signature_directories: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert!(result.roots.is_empty());
}

/// 验证结构探测文件预算由首候选消耗后，第二个有效候选不能获得新预算。
#[test]
fn signature_file_budget_is_shared_across_candidates() {
    let consuming_root = tempdir().expect("file budget consumer is available");
    fs::create_dir_all(consuming_root.path().join("sessions")).expect("sessions area is created");
    fs::write(
        consuming_root.path().join("sessions/rollout-invalid.jsonl"),
        "{\"type\":\"ordinary\"}\n",
    )
    .expect("invalid rollout is written");
    let valid_root = tempdir().expect("valid root is available");
    write_signature_rollout(
        &valid_root
            .path()
            .join("sessions/rollout-valid-after-file-budget.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![
                valid_root.path().to_path_buf(),
                consuming_root.path().to_path_buf(),
            ],
            max_signature_files: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert!(result.roots.is_empty());
}

/// 验证结构探测字节预算由首候选消耗后，第二个有效候选不能获得新预算。
#[test]
fn signature_byte_budget_is_shared_across_candidates() {
    let consuming_root = tempdir().expect("byte budget consumer is available");
    let invalid_line = b"{\"type\":\"ordinary\"}\n";
    fs::create_dir_all(consuming_root.path().join("sessions")).expect("sessions area is created");
    fs::write(
        consuming_root.path().join("sessions/rollout-invalid.jsonl"),
        invalid_line,
    )
    .expect("invalid rollout is written");
    let valid_root = tempdir().expect("valid root is available");
    write_signature_rollout(
        &valid_root
            .path()
            .join("sessions/rollout-valid-after-byte-budget.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![
                valid_root.path().to_path_buf(),
                consuming_root.path().to_path_buf(),
            ],
            max_signature_bytes: u64::try_from(invalid_line.len()).expect("fixture is small"),
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert!(result.roots.is_empty());
}

/// 写入只含最小允许字段的首条会话记录，不包含正文或用户路径。
fn write_signature_rollout(path: &Path) {
    fs::create_dir_all(path.parent().expect("fixture has a parent"))
        .expect("fixture parent is created");
    let mut file = File::create(path).expect("rollout fixture is created");
    writeln!(
        file,
        "{{\"timestamp\":\"2026-07-31T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"synthetic-session\"}}}}"
    )
    .expect("signature line is written");
}
