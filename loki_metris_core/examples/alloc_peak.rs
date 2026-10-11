//! 度量 canonical 去重与联合窗口构建的 Rust 堆峰值：计数分配器记录同时存活字节的高水位。
//! 用法：`cargo run --release -p loki_metris_core --example alloc_peak`。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use loki_metris_core::*;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            if new_size >= layout.size() {
                let live = LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size - layout.size();
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// 形状贴近真实索引：少量项目/线程/来源被大量调用重复引用。
fn calls(count: usize) -> Vec<UsageCall> {
    (0..count)
        .map(|index| {
            let thread = index % 360;
            UsageCall {
                logical_call_id: format!("call-{index:016x}-{:08x}", index.wrapping_mul(2654435761)),
                occurred_at_epoch_ms: 1_760_000_000_000 + index as i64 * 60_000,
                model: Some("gpt-5-codex".to_owned()),
                reasoning_effort: Some("medium".to_owned()),
                project_key: Some(format!("project-{:016x}", index % 32)),
                project_label: Some(format!("proj{}", index % 32)),
                thread_key: format!("thread-{thread:016x}"),
                thread_label: Some(format!("thread title {thread}")),
                usage: TokenUsage::new(1000 + index as u64, 300, None, 100, 0, Some(1100 + index as u64)).unwrap(),
                adapter_consistency_key: Some(format!("consistency-{index:016x}")),
                confidence: Confidence::Exact,
                provenance: vec![SourceProvenance {
                    source_id: format!("source-{:016x}", index % 373),
                    root_id: "root-00000000deadbeef".to_owned(),
                    relative_label: format!("sessions/2026/10/{:02}/rollout-{}.jsonl", index % 28 + 1, index % 373),
                    archived: false,
                }],
            }
        })
        .collect()
}

/// 返回闭包执行期间相对起点的峰值增量与结束时的存活增量。
fn measure<T>(label: &str, work: impl FnOnce() -> T) -> T {
    let start = LIVE.load(Ordering::Relaxed);
    PEAK.store(start, Ordering::Relaxed);
    let result = work();
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(start);
    let kept = LIVE.load(Ordering::Relaxed).saturating_sub(start);
    println!("{label:<34} peak +{:>7.2} MiB   kept +{:>7.2} MiB", peak as f64 / 1048576.0, kept as f64 / 1048576.0);
    result
}

/// 带一个已启用根的 Agent 快照，调用先做 canonical 去重。
fn agent(client: SourceClientKind, canonical: CanonicalUsageSet) -> AgentUsageSnapshot {
    let snapshot = UsageSnapshot {
        canonical,
        index_state: LocalIndexState::Ready,
        read_ready: true,
        roots: vec![RootRecord {
            root_id: "root-00000000deadbeef".to_owned(),
            alias: "测试".to_owned(),
            enabled: true,
            activation_state: RootActivationState::Ready,
            is_primary: true,
            discovery_method: DiscoveryMethod::Registered,
            last_coverage: Some(CoverageState::Complete),
            source_file_count: 373,
            call_observation_count: 25_000,
        }],
    };
    let coverage = CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    };
    AgentUsageSnapshot::new(client, snapshot, coverage, Some("parser-1".to_owned()))
}

fn main() {
    let count = 50_000;
    let input = measure("build 50k calls (input)", || calls(count));
    let canonical = measure("canonicalize_usage_calls", || canonicalize_usage_calls(input));
    println!("canonical calls: {}", canonical.calls.len());
    drop(canonical);

    // 两个 Agent 各 2.5 万条，贴近真实"全部"视图。
    let first = canonicalize_usage_calls(calls(count / 2));
    let second = canonicalize_usage_calls(calls(count / 2));
    let inputs = vec![agent(SourceClientKind::Codex, first), agent(SourceClientKind::ClaudeCode, second)];
    let zone = jiff::tz::TimeZone::UTC;
    let combined = measure("combine_agent_usage_snapshots", || combine_agent_usage_snapshots(inputs).unwrap());
    let now = 1_760_000_000_000 + 50_000_i64 * 60_000;
    let records = measure("build_combined_local_windows", || {
        build_combined_local_windows_with_standard(&combined, now, TimeStandard::Local, &zone).unwrap()
    });
    println!("windows: {}", records.windows.len());
}
