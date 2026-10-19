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
    /// 顶层信封。
    Envelope,
    /// 信封的 payload 对象。
    Payload,
    /// 其它嵌套容器。
    Other,
}

/// 当前 map 键的协议含义；未知字段的正文只验证语法。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    /// type 键。
    Type,
    /// payload 键。
    Payload,
    /// 其它键，只验证语法。
    Other,
}

/// JSON 容器的下一合法语法位置，区分首项和逗号后以拒绝尾逗号。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// 对象的第一个键，或空对象结束。
    FirstKey,
    /// 逗号之后的键，不允许直接结束。
    Key,
    /// 键之后的冒号。
    Colon,
    /// 冒号或逗号之后的值。
    Value,
    /// 数组的第一个值，或空数组结束。
    FirstValue,
    /// 值之后的逗号或容器结束。
    CommaOrEnd,
    /// 容器已结束。
    End,
}

/// 有界栈帧只持有语法状态和两个协议字段的重复标志。
struct Frame {
    /// 是对象而非数组。
    object: bool,
    /// 容器所在的协议位置。
    role: Role,
    /// 下一个合法语法位置。
    expect: Expect,
    /// 当前键的协议含义。
    key: Key,
    /// 已出现过 type 键，重复即判为无效。
    seen_type: bool,
    /// 已出现过 payload 键，重复即判为无效。
    seen_payload: bool,
}

/// 当前词法阶段；字符串不保留未知正文，原子值按固定上限校验。
#[derive(Clone, Copy)]
enum Mode {
    /// 在值或标点之间。
    Idle,
    /// 字符串内部；附转义状态与剩余十六进制位数。
    String { escape: bool, hex_left: u8 },
    /// 数字或字面量内部。
    Atom,
}

/// 指定当前字符串需要保留的最小协议信息。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Capture {
    /// 保留键名以识别协议字段。
    Key,
    /// 保留指定层级的 type 值。
    Type(Role),
    /// 只验证不保留。
    Discard,
}

/// 一个完整超长行的流式验证状态；任何不确定、损坏或超预算都保持警告。
pub(super) struct NonUsageJsonLine {
    /// 目前为止语法与预算均合法；一旦为 false 不再恢复。
    valid: bool,
    /// 顶层值已经完整结束，之后只允许空白。
    root_done: bool,
    /// 有界的容器栈。
    frames: Vec<Frame>,
    /// 当前词法阶段。
    mode: Mode,
    /// 当前字符串需要保留的信息。
    capture: Capture,
    /// 当前字符串是对象键而非值。
    string_is_key: bool,
    /// 正在捕获的键名或 type 值，受固定字节上限约束。
    label: Vec<u8>,
    /// 正在读取的数字或字面量（true/false/null），受固定字节上限约束。
    atom: Vec<u8>,
    /// 信封层 type 字段的值。
    envelope_kind: Option<String>,
    /// payload 层 type 字段的值。
    payload_kind: Option<String>,
    /// 当前多字节 UTF-8 字符还缺的后续字节数。
    utf8_left: u8,
    /// 正在累积的码点。
    utf8_codepoint: u32,
    /// 该码点长度允许的最小值，用于拒绝过长编码。
    utf8_min: u32,
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - non_usage_line_classifier.rs：初始化固定上限的分类状态，不分配与正文长度相关的内存。
// - non_usage_line_tests.rs：本模块的单元测试
include!("non_usage_line_classifier.rs");
include!("non_usage_line_tests.rs");
