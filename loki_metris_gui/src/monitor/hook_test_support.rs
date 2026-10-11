//! 测试读取实际 Hook 配置及固定编码载荷，断言语义而不要求明文存储路径。
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

/// 只扩展断言用的正文，原配置和命令从不被修改或执行。
pub(crate) trait HookCommandText {
    fn hook_command_text(&self) -> String;
}
impl HookCommandText for str {
    /// Windows UTF-16LE 固定编码与其他平台明文均保留实际参数原值。
    fn hook_command_text(&self) -> String {
        const PREFIX: &str = "powershell.exe -NoProfile -NonInteractive -EncodedCommand ";
        if let Some(encoded) = self.strip_prefix(PREFIX)
            && let Ok(bytes) = STANDARD.decode(encoded)
            && bytes.len() % 2 == 0
            && let Ok(script) = String::from_utf16(&bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>())
        { return script; }
        let mut content = self.to_owned();
        if let Ok(value) = serde_json::from_str::<Value>(self) { append(&value, &mut content); }
        content
    }
}
/// 只遍历 JSON 字符串；半写入或无效编码不会被误认作已完成的托管命令。
fn append(value: &Value, content: &mut String) {
    match value {
        Value::String(command) => { content.push_str(&command.hook_command_text()); },
        Value::Array(values) => { for value in values { append(value, content); } },
        Value::Object(values) => { for value in values.values() { append(value, content); } },
        _ => {},
    }
}
