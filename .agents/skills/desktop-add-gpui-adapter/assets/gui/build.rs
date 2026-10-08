//! 只消费候选工具已校验并生成的静态发布条目；普通开发构建使用真实空状态。

use std::{env, fs, path::PathBuf};

/// Cargo 在选择或内容变化时重新编译，更新日志不写回源码也不在窗口线程读取。
fn main() {
    const INPUT: &str = "HARNESS_GPUI_RELEASE_NOTES_RS";
    println!("cargo:rerun-if-env-changed={INPUT}");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo 必须提供 OUT_DIR")).join("release_notes.rs");
    if let Some(input) = env::var_os(INPUT) {
        let input = PathBuf::from(input);
        assert!(input.is_absolute(), "候选静态更新日志必须使用绝对路径");
        let text = input.to_str().expect("候选静态更新日志路径必须是 UTF-8");
        assert!(!text.chars().any(char::is_control), "候选静态更新日志路径不得包含控制字符");
        let metadata = fs::symlink_metadata(&input).expect("候选静态更新日志必须存在");
        assert!(metadata.is_file() && metadata.len() <= 8 * 1024 * 1024, "候选静态更新日志必须是有界普通文件");
        println!("cargo:rerun-if-changed={text}");
        fs::copy(&input, output).expect("无法嵌入已验证的候选更新日志");
    } else {
        fs::write(output, "pub const RELEASE_NOTES_JSON: &[u8] = &[];\npub const RELEASE_NOTES: &[(&str, &str, &[(&str, &str)], &[(&str, &str)])] = &[];\n")
            .expect("无法建立空更新日志状态");
    }
}
