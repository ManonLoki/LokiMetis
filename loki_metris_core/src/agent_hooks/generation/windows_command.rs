//! 含 shell 展开字符的 Windows 安装路径使用 UTF-16LE 编码的固定字面量调用。

use base64::{Engine as _, engine::general_purpose::STANDARD};

/// 编码正文只含单个 base64 参数，CMD 和 PowerShell 都不会展开其中的路径。
pub(super) const PREFIX: &str = "powershell.exe -NoProfile -NonInteractive -EncodedCommand ";

/// PowerShell 也把四种 Unicode 单引号作为分隔符，必须与 ASCII 引号一样成对转义。
fn single_quote(value: char) -> bool { matches!(value, '\'' | '\u{2018}'..='\u{201b}') }

/// 单引号字面量保留路径原值；转义集合与 PowerShell CharTraits.IsSingleQuote 一致。
fn quote(value: &str) -> String {
    let mut quoted = String::from("'");
    for character in value.chars() {
        quoted.push(character);
        if single_quote(character) { quoted.push(character); }
    }
    quoted.push('\'');
    quoted
}

/// 直接调用原生程序并透传退出码；不读取或重编码 Hook 标准输入。
pub(super) fn invocation(executable: &str, arguments: &[&str]) -> String {
    let mut script = format!("& {}", quote(executable));
    for argument in arguments { script.push(' '); script.push_str(&quote(argument)); }
    script.push_str("; exit $LASTEXITCODE");
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!("{PREFIX}{}", STANDARD.encode(bytes))
}

/// 严格解析一个 PowerShell 单引号字面量，拒绝变量、表达式或额外操作符。
fn literal(input: &mut &str) -> Option<String> {
    *input = input.strip_prefix('\'')?;
    let mut value = String::new();
    loop {
        let index = input.find(single_quote)?;
        value.push_str(&input[..index]);
        let character = input[index..].chars().next()?;
        *input = &input[index + character.len_utf8()..];
        if let Some(rest) = input.strip_prefix(character) { value.push(character); *input = rest; }
        else { return Some(value); }
    }
}

/// 仅固定六参数 relay 调用属于本应用，任意编码脚本提及 marker 不能被接管。
pub(super) fn has_marker(command: &str, marker: &str) -> bool {
    let Some(encoded) = command.strip_prefix(PREFIX) else { return false; };
    let Ok(bytes) = STANDARD.decode(encoded) else { return false; };
    if bytes.len() % 2 != 0 { return false; }
    let units: Vec<_> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let Ok(script) = String::from_utf16(&units) else { return false; };
    let Some(mut remaining) = script.strip_prefix("& ") else { return false; };
    let mut arguments = Vec::with_capacity(6);
    for index in 0..6 {
        if index != 0 {
            let Some(rest) = remaining.strip_prefix(' ') else { return false; };
            remaining = rest;
        }
        let Some(value) = literal(&mut remaining) else { return false; };
        arguments.push(value);
    }
    remaining == "; exit $LASTEXITCODE" && !arguments[0].is_empty()
        && arguments[1] == "--loki-metris-hook-relay" && !arguments[3].is_empty()
        && arguments[4] == "--managed-by" && arguments[5] == marker
        && marker.strip_prefix("LokiMetris:tool=") == Some(arguments[2].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 特殊字符必须保留字面量，完整身份及固定脚本以外的载荷不能命中管理标识。
    #[test]
    fn encoded_relay_identity_is_exact_and_rejects_arbitrary_scripts() {
        let path = "C:\\中文 $env:OS %OS% !OS! ` ' ‘ ’ ‚ ‛ “ ” „ &\\relay.exe";
        let marker = "LokiMetris:tool=codex";
        let command = invocation(path, &["--loki-metris-hook-relay", "codex", "Stop", "--managed-by", marker]);
        assert!(has_marker(&command, marker));
        assert!(!has_marker(&command, "LokiMetris:tool=claude-code"));
        assert!(!has_marker(&(command.clone() + " extra"), marker));
        assert!(!has_marker(&invocation(path, &[marker]), marker));
        let mismatched = invocation(path, &["--loki-metris-hook-relay", "claude-code", "Stop", "--managed-by", marker]);
        assert!(!has_marker(&mismatched, marker));
    }

    /// 两种真实 Windows 宿主必须启动特殊路径的同一原生程序并透传退出码。
    #[cfg(windows)]
    #[test]
    fn real_windows_shells_preserve_special_executable_path_and_exit_code() {
        use std::process::Command;
        use std::os::windows::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("中文 $env-OS %OS% !OS! ` ' ‘ ’ ‚ ‛ “ ” „ & relay.exe");
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        std::fs::copy(system.join("cmd.exe"), &executable).unwrap();
        let command = invocation(executable.to_str().unwrap(), &["/d", "/c", "exit 7"]);
        // 外层 PowerShell 的 -Command 默认将非零退出转成1，显式读取实际子进程退出码。
        let powershell = format!("{command}; exit $LASTEXITCODE");
        for (host, arguments) in [("cmd.exe", vec!["/v:on", "/d", "/s", "/c", command.as_str()]),
            ("WindowsPowerShell/v1.0/powershell.exe", vec!["-NoProfile", "-NonInteractive", "-Command", powershell.as_str()])] {
            let output = Command::new(system.join(host)).args(arguments).creation_flags(0x0800_0000).output().unwrap();
            assert_eq!(output.status.code(), Some(7), "{host}: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
}
