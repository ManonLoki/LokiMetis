//! 仅以标准库校验原始节点内容，并将已审阅的标题兼容补丁生成到 OUT_DIR。

use std::{env, fs, path::PathBuf};

#[path = "../gpui-patch-support/sha256.rs"]
mod sha256;

/// 未修改 node.rs 的完整 SHA-256，与 provenance.json 的 original_node_sha256 一致。
const ORIGINAL_NODE_SHA256: &str = "7fcbb84ed76e25d1ba4f69ef646a5b0cd83f96aaccfa7a0b500518da1ef43e91";
/// 补丁结果的完整 SHA-256，确保替换模板仍对应已审阅的 Heading 兼容结果。
const PATCHED_NODE_SHA256: &str = "39381eadab4dff6ca63ffa551aa22cd23e29461f693e9db0493f2fe7f7257f3d";

/// 一次严格原文替换；每个片段必须且只能出现一次，禁止模糊匹配或静默跳过。
struct Replacement {
    name: &'static str,
    before: &'static str,
    after: &'static str,
}

/// 替换模板与显式补丁并存，构建时无需宿主 patch 命令或额外第三方依赖。
const REPLACEMENTS: &[Replacement] = &[
    Replacement {
        name: "01-heading-role",
        before: include_str!("patches/01-heading-role.before"),
        after: include_str!("patches/01-heading-role.after"),
    },
    Replacement {
        name: "02-heading-attributes",
        before: include_str!("patches/02-heading-attributes.before"),
        after: include_str!("patches/02-heading-attributes.after"),
    },
    Replacement {
        name: "03-value-selector",
        before: include_str!("patches/03-value-selector.before"),
        after: include_str!("patches/03-value-selector.after"),
    },
    Replacement {
        name: "04-names-selector",
        before: include_str!("patches/04-names-selector.before"),
        after: include_str!("patches/04-names-selector.after"),
    },
    Replacement {
        name: "05-heading-value",
        before: include_str!("patches/05-heading-value.before"),
        after: include_str!("patches/05-heading-value.after"),
    },
    Replacement {
        name: "06-heading-value-type",
        before: include_str!("patches/06-heading-value-type.before"),
        after: include_str!("patches/06-heading-value-type.after"),
    },
    Replacement {
        name: "07-heading-value-object",
        before: include_str!("patches/07-heading-value-object.before"),
        after: include_str!("patches/07-heading-value-object.after"),
    },
];

/// 仅接受唯一原文位置，否则返回可定位的维护错误，保持失败关闭。
fn replace_unique(source: &mut String, replacement: &Replacement) -> Result<(), String> {
    if replacement.before.is_empty() {
        return Err(format!(
            "{} has an empty original fragment",
            replacement.name
        ));
    }
    let count = source.match_indices(replacement.before).count();
    if count != 1 {
        return Err(format!(
            "{} expected exactly one original fragment, found {count}",
            replacement.name
        ));
    }
    *source = source.replacen(replacement.before, replacement.after, 1);
    Ok(())
}

/// 先验证未修改的完整上游文件，再应用唯一替换并验证最终补丁结果。
fn patched_node(original: &str) -> Result<String, String> {
    let actual = sha256::hex(original.as_bytes());
    if actual != ORIGINAL_NODE_SHA256 {
        return Err(format!(
            "upstream node.rs drift: expected {ORIGINAL_NODE_SHA256}, got {actual}; verify provenance before updating the patch"
        ));
    }
    let mut patched = original.to_owned();
    for replacement in REPLACEMENTS {
        replace_unique(&mut patched, replacement)?;
    }
    if sha256::hex(patched.as_bytes()) != PATCHED_NODE_SHA256 {
        return Err("replacement templates no longer match the reviewed Heading patch".into());
    }
    Ok(patched)
}

/// 从原样第三方源生成完整节点模块，保留其 inner attribute、版权与模块关系。
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=upstream/src/node.rs");
    println!("cargo:rerun-if-changed=patches");

    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let original = fs::read_to_string(manifest.join("upstream/src/node.rs"))
        .expect("read original AccessKit node.rs");
    let patched = patched_node(&original).unwrap_or_else(|error| panic!("{error}"));
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output directory"));
    let generated = format!(
        "// Generated; do not edit. LokiMetris applies only the documented AccessKit Heading patch.\nmod node {{\n{patched}\n}}\n"
    );
    fs::write(output.join("node.rs"), generated).expect("write generated AccessKit node module");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 原始文件任意字节变化都必须在生成前被完整指纹拒绝。
    #[test]
    fn rejects_upstream_byte_drift() {
        let mut original = include_str!("upstream/src/node.rs").to_owned();
        original.push('\n');
        assert!(
            patched_node(&original)
                .unwrap_err()
                .contains("upstream node.rs drift")
        );
    }

    /// 替换目标缺失或重复时必须报错，不能依赖首次匹配继续生成。
    #[test]
    fn rejects_missing_or_ambiguous_fragments() {
        let replacement = Replacement {
            name: "probe",
            before: "needle",
            after: "value",
        };
        for original in ["absent", "needle needle"] {
            assert!(replace_unique(&mut original.to_owned(), &replacement).is_err());
        }
    }

    /// 当前原文与七个模板可以生成已审阅的精确结果，而不是仅通过输入校验。
    #[test]
    fn accepts_current_upstream_and_reviewed_templates() {
        let original = include_str!("upstream/src/node.rs");
        let patched = patched_node(original).expect("current upstream and patch must agree");
        assert_eq!(sha256::hex(patched.as_bytes()), PATCHED_NODE_SHA256);
    }
}
