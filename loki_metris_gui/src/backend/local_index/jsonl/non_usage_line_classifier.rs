// 本文件是 non_usage_line.rs 的分片（第 2/3 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：初始化固定上限的分类状态，不分配与正文长度相关的内存。

impl NonUsageJsonLine {
    /// 初始化固定上限的分类状态，不分配与正文长度相关的内存。
    pub(super) fn new() -> Self {
        Self {
            valid: true,
            root_done: false,
            frames: Vec::with_capacity(MAX_DEPTH),
            mode: Mode::Idle,
            capture: Capture::Discard,
            string_is_key: false,
            label: Vec::with_capacity(MAX_LABEL_BYTES),
            atom: Vec::with_capacity(MAX_ATOM_BYTES),
            envelope_kind: None,
            payload_kind: None,
            utf8_left: 0,
            utf8_codepoint: 0,
            utf8_min: 0,
        }
    }

    /// 输入任意切块，包括初次越限时重放的有界前缀。
    pub(super) fn push(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.push_byte(*byte);
        }
    }

    /// 消费单字节；一旦失败便常量成本跳过剩余正文，等待原扫描器换行。
    pub(super) fn push_byte(&mut self, byte: u8) {
        if !self.valid {
            return;
        }
        if !self.accept_utf8(byte) {
            self.valid = false;
            return;
        }
        match self.mode {
            Mode::Idle => self.idle(byte),
            Mode::Atom => {
                if byte.is_ascii_whitespace() || matches!(byte, b',' | b']' | b'}') {
                    self.finish_atom();
                    if self.valid {
                        self.idle(byte);
                    }
                } else if self.atom.len() < MAX_ATOM_BYTES {
                    self.atom.push(byte);
                } else {
                    self.valid = false;
                }
            }
            Mode::String { escape, hex_left } => {
                if self.capture != Capture::Discard {
                    if self.label.len() >= MAX_LABEL_BYTES {
                        self.valid = false;
                        return;
                    }
                    self.label.push(byte);
                }
                if hex_left != 0 {
                    if !byte.is_ascii_hexdigit() {
                        self.valid = false;
                    }
                    self.mode = Mode::String {
                        escape: false,
                        hex_left: hex_left - 1,
                    };
                } else if escape {
                    self.mode = Mode::String {
                        escape: false,
                        hex_left: if byte == b'u' { 4 } else { 0 },
                    };
                    if !matches!(
                        byte,
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u'
                    ) {
                        self.valid = false;
                    }
                } else if byte == b'"' {
                    self.finish_string();
                } else if byte == b'\\' {
                    self.mode = Mode::String {
                        escape: true,
                        hex_left: 0,
                    };
                } else if byte < 0x20 {
                    self.valid = false;
                }
            }
        }
    }

    /// 仅在整行闭合、语法有效且最外层为已知非用量记录时解除超长警告。
    pub(super) fn is_complete_non_usage(mut self) -> bool {
        if matches!(self.mode, Mode::Atom) {
            self.finish_atom();
        }
        self.valid
            && self.utf8_left == 0
            && self.root_done
            && self.frames.is_empty()
            && matches!(self.mode, Mode::Idle)
            && !matches!(
                self.payload_kind.as_deref(),
                Some(
                    "token_count"
                        | "token_usage_record"
                        | "session_meta"
                        | "turn_context"
                        | "task_started"
                        | "thread_name_updated"
                )
            )
            && is_known_non_usage_event(self.envelope_kind.as_deref(), self.payload_kind.as_deref())
    }

    /// 校验跨块 UTF-8 标量，拒绝截断、过长编码、surrogate 和超出 Unicode 范围。
    fn accept_utf8(&mut self, byte: u8) -> bool {
        if self.utf8_left != 0 {
            if !(0x80..=0xbf).contains(&byte) {
                return false;
            }
            self.utf8_codepoint = (self.utf8_codepoint << 6) | u32::from(byte & 0x3f);
            self.utf8_left -= 1;
            return self.utf8_left != 0
                || (self.utf8_codepoint >= self.utf8_min
                    && self.utf8_codepoint <= 0x10ffff
                    && !(0xd800..=0xdfff).contains(&self.utf8_codepoint));
        }
        let (left, mask, min) = match byte {
            0..=0x7f => return true,
            0xc2..=0xdf => (1, 0x1f, 0x80),
            0xe0..=0xef => (2, 0x0f, 0x800),
            0xf0..=0xf4 => (3, 0x07, 0x10000),
            _ => return false,
        };
        self.utf8_left = left;
        self.utf8_codepoint = u32::from(byte & mask);
        self.utf8_min = min;
        true
    }

    /// 返回当前容器所需 token；根值结束后只允许空白。
    fn expected(&self) -> Expect {
        self.frames
            .last()
            .map(|f| f.expect)
            .unwrap_or(if self.root_done {
                Expect::End
            } else {
                Expect::Value
            })
    }

    /// 在字符串/原子之外检查结构标点和合法 token 起点。
    fn idle(&mut self, byte: u8) {
        if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
            return;
        }
        let expected = self.expected();
        match byte {
            b':' if expected == Expect::Colon => {
                self.frames.last_mut().unwrap().expect = Expect::Value
            }
            b',' if expected == Expect::CommaOrEnd => {
                let frame = self.frames.last_mut().unwrap();
                frame.expect = if frame.object {
                    Expect::Key
                } else {
                    Expect::Value
                };
            }
            b'}' => self.close(true),
            b']' => self.close(false),
            b'"' if matches!(
                expected,
                Expect::FirstKey | Expect::Key | Expect::Value | Expect::FirstValue
            ) =>
            {
                self.start_string()
            }
            b'{' | b'[' if matches!(expected, Expect::Value | Expect::FirstValue) => {
                self.open(byte == b'{')
            }
            b'-' | b'0'..=b'9' | b't' | b'f' | b'n'
                if matches!(expected, Expect::Value | Expect::FirstValue) =>
            {
                if self.named_type_value() {
                    self.valid = false;
                    return;
                }
                self.atom.clear();
                self.atom.push(byte);
                self.mode = Mode::Atom;
            }
            _ => self.valid = false,
        }
    }

    /// 类型字段只能为字符串，不能让内层对象或数字伪装为已知事件。
    fn named_type_value(&self) -> bool {
        self.frames
            .last()
            .is_some_and(|f| f.object && f.role != Role::Other && f.key == Key::Type)
    }

    /// 根 payload 允许对象或 null，其余类型与最小协议结构不相容。
    fn named_payload_value(&self) -> bool {
        self.frames
            .last()
            .is_some_and(|f| f.object && f.role == Role::Envelope && f.key == Key::Payload)
    }

    /// 打开有界容器；只有直接的根 payload 对象拥有 payload 类型路由权。
    fn open(&mut self, object: bool) {
        if self.named_type_value()
            || (self.named_payload_value() && !object)
            || self.frames.len() >= MAX_DEPTH
        {
            self.valid = false;
            return;
        }
        let role = if self.frames.is_empty() && object {
            Role::Envelope
        } else if self.named_payload_value() {
            Role::Payload
        } else {
            Role::Other
        };
        self.frames.push(Frame {
            object,
            role,
            expect: if object {
                Expect::FirstKey
            } else {
                Expect::FirstValue
            },
            key: Key::Other,
            seen_type: false,
            seen_payload: false,
        });
    }

    /// 闭合容器并推进父值，拒绝错配括号、缺值和尾逗号。
    fn close(&mut self, object: bool) {
        let allowed = self.frames.last().is_some_and(|f| {
            f.object == object
                && matches!(
                    f.expect,
                    Expect::CommaOrEnd | Expect::FirstKey | Expect::FirstValue
                )
        });
        if !allowed {
            self.valid = false;
            return;
        }
        self.frames.pop();
        self.finish_value();
    }

    /// 开始字符串，最多捕获两层协议键或其 type 值，其余正文完全丢弃。
    fn start_string(&mut self) {
        self.string_is_key = matches!(self.expected(), Expect::FirstKey | Expect::Key);
        if !self.string_is_key && self.named_payload_value() {
            self.valid = false;
            return;
        }
        self.capture = if self.string_is_key {
            if self.frames.last().is_some_and(|f| f.role != Role::Other) {
                Capture::Key
            } else {
                Capture::Discard
            }
        } else if self.named_type_value() {
            Capture::Type(self.frames.last().unwrap().role)
        } else {
            Capture::Discard
        };
        self.label.clear();
        if self.capture != Capture::Discard {
            self.label.push(b'"');
        }
        self.mode = Mode::String {
            escape: false,
            hex_left: 0,
        };
    }

    /// 完成字符串，解码有界协议字段并拒绝重复的类型或 payload 路由键。
    fn finish_string(&mut self) {
        let decoded = if self.capture != Capture::Discard {
            match serde_json::from_slice::<String>(&self.label) {
                Ok(value) => Some(value),
                Err(_) => {
                    self.valid = false;
                    return;
                }
            }
        } else {
            None
        };
        self.mode = Mode::Idle;
        if self.string_is_key {
            let frame = self.frames.last_mut().unwrap();
            let key = match decoded.as_deref() {
                Some("type") => Key::Type,
                Some("payload") if frame.role == Role::Envelope => Key::Payload,
                _ => Key::Other,
            };
            if key == Key::Type {
                if frame.seen_type {
                    self.valid = false;
                    return;
                }
                frame.seen_type = true;
            } else if key == Key::Payload {
                if frame.seen_payload {
                    self.valid = false;
                    return;
                }
                frame.seen_payload = true;
            }
            frame.key = key;
            frame.expect = Expect::Colon;
        } else {
            match self.capture {
                Capture::Type(Role::Envelope) => self.envelope_kind = decoded,
                Capture::Type(Role::Payload) => self.payload_kind = decoded,
                _ => {}
            }
            self.finish_value();
        }
    }

    /// 用 serde 验证固定大小的数字/布尔/null 语法，不放宽普通 JSON 接受集。
    fn finish_atom(&mut self) {
        if serde_json::from_slice::<serde::de::IgnoredAny>(&self.atom).is_err()
            || (self.named_payload_value() && self.atom != b"null")
        {
            self.valid = false;
            return;
        }
        self.mode = Mode::Idle;
        self.finish_value();
    }

    /// 一个值结束后只允许父容器逗号/闭合，或根后的空白。
    fn finish_value(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            frame.expect = Expect::CommaOrEnd;
        } else {
            self.root_done = true;
        }
    }
}
