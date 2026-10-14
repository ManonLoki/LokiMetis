//! Windows 原生 Hook 使用 UTF-16LE 编码的固定字面量调用，不依赖 Agent 的外层 shell。

use base64::{Engine as _, engine::general_purpose::STANDARD};

/// 编码正文只含单个 base64 参数，CMD 和 PowerShell 都不会展开其中的路径。
pub(super) const PREFIX: &str = "powershell.exe -NoProfile -NonInteractive -EncodedCommand ";
const HEAD: &str = "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; try { $s=New-Object System.Diagnostics.ProcessStartInfo; $s.FileName=";
const ARGUMENTS: &str = "; $s.Arguments=[string]::Join(' ', [string[]]@(";
const TAIL: &str = ")); $s.UseShellExecute=$false; $s.CreateNoWindow=$true; $s.RedirectStandardInput=$true; $s.RedirectStandardOutput=$true; $s.RedirectStandardError=$true; $p=[System.Diagnostics.Process]::Start($s); $i=[Console]::OpenStandardInput().CopyToAsync($p.StandardInput.BaseStream); $o=$p.StandardOutput.BaseStream.CopyToAsync([Console]::OpenStandardOutput()); $e=$p.StandardError.BaseStream.CopyToAsync([Console]::OpenStandardError()); [void]$i.GetAwaiter().GetResult(); $p.StandardInput.Close(); $p.WaitForExit(); [void]$o.GetAwaiter().GetResult(); [void]$e.GetAwaiter().GetResult(); exit $p.ExitCode } catch { [Console]::Error.WriteLine('LokiMetris hook relay failed to start.'); exit 1 }";

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

/// 原生程序显式等待并按字节并发转发三个标准流，不对 Hook 输入输出做文本重编码。
pub(super) fn invocation(executable: &str, arguments: &[&str]) -> String {
    let arguments = arguments.iter().map(|value| quote(&windows_argument(value))).collect::<Vec<_>>().join(", ");
    let script = format!("{HEAD}{}{ARGUMENTS}{arguments}{TAIL}", quote(executable));
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!("{PREFIX}{}", STANDARD.encode(bytes))
}

/// ProcessStartInfo 的固定命令行按 Windows argv 规则保护引号与末尾反斜杠。
fn windows_argument(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for character in value.chars() {
        if character == '\\' { slashes += 1; continue; }
        result.extend(std::iter::repeat_n('\\', if character == '"' { slashes * 2 + 1 } else { slashes }));
        result.push(character);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
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
    let units: Vec<_> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let Ok(script) = String::from_utf16(&units) else { return false; };
    let Some(arguments) = relay_arguments(&script) else { return false; };
    !arguments[0].is_empty()
        && arguments[1] == "--loki-metris-hook-relay" && !arguments[3].is_empty()
        && arguments[4] == "--managed-by" && arguments[5] == marker
        && marker.strip_prefix("LokiMetris:tool=") == Some(arguments[2].as_str())
}

/// 兼容已写入的旧编码模板；两代模板都只允许固定 relay 调用，拒绝额外脚本。
fn relay_arguments(script: &str) -> Option<Vec<String>> {
    let mut arguments = Vec::with_capacity(6);
    if let Some(mut remaining) = script.strip_prefix("& ") {
        for index in 0..6 {
            if index != 0 { remaining = remaining.strip_prefix(' ')?; }
            arguments.push(literal(&mut remaining)?);
        }
        return (remaining == "; exit $LASTEXITCODE").then_some(arguments);
    }
    let mut remaining = script.strip_prefix(HEAD)?;
    arguments.push(literal(&mut remaining)?);
    remaining = remaining.strip_prefix(ARGUMENTS)?;
    for index in 0..5 {
        if index != 0 { remaining = remaining.strip_prefix(", ")?; }
        let encoded = literal(&mut remaining)?;
        let value = encoded.strip_prefix('"')?.strip_suffix('"')?;
        // 本产品五个协议参数均不含路径或双引号；不认领非规范 argv 编码。
        if value.contains(['"', '\\']) { return None; }
        arguments.push(value.to_owned());
    }
    (remaining == TAIL).then_some(arguments)
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
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        for name in ["ordinary relay.exe", "中文 $env-OS %OS% !OS! ` ' ‘ ’ ‚ ‛ “ ” „ & relay.exe"] {
        let executable = root.path().join(name);
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let command = invocation(executable.to_str().unwrap(), &["--exact", "agent_hooks::generation::windows_command::tests::relay_stdio_fixture", "--nocapture"]);
        // 外层 PowerShell 的 -Command 默认将非零退出转成1，显式读取实际子进程退出码。
        let powershell = format!("{command}; exit $LASTEXITCODE");
        for (host, arguments) in [("cmd.exe", vec!["/v:on", "/d", "/s", "/c", command.as_str()]),
            ("WindowsPowerShell/v1.0/powershell.exe", vec!["-NoProfile", "-NonInteractive", "-Command", powershell.as_str()])] {
            let output = Command::new(system.join(host)).args(arguments).env("LOKI_HOOK_STDIO_FIXTURE", "exit").creation_flags(0x0800_0000).output().unwrap();
            assert_eq!(output.status.code(), Some(7), "{host}: {}", String::from_utf8_lossy(&output.stderr));
        }
        }
    }

    /// GUI 子系统与 console 都必须继承 stdin/stdout，等待 relay 完成并透传退出码。
    #[cfg(windows)]
    #[test]
    fn real_windows_shells_wait_for_gui_subsystem_and_preserve_standard_streams() {
        use std::{io::Write, process::{Command, Stdio}};
        use std::os::windows::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        for subsystem in [2_u16, 3] {
            let executable = root.path().join(format!("owned-subsystem-{subsystem}.exe"));
            let mut bytes = std::fs::read(std::env::current_exe().unwrap()).unwrap();
            let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
            assert_eq!(&bytes[pe..pe + 4], b"PE\0\0");
            bytes[pe + 24 + 68..pe + 24 + 70].copy_from_slice(&subsystem.to_le_bytes());
            std::fs::write(&executable, bytes).unwrap();
            let command = invocation(executable.to_str().unwrap(), &["--exact", "agent_hooks::generation::windows_command::tests::relay_stdio_fixture", "--nocapture"]);
            for host in ["cmd.exe", "WindowsPowerShell/v1.0/powershell.exe"] {
                let mut process = Command::new(system.join(host));
                if host == "cmd.exe" { process.args(["/d", "/s", "/c", &command]); }
                else { process.args(["-NoProfile", "-NonInteractive", "-Command", &format!("{command}; exit $LASTEXITCODE")]); }
                let mut child = process.env("LOKI_HOOK_STDIO_FIXTURE", "stdio").creation_flags(0x0800_0000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
                let input = "hook-standard-input 中文\r\n".as_bytes();
                child.stdin.take().unwrap().write_all(input).unwrap();
                let output = child.wait_with_output().unwrap();
                assert_eq!(output.status.code(), Some(7), "{host}/{subsystem}: {}", String::from_utf8_lossy(&output.stderr));
                assert!(output.stdout.ends_with(input), "{host}/{subsystem}: stdin/stdout bytes must be preserved: {:?}", output.stdout);
                assert!(output.stderr.is_empty(), "{host}/{subsystem}: {}", String::from_utf8_lossy(&output.stderr));
            }
        }
    }

    /// 仅由带独立环境标记的子进程执行真实 stdio 协议，普通测试运行不退出宿主。
    #[cfg(windows)]
    #[test]
    fn relay_stdio_fixture() {
        use std::io::{Read, Write};
        let mode = std::env::var("LOKI_HOOK_STDIO_FIXTURE");
        if mode.as_deref() == Ok("exit") { std::process::exit(7); }
        if mode.as_deref() != Ok("stdio") { return; }
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes).unwrap();
        std::io::stdout().write_all(&bytes).unwrap();
        std::io::stdout().flush().unwrap();
        std::process::exit(7);
    }

    /// 已删除或不可启动的安装路径必须返回失败，不能把空 LASTEXITCODE 当作成功。
    #[cfg(windows)]
    #[test]
    fn missing_windows_relay_executable_returns_failure() {
        use std::{process::Command, os::windows::process::CommandExt};
        let root = tempfile::tempdir().unwrap();
        let command = invocation(root.path().join("missing.exe").to_str().unwrap(), &[]);
        let output = Command::new("cmd.exe").args(["/d", "/s", "/c", &command]).creation_flags(0x0800_0000).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("LokiMetris hook relay failed to start."));
    }
}
