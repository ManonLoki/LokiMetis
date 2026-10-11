//! 对超长行做有界 JSON 语法与外层类型检查，不缓存正文、不凭前缀放行。
//!
//! 字符串正文仅验证 UTF-8、控制字节及 JSON 转义语法；和 serde IgnoredAny
//! 一样不解释正文中的 Unicode surrogate。被读取的键与类型则由 serde 解码。
//! 只保证完整语法与安全外层路由，不逐字段复刻普通行的可选字段 schema。

use super::context::is_known_non_usage_event;

/// 最多保留 128 层容器、512 字节键/类型和 128 字节数字/字面量。
const MAX_DEPTH: usize = 128;
const MAX_LABEL_BYTES: usize = 512;
const MAX_ATOM_BYTES: usize = 128;

/// 标识容器所在的协议位置，内层 type 不能冒充信封或 payload 类型。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Envelope,
    Payload,
    Other,
}

/// 当前 map 键的协议含义；未知字段的正文只验证语法。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Type,
    Payload,
    Other,
}

/// JSON 容器的下一合法语法位置，区分首项和逗号后以拒绝尾逗号。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    FirstKey,
    Key,
    Colon,
    Value,
    FirstValue,
    CommaOrEnd,
    End,
}

/// 有界栈帧只持有语法状态和两个协议字段的重复标志。
struct Frame {
    object: bool,
    role: Role,
    expect: Expect,
    key: Key,
    seen_type: bool,
    seen_payload: bool,
}

/// 当前词法阶段；字符串不保留未知正文，原子值按固定上限校验。
#[derive(Clone, Copy)]
enum Mode {
    Idle,
    String { escape: bool, hex_left: u8 },
    Atom,
}

/// 指定当前字符串需要保留的最小协议信息。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Capture {
    Key,
    Type(Role),
    Discard,
}

/// 一个完整超长行的流式验证状态；任何不确定、损坏或超预算都保持警告。
pub(super) struct NonUsageJsonLine {
    valid: bool,
    root_done: bool,
    frames: Vec<Frame>,
    mode: Mode,
    capture: Capture,
    string_is_key: bool,
    label: Vec<u8>,
    atom: Vec<u8>,
    envelope_kind: Option<String>,
    payload_kind: Option<String>,
    utf8_left: u8,
    utf8_codepoint: u32,
    utf8_min: u32,
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - non_usage_line_classifier.rs：初始化固定上限的分类状态，不分配与正文长度相关的内存。
// - non_usage_line_tests.rs：本模块的单元测试
include!("non_usage_line_classifier.rs");
include!("non_usage_line_tests.rs");
